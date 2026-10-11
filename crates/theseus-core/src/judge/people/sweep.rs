//! The nightly sweep (theseus-j8qb; the owner's design for live people,
//! theseus-u5n8, step 3): what the live point's Jev gate (`people_seen.v1`)
//! let pass. At quiet time once a day, `[judge] learning_hour` local (a
//! missed day once, as soon as it may, never within 10 minutes of a start:
//! the learning tender's `due` and `tend`), one pass (`run.rs`: the
//! `[people] extract_profile` extraction, then `people.v1` on each kept
//! candidate) for each private session, never a task's, that got human text
//! since the last sweep (the last day, the first time) that no extraction
//! read: the gate shut there, or no due point came. Proposals only, never
//! memberships; the place rule's private places only; the extraction's
//! `people.extracted` row has the purpose `sweep`.
//!
//! - **What it reads.** The session's human-facing lines (`lines`) written
//!   in the window after its newest extraction (any purpose). The live
//!   mark (META `judge.people.<session>`) moves in the pass's frame to the
//!   newest human message read, never back, so the live point does not ask
//!   the gate about the same text again.
//! - **Under a daily cap**, `[people] sweep_usd_per_day` (2.0; 0 turns it
//!   off, as `[people] live = false` does): a session whose worst case (the
//!   extractor's at `extract::MAX_TOKENS`, and Jev's projection) would pass
//!   what is left of the local day's cap is not begun, as the backfill's
//!   cap does (`backfill.rs`); the day's ceiling holds each call too. Paced
//!   by the machine's quiet between sessions, and it stops when the daemon
//!   does. A session the live point is deciding is left to it.
//! - **Its row**, `people.swept` (due, swept, found, judged, spent, the cap,
//!   why it stopped), in one frame with its META mark (`people.sweep.last`:
//!   when, the local day, and that day's spend).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use theseus_judge::price::Micros;
use theseus_kernel::{micros_to_usd, usd_to_micros};
use theseus_store::{kinds, NewRecord};
use tokio::time::Instant;

use super::backfill::jev_projected;
use super::live::MARK_PREFIX;
use super::run::{meta, Session};
use super::{fold, lines, Line, PACK, SCOPE};
use crate::judge::categorize::{is_human, Mark};
use crate::judge::spend;
use crate::learning::tender::{due, tend};
use crate::ledger::LedgerRow;
use crate::node::Node;
use crate::places::PlaceClass;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The last run's META key.
pub const LAST_RUN: &str = "people.sweep.last";
/// The window of a first run: a day.
pub const FIRST_WINDOW_MS: u64 = 24 * 3_600_000;

/// The last run: when, its local day, and what that day's runs spent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SweepMark {
    pub at_ms: u64,
    pub day: String,
    pub spent_usd: f64,
}

