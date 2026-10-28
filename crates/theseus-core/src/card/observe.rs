//! The exchange-end observer (theseus-6f8t0, option A). Today the card learns
//! about the owner from the imported archive and from whole-message
//! directives; a fact said in passing inside a work request ("fix the build.
//! I'm on the M1 now, by the way") is lost. When a private conversation the
//! owner is alone in reaches a due point, one cheap call on `[card] profile`
//! reads the owner's own lines since the session's mark and keeps what they
//! say about the owner that should hold from now on, as unconfirmed
//! `card.obs.<id>` records (`state::obs_record`). They are private notes:
//! never a node, never indexed, never recalled, in no prompt and in no shared
//! place. The card's live pass turns them into questions.
//!
//! It copies `people.v1` live (`judge/people/live.rs`) and keeps its
//! discipline: never a task's session, never a place the owner is not alone
//! in (`capture::owner_alone`), spawned after the turn's last frame and one
//! decision a session at a time, a due rule (`categorize::due`) and a mark of
//! its own (META `card.observe.<session>`), the lines scrubbed, the call
//! through the session's Google and privacy marks and so the data gate (a
//! China-origin `[card] profile` is refused there), reserved on `[card]
//! day_usd` and the day ceiling first.
//!
//! - **Which lines.** Only the owner's own human messages after the mark: a
//!   node does not say whether a job sent it, so the inbound step marks the
//!   nodes `turn.submit` noted as the owner's (`capture::expect`: not a job's
//!   `opened_from`, not the MCP server's or an unnamed surface's, not a
//!   prompt's, none with files). The mark is in memory, so after a restart
//!   the earlier lines are not read rather than guessed at. At most
//!   [`extract::MAX_LINES`] a call, the oldest first, and the mark moves only
//!   as far as the last line read, so no line is passed unread.
//! - **What is kept** is code's choice (`extract::parse`, `extract::filter`);
//!   a line that says a live record again is counted like capture's
//!   `standing.repeated` and writes no observation.
//! - **One frame.** The observations, the moved mark, the day's spend and the
//!   `card.observed` row are written together, so a crash between the call
//!   and the frame loses the call and nothing else: the mark is unmoved and
//!   the next due point reads the same lines again. A call that fails writes
//!   nothing.
//! - **The sweep** ([`sweep`], nightly, called first by the live pass)
//!   reads each such session's lines past its mark, due or not, so the end of
//!   the day's last conversation is not lost. Its rows say `sweep`.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use theseus_kernel::micros_to_usd;
use theseus_protocol::{CardObservation, StandingKind, CARD_OBSERVE_MARK_PREFIX};
use theseus_store::{blocking, kinds, NewRecord};

use self::extract::{Held, OwnerLine, Shown};
use super::state::{self, State};
use crate::fact::card::CardObserved;
use crate::judge::categorize::{due, is_human, ExchangeEnd, Mark};
use crate::judge::{spend, JudgeService};
use crate::node::{Body, Node};
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::standing;

pub mod extract;
#[cfg(test)]
mod tests;

/// A sweep reads the sessions active in this long, and, for a session with
/// no mark, the lines written in it.
pub const SWEEP_WINDOW_MS: u64 = 48 * 3_600_000;
/// The most sessions one sweep reads.
pub const SWEEP_SESSIONS: usize = 40;

/// The sessions whose decision is running.
#[derive(Default)]
pub struct Point {
    deciding: Mutex<HashSet<String>>,
    /// A due point with none of the owner's lines moves its mark here, in
    /// memory, and writes no frame: a job's or a server's conversation costs
    /// the store nothing. After a restart the mark is the stored one.
    passed: Mutex<std::collections::HashMap<String, Mark>>,
}

/// The sessions whose passed mark is kept in memory, at most.
const PASSED_KEPT: usize = 1024;

/// What a pass did.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    /// Not due, no owner text, or nothing to read.
    Idle,
    /// The mark moved with no call (a due point with none of the owner's lines).
    Passed,
    /// The call was not made: its reason.
    Stopped(String),
    /// The call failed: nothing was written.
    Failed(String),
    /// The call was made and its frame written.
    Done(Report),
}