/// One session due.
struct Due {
    sid: String,
    title: String,
    lines: Vec<Line>,
    /// The live mark, moved to the newest human message read.
    mark: Option<NewRecord>,
    known: HashSet<String>,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Swept {
    pub due: u64,
    pub swept: u64,
    pub candidates: u64,
    pub judged: u64,
    pub spent: Micros,
    pub stopped: Option<String>,
}

impl Core {
    /// The sweep's tender, after serving: nothing when the cap is 0 or
    /// `[people] live` is off.
    pub fn people_sweep_after_serving(self: &Arc<Self>) {
        let cfg = &self.runner.cfg.people;
        if cfg.sweep_usd_per_day <= 0.0 || !cfg.live {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            // A run that wrote no mark (a failed one) is not due again at once.
            let ran = Arc::new(AtomicU64::new(0));
            let (read, ran_at) = (core.clone(), ran.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let stored = theseus_store::blocking(|| c.store.get_meta::<SweepMark>(LAST_RUN))
                    .ok()
                    .flatten()
                    .map(|m| m.at_ms);
                let here = ran_at.load(Ordering::Relaxed);
                let last = stored.max((here > 0).then_some(here));
                let now = theseus_protocol::now_unix_ms();
                let (at, trigger) = due(now, c.runner.cfg.judge.learning_hour, last);
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |trigger: &'static str| {
                let core = core.clone();
                ran.store(theseus_protocol::now_unix_ms(), Ordering::Relaxed);
                async move {
                    let Some(c) = core.upgrade() else { return };
                    match c.people_sweep(trigger).await {
                        Ok(s) => tracing::info!(
                            due = s.due,
                            swept = s.swept,
                            spent_usd = micros_to_usd(s.spent),
                            "people: the sweep ran"
                        ),
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "people: the sweep did not run");
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }

    /// One run of the sweep, `trigger` saying why (`nightly`, `missed`).
    pub async fn people_sweep(&self, trigger: &str) -> anyhow::Result<Swept> {
        let now = theseus_protocol::now_unix_ms();
        let today = spend::local_day(now);
        let last: Option<SweepMark> = theseus_store::blocking(|| self.store.get_meta(LAST_RUN))?;
        let from_ms = last
            .as_ref()
            .map_or(now.saturating_sub(FIRST_WINDOW_MS), |m| m.at_ms);
        let before = last.filter(|m| m.day == today).map_or(0.0, |m| m.spent_usd);
        let cap_usd = self.runner.cfg.people.sweep_usd_per_day;
        let cap = usd_to_micros((cap_usd - before).max(0.0));
        let mut out = Swept::default();
        match self.runner.judge.pack_on(PACK) {
            true => self.sweep_sessions(from_ms, now, cap, &mut out).await?,
            false => out.stopped = Some(format!("{PACK} is off")),
        }
        let spent_usd = micros_to_usd(out.spent);
        let f = crate::fact::people::PeopleSwept {
            trigger,
            from_ms,
            due: out.due,
            swept: out.swept,
            candidates: out.candidates,
            judged: out.judged,
            spent_usd,
            cap_usd,
            spent_before_usd: before,
            stopped: out.stopped.as_deref(),
        };
        let mark = SweepMark {
            at_ms: now,
            day: today,
            spent_usd: before + spent_usd,
        };
        self.people_write(&[crate::fact::row(&f, None, None)?, meta(LAST_RUN, &mark)?])
            .await?;
        self.rec(None).announce(&f);
        Ok(out)
    }

    /// The due sessions, each passed under the cap.
    async fn sweep_sessions(
        &self,
        from_ms: u64,
        now: u64,
        cap: Micros,
        out: &mut Swept,
    ) -> anyhow::Result<()> {
        let plan = theseus_store::blocking(|| self.sweep_plan(from_ms, now))?;
        out.due = plan.len() as u64;
        let (target, _) = match self.people_extractor() {
            Ok(t) => t,
            Err(why) => {
                out.stopped = Some(why);
                return Ok(());
            }
        };
        let Some(price) = self.runner.catalog.get(&target.model).cloned() else {
            out.stopped = Some(format!("no price for {}", target.model));
            return Ok(());
        };
        for (i, d) in plan.into_iter().enumerate() {
            let left = out.due - i as u64;
            if i > 0 {
                theseus_store::pressure::quiet(theseus_store::pressure::BOUND).await;
            }
            if self.outbox.stopping() {
                out.stopped = Some(format!("the daemon is stopping, {left} sessions left"));
                break;
            }
            let request = self.people_request(&target.model, &d.title, &d.lines);
            let est = crate::compiler::estimate(&request, price.bytes_per_token, None).tokens;
            let worst = price.reserve_micros(super::extract::MAX_TOKENS, est) + jev_projected();
            if out.spent.saturating_add(worst) > cap {
                out.stopped = Some(format!(
                    "at the day's cap: the next session's worst case ({}) would pass what is left \
                     ({}), {left} sessions left for tomorrow's",
                    crate::narrative::dollars(worst),
                    crate::narrative::dollars(cap.saturating_sub(out.spent)),
                ));
                break;
            }
            let deciding = &self.runner.judge.people.deciding;
            if !deciding.lock().unwrap().insert(d.sid.clone()) {
                continue;
            }
            let pass = self
                .propose_people(Session {
                    sid: &d.sid,
                    title: &d.title,
                    lines: d.lines,
                    purpose: "sweep",
                    mark: d.mark,
                    known: d.known,
                })
                .await;
            deciding.lock().unwrap().remove(&d.sid);
            match pass {
                Ok(p) => {
                    out.swept += 1;
                    out.spent += p.spent;
                    out.candidates += p.candidates;
                    out.judged += p.judged;
                }
                Err(why) => {
                    out.stopped = Some(format!("{why}, {left} sessions left"));
                    break;
                }
            }
        }
        Ok(())
    }

    /// The private sessions with human text since `from_ms` that no
    /// extraction read, the newest first, each with its lines, its moved
    /// mark and the names it was asked about: one scan of `judge:people`.
    fn sweep_plan(&self, from_ms: u64, now: u64) -> anyhow::Result<Vec<Due>> {
        let mut extracted: HashMap<String, u64> = HashMap::new();
        let mut known: HashMap<String, HashSet<String>> = HashMap::new();
        for r in self.store.scope_after(SCOPE, 0)? {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            let Some(sid) = row.session_id.clone() else {
                continue;
            };
            match row.kind.as_str() {
                "people.extracted" => {
                    let at = extracted.entry(sid).or_default();
                    *at = (*at).max(r.position);
                }
                "judge.call" => {
                    if let Some(name) = row.data["context"]["candidate"]["name"].as_str() {
                        known.entry(sid).or_default().insert(fold(name));
                    }
                }
                _ => {}
            }
        }
        let mut sessions: Vec<SessionRecord> = self.store.live_sessions()?;
        sessions.retain(|s| {
            s.task.is_none()
                && s.last_active_ms >= from_ms
                && self.runner.class_of(&s.session_id) == PlaceClass::Private
        });
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_active_ms));
        let mut plan = Vec::new();
        for s in sessions {
            let sid = s.session_id;
            let after = extracted.get(&sid).copied().unwrap_or(0);
            let fresh: Vec<(u64, Node)> = self
                .store
                .scope_after(&sid, after)?
                .iter()
                .filter(|r| r.kind == kinds::NODE)
                .filter_map(|r| Some((r.position, r.decode::<Node>().ok()?)))
                .filter(|(_, n)| n.created_at_ms >= from_ms)
                .collect();
            let Some((through, through_ms)) = fresh
                .iter()
                .rev()
                .find(|(_, n)| is_human(n))
                .map(|(p, n)| (*p, n.created_at_ms))
            else {
                continue;
            };
            let found = lines(&fresh);
            if found.is_empty() {
                continue;
            }
            let key = format!("{MARK_PREFIX}{sid}");
            let held: Option<Mark> = self.store.get_meta(&key)?;
            let mark = match held.is_none_or(|m| m.through < through) {
                true => Some(meta(
                    &key,
                    &Mark {
                        judgment: String::new(),
                        through,
                        through_ms,
                        at_ms: now,
                    },
                )?),
                false => None,
            };
            plan.push(Due {
                known: known.remove(&sid).unwrap_or_default(),
                title: s.title.unwrap_or_default(),
                sid,
                lines: found,
                mark,
            });
        }
        Ok(plan)
    }
}