/// The owner's lines to read, and the position and time the mark moves to.
struct Read {
    lines: Vec<OwnerLine>,
    through: (u64, u64),
}

/// What `observe_ask` came back with.
enum Asked {
    Answer(
        Box<crate::provider::ModelResponse>,
        theseus_judge::price::Micros,
    ),
    Not(Seen),
}

/// A call's numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub returned: u32,
    pub kept: u32,
    pub restated: u32,
    pub cost_usd: f64,
}

impl JudgeService {
    /// A conversation's exchange end: whether the owner's lines are observed
    /// is decided in a task of its own. Returns at once.
    pub fn observe_at_exchange_end(&self, end: &ExchangeEnd, task: bool) {
        if task {
            return;
        }
        let Some(core) = self.core() else { return };
        if !core.observing() || !core.owner_alone_in(&end.session_id) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let sid = end.session_id.clone();
        if !self.observing.deciding.lock().unwrap().insert(sid.clone()) {
            return;
        }
        let weak = Arc::downgrade(&core);
        drop(core);
        rt.spawn(async move {
            if let Some(core) = weak.upgrade() {
                match core.observe_session(&sid, "live").await {
                    Ok(Seen::Failed(why) | Seen::Stopped(why)) => {
                        tracing::info!(session = %sid, %why, "card: nothing observed");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(session = %sid, error = %format!("{e:#}"), "card: an exchange was not observed");
                    }
                }
                core.runner
                    .judge
                    .observing
                    .deciding
                    .lock()
                    .unwrap()
                    .remove(&sid);
            }
        });
    }
}

impl Core {
    /// Whether the observer is on: `[card] observe` and a day line above 0.
    fn observing(&self) -> bool {
        self.cfg.card.observe && self.cfg.card.day_usd > 0.0
    }

    /// Whether `sid` is a private place only the owner writes in.
    fn owner_alone_in(&self, sid: &str) -> bool {
        let class = self.runner.class_of(sid);
        let place = self.outbox.try_target(sid).ok().flatten();
        crate::capture::owner_alone(class, place.as_deref())
    }

    /// `[card] profile`'s target and provider. (`run.rs` has the same two
    /// steps privately; the join may fold them.)
    fn observe_model(
        &self,
    ) -> Result<(
        crate::turn::Target,
        Arc<dyn crate::provider::Provider>,
        String,
    )> {
        let profile = self.cfg.card.profile.clone();
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&profile), None, None)
            .map_err(|e| anyhow!("[card] profile {profile:?} does not resolve: {e:#}"))?;
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| anyhow!("no provider {:?} for {profile}", target.provider))?;
        Ok((target, provider, profile))
    }

    /// The live fact and preference records, grouped by scope, the newest
    /// [`extract::MAX_RECORDS`] of each.
    fn observe_records(&self, now: u64) -> Result<Vec<Shown>> {
        let all = self.runner.standing.all(&self.store)?;
        let mut live = standing::live(&all, None, now);
        live.retain(|r| matches!(r.kind, StandingKind::Fact | StandingKind::Preference));
        live.sort_by_key(|r| (r.scope.words(), std::cmp::Reverse(r.since_ms), r.id.clone()));
        let mut out: Vec<Shown> = Vec::new();
        let mut in_scope = 0;
        for r in live {
            let scope = r.scope.words();
            if out.last().is_none_or(|l| l.scope != scope) {
                in_scope = 0;
            }
            in_scope += 1;
            if in_scope > extract::MAX_RECORDS {
                continue;
            }
            out.push(Shown {
                id: r.id.clone(),
                kind: r.kind.name(),
                scope,
                text: r.text.clone(),
            });
        }
        Ok(out)
    }

    /// One session's pass: read its owner lines past the mark, call, keep,
    /// and write the one frame. `purpose` is `live` (due points only) or
    /// `sweep` (any owner text past the mark, within [`SWEEP_WINDOW_MS`]).
    pub(crate) async fn observe_session(&self, sid: &str, purpose: &str) -> Result<Seen> {
        if !self.observing() || !self.owner_alone_in(sid) {
            return Ok(Seen::Idle);
        }
        let now = theseus_protocol::now_unix_ms();
        let live = purpose == "live";
        let key = format!("{CARD_OBSERVE_MARK_PREFIX}{sid}");
        let floor = if live {
            0
        } else {
            now.saturating_sub(SWEEP_WINDOW_MS)
        };
        let read = blocking(|| self.observe_read(sid, &key, live, floor))?;
        let Some(Read { lines, through }) = read else {
            return Ok(Seen::Idle);
        };
        let moved = Mark {
            judgment: String::new(),
            through: through.0,
            through_ms: through.1,
            at_ms: now,
        };
        if lines.is_empty() {
            if !live {
                return Ok(Seen::Idle);
            }
            let mut passed = self.runner.judge.observing.passed.lock().unwrap();
            if passed.len() >= PASSED_KEPT {
                passed.clear();
            }
            passed.insert(sid.to_string(), moved);
            return Ok(Seen::Passed);
        }
        let mark = NewRecord::json(kinds::META, Some(&key), &moved)?;
        self.observe_call(sid, purpose, &lines, mark, now).await
    }

    /// The call and what is kept of it: ask, parse, filter, and write the
    /// one frame.
    async fn observe_call(
        &self,
        sid: &str,
        purpose: &str,
        lines: &[OwnerLine],
        mark: NewRecord,
        now: u64,
    ) -> Result<Seen> {
        let (records, held_texts, earlier) = blocking(|| self.observe_context(sid, now))?;
        let (r, cost) = match self.observe_ask(sid, lines, &records, now).await? {
            Asked::Answer(r, cost) => (r, cost),
            Asked::Not(seen) => return Ok(seen),
        };
        let today = spend::local_day(now);
        // A cut answer's tool input is partial, and one with no call holds
        // nothing: either is an answer that found nothing.
        let parsed = match crate::provider::hit_cap(r.stop_reason.as_deref()) {
            true => None,
            false => extract::parse(&r.content, lines, &records),
        }
        .unwrap_or_default();
        let returned = parsed.returned;
        let kept = extract::filter(
            parsed,
            &records,
            &Held {
                asked: &held_texts,
                earlier: &earlier,
            },
        );
        let observations: Vec<CardObservation> = kept
            .keep
            .iter()
            .map(|it| CardObservation {
                id: crate::new_id("cob"),
                session: sid.to_string(),
                nodes: it.nodes.clone(),
                kind: it.kind,
                text: it.text.clone(),
                at_ms: now,
                contradicts: it.contradicts.map(|i| records[i].id.clone()),
                state: Default::default(),
                proposal: None,
            })
            .collect();
        let said_again: Vec<(String, String)> = kept
            .restated
            .iter()
            .map(|(i, node)| (records[*i].id.clone(), node.clone()))
            .collect();
        let report = Report {
            returned,
            kept: observations.len() as u32,
            restated: said_again.len() as u32,
            cost_usd: micros_to_usd(cost),
        };
        let f = CardObserved {
            session: sid,
            purpose,
            model: &r.model,
            cost_usd: report.cost_usd,
            input_tokens: r.usage.input_tokens,
            output_tokens: r.usage.output_tokens,
            returned,
            kept: report.kept,
            restated: report.restated,
        };
        let mut frame = Vec::new();
        for o in &observations {
            frame.push(state::obs_record(o)?);
        }
        frame.push(mark);
        frame.push(crate::fact::row(&f, Some(sid), None)?);
        let spend = Some((today.as_str(), report.cost_usd));
        self.observe_frame(sid, frame, &said_again, spend, now)
            .await?;
        self.rec(Some(sid)).announce(&f);
        Ok(Seen::Done(report))
    }

    /// What the call is told besides the lines: the live records, the lines
    /// capture holds, and this session's earlier observations.
    fn observe_context(
        &self,
        sid: &str,
        now: u64,
    ) -> Result<(Vec<Shown>, Vec<String>, Vec<String>)> {
        let records = self.observe_records(now)?;
        let cap = self.runner.standing.capture.peek(&self.store)?;
        let mut asked: Vec<String> = cap.asks.iter().map(|o| o.ask.text.clone()).collect();
        asked.extend(cap.candidates.iter().map(|c| c.line.clone()));
        let earlier: Vec<String> = state::observations(&self.store, None)?
            .into_iter()
            .filter(|o| o.session == sid)
            .map(|o| o.text)
            .collect();
        Ok((records, asked, earlier))
    }

    /// The model call: scrubbed, through the session's marks, reserved on the
    /// card's day line and the day ceiling first.
    async fn observe_ask(
        &self,
        sid: &str,
        lines: &[OwnerLine],
        records: &[Shown],
        now: u64,
    ) -> Result<Asked> {
        let (target, provider, _) = self.observe_model()?;
        let provider = crate::data_gate::bound(provider, sid);
        let scrubber = self.tools.scrubber.clone();
        let how = self.runner.catalog.forcing(&target.model);
        let request =
            extract::request(&target.model, how, lines, records, &|t| scrubber.scrub(t).0);
        let Some(need) = self.audit_reserve(&request) else {
            return Ok(Asked::Not(Seen::Stopped(format!(
                "no price for {}",
                target.model
            ))));
        };
        let st = blocking(|| State::read(&self.store))?;
        let left = (self.cfg.card.day_usd - st.spent_on(&spend::local_day(now))).max(0.0);
        if micros_to_usd(need) > left {
            return Ok(Asked::Not(Seen::Stopped(format!(
                "budget: ${left:.2} of [card] day_usd is left, under a call's worst case"
            ))));
        }
        let hold = match self.runner.day_hold(need, "card") {
            Ok(h) => h,
            Err(why) => return Ok(Asked::Not(Seen::Stopped(format!("budget: {why}")))),
        };
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = provider.stream_message(&request, &mut quiet).await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &target.model, need));
        hold.settle(cost);
        Ok(match answered {
            Ok(r) => Asked::Answer(Box::new(r), cost),
            Err(e) => Asked::Not(Seen::Failed(format!("{e:#}"))),
        })
    }

    /// The blocking half: the mark, the records after it, whether a live
    /// pass is due, and the owner's lines (oldest first, at most
    /// [`extract::MAX_LINES`], none written before `floor`) with the position
    /// and time the mark moves to. `None`: nothing to do.
    fn observe_read(&self, sid: &str, key: &str, live: bool, floor: u64) -> Result<Option<Read>> {
        let stored: Option<Mark> = self.store.get_meta(key)?;
        let kept = self
            .runner
            .judge
            .observing
            .passed
            .lock()
            .unwrap()
            .get(sid)
            .cloned();
        let mark = match (stored, kept) {
            (Some(a), Some(b)) => Some(if b.through > a.through { b } else { a }),
            (a, b) => a.or(b),
        };
        let after: Vec<(u64, Node)> = self
            .store
            .scope_after(sid, mark.as_ref().map_or(0, |m| m.through))?
            .iter()
            .filter(|r| r.kind == kinds::NODE)
            .filter_map(|r| Some((r.position, r.decode::<Node>().ok()?)))
            .collect();
        if live && due(&after, mark.as_ref()).is_none() {
            return Ok(None);
        }
        let Some(newest) = after
            .iter()
            .rev()
            .find(|(_, n)| is_human(n))
            .map(|(p, n)| (*p, n.created_at_ms))
        else {
            return Ok(None);
        };
        let expected = &self.runner.standing.expected;
        let owner: Vec<&(u64, Node)> = after
            .iter()
            .filter(|(_, n)| is_human(n) && n.created_at_ms >= floor && expected.is_owner(&n.id))
            .collect();
        // Past the cap the mark stops at the last line read.
        let through = match owner.get(extract::MAX_LINES - 1) {
            Some((p, n)) if owner.len() > extract::MAX_LINES => (*p, n.created_at_ms),
            _ => newest,
        };
        let lines: Vec<OwnerLine> = owner
            .iter()
            .take(extract::MAX_LINES)
            .filter_map(|(_, n)| match &n.body {
                Body::UserMessage { text, .. } => {
                    let t: String = text.trim().chars().take(extract::LINE_CHARS).collect();
                    (!t.is_empty()).then(|| OwnerLine {
                        node: n.id.clone(),
                        text: t,
                    })
                }
                _ => None,
            })
            .collect();
        Ok(Some(Read { lines, through }))
    }

    /// One frame, written between turns (the memory pass's writer handshake):
    /// `frame`, the day's spend on the card's state when `spend` says, and a
    /// said-again row for each record restated, with capture's count of it,
    /// whose state rides the frame and is held once the frame is written.
    async fn observe_frame(
        &self,
        sid: &str,
        mut frame: Vec<NewRecord>,
        said_again: &[(String, String)],
        spend: Option<(&str, f64)>,
        now: u64,
    ) -> Result<()> {
        let w = self.runner.pass.writing().await;
        let written = blocking(|| -> Result<()> {
            if let Some((day, usd)) = spend {
                let mut st = State::read(&self.store)?;
                st.spend(day, usd);
                frame.push(st.record()?);
            }
            for (record, node) in said_again {
                let f = crate::fact::standing::StandingRepeated { record, node };
                frame.push(crate::fact::row(&f, Some(sid), None)?);
            }
            if said_again.is_empty() {
                self.store.append(&frame)?;
                return Ok(());
            }
            let mut held = self.runner.standing.capture.lock(&self.store)?;
            let mut next = (*held).clone();
            for (record, _) in said_again {
                next.repeats.entry(record.clone()).or_default().push(now);
            }
            next.trim(now);
            frame.push(next.row()?);
            self.store.append(&frame)?;
            held.set(next);
            Ok(())
        });
        drop(w);
        written
    }

    /// The nightly sweep: every private session the owner is alone in, never
    /// a task's, with owner text past its mark, one pass each
    /// (`purpose` `sweep`). Returns how many sessions it read. It stops at
    /// the first call a budget refuses.
    async fn observe_sweep(&self, now_ms: u64) -> Result<u32> {
        if !self.observing() {
            return Ok(0);
        }
        let from = now_ms.saturating_sub(SWEEP_WINDOW_MS);
        let mut sessions: Vec<SessionRecord> = blocking(|| self.store.live_sessions())?;
        sessions.retain(|s| s.task.is_none() && s.last_active_ms >= from);
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_active_ms));
        let mut read = 0;
        for s in sessions.into_iter().take(SWEEP_SESSIONS) {
            let sid = s.session_id;
            if self.outbox.stopping() {
                break;
            }
            if !self.owner_alone_in(&sid) {
                continue;
            }
            let deciding = &self.runner.judge.observing.deciding;
            if !deciding.lock().unwrap().insert(sid.clone()) {
                continue;
            }
            let seen = self.observe_session(&sid, "sweep").await;
            deciding.lock().unwrap().remove(&sid);
            match swept(&sid, seen) {
                Some(true) => read += 1,
                Some(false) => break,
                None => {}
            }
        }
        Ok(read)
    }
}

/// The nightly sweep, for the card's live pass: how many sessions it read.
/// The live pass calls it first (`Core::sweep_observations`).
pub async fn sweep(core: &Core, now_ms: u64) -> Result<u32> {
    core.observe_sweep(now_ms).await
}

/// What a sweep does with a session's pass: `Some(true)` it was read,
/// `Some(false)` stop the sweep (a budget refused), `None` go on.
fn swept(sid: &str, seen: Result<Seen>) -> Option<bool> {
    match seen {
        Ok(Seen::Done(_)) => Some(true),
        Ok(Seen::Stopped(why)) => {
            tracing::info!(%why, "card: the sweep stopped");
            Some(false)
        }
        Ok(Seen::Failed(why)) => {
            tracing::info!(session = %sid, %why, "card: a session was not observed");
            None
        }
        Ok(Seen::Idle | Seen::Passed) => None,
        Err(e) => {
            tracing::warn!(session = %sid, error = %format!("{e:#}"), "card: a session was not observed");
            None
        }
    }
}
