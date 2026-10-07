//! `categorize.v1` in shadow (M5 step 28b; design §2.4, §2.12): at a
//! conversation's exchange end, which declared topic its recent human
//! messages belong to. Jev writes no membership: an answer that names a
//! topic the session is not in, or `new_topic`, is a proposal the operator
//! accepts or rejects (`rpc/proposals.rs`), and that answer is the
//! judgment's label.
//!
//! - **Where.** A turn of a conversation the baseline ended with no tool
//!   calls, in a private place: a shared place's compile walk reads no
//!   interpreted membership (the place rule), and a task's one human
//!   message is its brief. Nothing is decided on the turn's path: the
//!   turn's end spawns the decision, as `loop.v1`'s.
//! - **When** ([`due`]): 10 human messages have arrived since the
//!   session's last `categorize.v1` judgment, or this exchange began after
//!   30 minutes' quiet and a human message arrived since that judgment. The
//!   last judgment is the session's mark (a META record under
//!   [`MARK_PREFIX`]), so the decision reads only the session's records
//!   after it. The mark moves in memory as the judgment is dispatched, so
//!   the next exchange end reads it at once, and is written in the sink's
//!   frame beside the judgment's row (theseus-xkbs): never in a frame of its
//!   own, which landed inside the next turn, and a crash loses the two
//!   together, where a mark used to outlive its lost row.
//! - **What** ([`input`]): the session's title and its last ten human
//!   messages; up to 50 candidate topics with their descriptions, from the
//!   ontology's snapshot; and up to 5 of the session's interpreted
//!   memberships, for the `still_member` Nouls. With more than 50 topics,
//!   the session's own come first, then those with a description, then the
//!   rest, each part by name; the judgment's context counts what was left
//!   out. With no topic declared the point still judges (theseus-ext.12):
//!   the Choice is `new_topic` and `none`, so a session can propose the
//!   ontology's first topic, and the mark moves as for any judgment.
//! - **What it reads** (theseus-gky0). The records after the mark. With
//!   fewer than ten human messages since it, the input reaches back to the
//!   session's start for the last ten, but only when a topic is declared:
//!   with none, it takes the human messages since the mark alone, so a
//!   session with no topic never rereads its history.
//! - **No mark in a trace.** The decision runs after the turn's last frame,
//!   outside every turn, so it marks no trace (the convention's "a dispatch
//!   outside a turn needs no mark"); the id is still minted here, and the
//!   mark names it.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_judge::builders::{CategorizeInput, MembershipInput, TopicInput};
use theseus_judge::{Ask, DecisionPoint, Input, Judge, Outcome, Pack, Urgency};
use theseus_ontology::{Category, Ontology};
use theseus_store::kinds;

use super::{spend, JudgeService, Prepared, ScrubWith};
use crate::node::{Body, Node, Origin};
use crate::places::PlaceClass;
use crate::rpc::Core;

/// The pack (design §2.4's `categorize.v1`).
pub const PACK: &str = "categorize.v1";
/// Human messages since the last judgment that bring the next.
pub const EVERY: usize = 10;
/// The quiet before an exchange that brings a judgment, when anything new
/// arrived since the last.
pub const QUIET_MS: u64 = 30 * 60_000;
/// The topics a judgment offers at most.
pub const CANDIDATES: usize = 50;
/// The memberships a judgment asks about at most (`still_member`).
pub const MEMBERSHIPS: usize = 5;
/// The human messages the state carries.
pub const RECENT: usize = 10;
/// A session's mark: its last judgment, at META `judge.categorize.<session>`.
pub const MARK_PREFIX: &str = "judge.categorize.";
/// Option ids the pack keeps for itself: a topic named so is not offered.
const RESERVED: [&str; 2] = ["new_topic", "none"];

/// A session's last `categorize.v1` judgment, and the newest human message
/// it read: the decision reads the session's records after `through`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub judgment: String,
    /// The WAL position of the newest human message it read.
    pub through: u64,
    /// When that message was written.
    pub through_ms: u64,
    pub at_ms: u64,
}

/// Why a judgment is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// [`EVERY`] human messages since the last.
    Count,
    /// The exchange began after [`QUIET_MS`] of quiet.
    Quiet,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Count => "count",
            Trigger::Quiet => "quiet",
        }
    }
}

/// What the point keeps: the core (set once it is built, held by `Weak`),
/// and the sessions whose decision is running, so two exchange ends close
/// together make one judgment.
#[derive(Default)]
pub struct Point {
    core: OnceLock<Weak<Core>>,
    deciding: Mutex<HashSet<String>>,
    /// The session records the decisions have read, all told: what the
    /// tests count to hold a decision to the records after its mark.
    read: AtomicU64,
    /// The marks moved and not yet written, by judgment: the sink writes
    /// each beside its judgment's row (theseus-xkbs).
    unwritten: Mutex<std::collections::HashMap<String, (String, Mark)>>,
}

/// A human message: one an operator wrote. A task's brief and report, a
/// wake's note, and the harness's notices are not.
pub fn is_human(n: &Node) -> bool {
    n.origin == Origin::Operator && matches!(n.body, Body::UserMessage { .. })
}

/// Whether a judgment is due, from the session's nodes after its mark
/// (every node, when it has none), oldest first.
///
/// "The first exchange end after 30 minutes' quiet" is read as: the run of
/// human messages that began the latest exchange (its first message, and
/// any that followed it with nothing between) came 30 minutes or more after
/// the node before it, or after the mark's message when the run is the
/// first thing after it. A session's first message has nothing before it,
/// so it is never quiet. Once judged, the mark moves past the run, so the
/// same quiet brings one judgment.
pub fn due(after: &[(u64, Node)], mark: Option<&Mark>) -> Option<Trigger> {
    let human = after.iter().filter(|(_, n)| is_human(n)).count();
    if human >= EVERY {
        return Some(Trigger::Count);
    }
    let last = after.iter().rposition(|(_, n)| is_human(n))?;
    let mut first = last;
    while first > 0 && is_human(&after[first - 1].1) {
        first -= 1;
    }
    let before = match first {
        0 => mark.map(|m| m.through_ms)?,
        i => after[i - 1].1.created_at_ms,
    };
    (after[first].1.created_at_ms.saturating_sub(before) >= QUIET_MS).then_some(Trigger::Quiet)
}

/// The candidate topics and the session's memberships, from the snapshot:
/// the categories of each interpreted kind the operator assigns (the kinds
/// table's `assigned_by`), at most [`CANDIDATES`]; and the session's
/// interpreted memberships, newest first, at most [`MEMBERSHIPS`]. With
/// more topics than fit, the session's own come first, then those with a
/// description, then the rest, each by name. Returns how many were left out.
pub fn candidates(o: &Ontology, session: &str) -> (Vec<TopicInput>, Vec<MembershipInput>, usize) {
    let mut members = o.memberships(session);
    members.sort_by_key(|m| std::cmp::Reverse(m.as_of_ms));
    let assignable = |kind: &str| {
        o.kind(kind).is_some_and(|k| {
            !k.is_given() && k.assigned_by.contains(&theseus_ontology::Origin::Operator)
        })
    };
    let mut topics: Vec<&Category> = o
        .categories()
        .filter(|c| assignable(c.kind()) && !RESERVED.contains(&c.id.local()))
        .collect();
    let own = |c: &Category| members.iter().any(|m| m.category == c.id);
    topics.sort_by(|a, b| {
        (
            !own(a),
            a.description.trim().is_empty(),
            &a.name,
            a.id.as_str(),
        )
            .cmp(&(
                !own(b),
                b.description.trim().is_empty(),
                &b.name,
                b.id.as_str(),
            ))
    });
    let left_out = topics.len().saturating_sub(CANDIDATES);
    let candidates = topics
        .into_iter()
        .take(CANDIDATES)
        .map(|c| TopicInput {
            id: c.id.local().to_string(),
            description: match c.description.trim() {
                "" => c.name.clone(),
                d => format!("{}: {d}", c.name),
            },
        })
        .collect();
    let memberships = members
        .iter()
        .filter(|m| assignable(m.kind()))
        .take(MEMBERSHIPS)
        .map(|m| MembershipInput {
            id: m.category.local().to_string(),
            title: o
                .category(&m.category)
                .map_or_else(|| m.category.to_string(), |c| c.name.clone()),
        })
        .collect();
    (candidates, memberships, left_out)
}

/// The builder's input: the title, the last [`RECENT`] human messages of
/// `nodes` (oldest first), and the snapshot's candidates and memberships.
pub fn input(
    title: &str,
    nodes: &[(u64, Node)],
    o: &Ontology,
    session: &str,
) -> (CategorizeInput, usize) {
    let mut recent: Vec<String> = nodes
        .iter()
        .rev()
        .filter(|(_, n)| is_human(n))
        .take(RECENT)
        .filter_map(|(_, n)| match &n.body {
            Body::UserMessage { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    recent.reverse();
    let (candidates, memberships, left_out) = candidates(o, session);
    (
        CategorizeInput {
            session_title: title.to_string(),
            recent_human_messages: recent,
            memberships,
            candidates,
        },
        left_out,
    )
}

/// What the turn's end hands the point.
#[derive(Debug, Clone)]
pub struct ExchangeEnd {
    pub session_id: String,
    pub execution_id: String,
    pub turn_id: String,
}

impl JudgeService {
    /// The core the point reads (its places, its ontology), held by `Weak`:
    /// set once, as the core is built.
    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.categorize.core.set(Arc::downgrade(core));
    }

    /// The core it was attached to, while it lives (the notices' outbox).
    pub(crate) fn core(&self) -> Option<Arc<Core>> {
        self.categorize.core.get()?.upgrade()
    }

    /// The session records `categorize.v1`'s decisions have read since the
    /// start.
    pub fn categorize_records_read(&self) -> u64 {
        self.categorize.read.load(Ordering::Relaxed)
    }

    /// A conversation's turn that the baseline ended with no tool calls:
    /// whether `categorize.v1` judges it is decided in a task of its own.
    /// Returns at once, whatever Jev does.
    pub fn at_exchange_end(&self, end: ExchangeEnd, task: bool) {
        // The version standing in categorize.v1's place (25f).
        let name = self.placed(PACK, &end.session_id);
        if task || !self.pack_on(&name) {
            return;
        }
        let Some(pack) = self.pack(&name) else {
            return;
        };
        if !super::sampled(&end.turn_id, self.cfg.sample_of(PACK, pack.sample)) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if !self
            .categorize
            .deciding
            .lock()
            .unwrap()
            .insert(end.session_id.clone())
        {
            return;
        }
        rt.spawn(judge_categorize(self.me.clone(), pack, end));
    }

    /// The blocking half: the place's class, the mark, the records after
    /// it, the trigger; then the state and its blob, and the mark to move
    /// once the reservation is made. `None`: nothing to send.
    fn prepare_categorize(&self, pack: Arc<Pack>, end: &ExchangeEnd) -> Option<(Prepared, Mark)> {
        let core = self.categorize.core.get()?.upgrade()?;
        let sid = end.session_id.as_str();
        if core.runner.class_of(sid) != PlaceClass::Private {
            return None;
        }
        let mark = self.mark(sid);
        let (after, read) = nodes_after(&self.store, sid, mark.as_ref().map_or(0, |m| m.through))?;
        self.categorize.read.fetch_add(read, Ordering::Relaxed);
        let trigger = due(&after, mark.as_ref())?;
        let o = match core.runner.ontology.held() {
            Some(o) => o,
            None => core.runner.ontology.snapshot(&self.store).ok()?,
        };
        let human_after = after.iter().filter(|(_, n)| is_human(n)).count();
        let declared = !candidates(&o, sid).0.is_empty();
        let nodes = match human_after >= RECENT || !declared {
            true => after,
            false => {
                let all = self.store.session_nodes(sid).ok()?;
                self.categorize
                    .read
                    .fetch_add(all.len() as u64, Ordering::Relaxed);
                all
            }
        };
        let (newest, newest_ms) = nodes
            .iter()
            .rev()
            .find(|(_, n)| is_human(n))
            .map(|(p, n)| (*p, n.created_at_ms))?;
        let title = self
            .store
            .get_session::<crate::session::SessionRecord>(sid)
            .ok()
            .flatten()
            .and_then(|s| s.title)
            .unwrap_or_default();
        let (input, left_out) = input(&title, &nodes, &o, sid);
        let candidates = input.candidates.len();
        let scrub = ScrubWith(self.scrubber.clone());
        let state = theseus_judge::prepare(&pack, &Input::Categorize(input), &scrub).ok()?;
        let blob = self
            .store
            .blobs()
            .put(state.state.json.as_bytes())
            .map_err(|e| tracing::warn!(error = %e, "judge: the state's blob was not written; not judged"))
            .ok()?;
        let built = self
            .built()
            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the Jev client was not built"))
            .ok()?;
        let id = format!("jdg_{}", uuid::Uuid::now_v7().simple());
        let mut context = json!({
            "session": sid, "execution": end.execution_id, "turn": end.turn_id,
            "baseline": "no_membership", "decision": "no_membership", "trigger": trigger.as_str(),
            "class": "reply", "blob": blob, "on_path_ms": 0, "through": newest,
            "candidates": candidates, "candidates_left_out": left_out,
        });
        let mode = self.ask_mode(&pack.name(), &mut context);
        let mut ask = Ask::new(pack, &state, mode, context);
        ask.id = Some(id.clone());
        let need = built
            .judge
            .inner()
            .reserve_micros(std::slice::from_ref(&ask))
            .unwrap_or(0);
        let moved = Mark {
            judgment: id,
            through: newest,
            through_ms: newest_ms,
            at_ms: theseus_protocol::now_unix_ms(),
        };
        Some((Prepared { built, ask, need }, moved))
    }

    /// A session's mark: the newest moved and not yet written, else the
    /// store's. A mark only moves forward, so the newer is the one further
    /// through the session.
    fn mark(&self, sid: &str) -> Option<Mark> {
        let stored: Option<Mark> = self
            .store
            .get_meta(&format!("{MARK_PREFIX}{sid}"))
            .ok()
            .flatten();
        let moved = self
            .categorize
            .unwritten
            .lock()
            .unwrap()
            .values()
            .filter(|(s, _)| s == sid)
            .map(|(_, m)| m.clone())
            .max_by_key(|m| m.through);
        match (stored, moved) {
            (Some(s), Some(m)) if s.through >= m.through => Some(s),
            (s, m) => m.or(s),
        }
    }

    /// The marks the judgments of `batch` moved, keyed for their META
    /// records, for the sink's frame (theseus-xkbs); one the store already
    /// holds a newer mark than (a later judgment's row written first) is
    /// left out, so the mark never moves back.
    pub(crate) fn unwritten_marks(&self, batch: &[theseus_judge::Judgment]) -> Vec<(String, Mark)> {
        let found: Vec<(String, Mark)> = {
            let u = self.categorize.unwritten.lock().unwrap();
            if u.is_empty() {
                return Vec::new();
            }
            batch.iter().filter_map(|j| u.get(&j.id).cloned()).collect()
        };
        newest_each(found)
            .into_iter()
            .filter_map(|(sid, m)| {
                let key = format!("{MARK_PREFIX}{sid}");
                let stored: Option<Mark> = self.store.get_meta(&key).ok().flatten();
                match stored {
                    Some(s) if s.through >= m.through => None,
                    _ => Some((key, m)),
                }
            })
            .collect()
    }

    /// The sink's frame with `batch`'s rows was written (or lost with
    /// them): their marks are the store's from now on.
    pub(crate) fn marks_written(&self, batch: &[theseus_judge::Judgment]) {
        let mut u = self.categorize.unwritten.lock().unwrap();
        if u.is_empty() {
            return;
        }
        for j in batch {
            u.remove(&j.id);
        }
    }
}

/// Each session's newest mark of `marks` (by `through`), in the order the
/// sessions first appear: two judgments of one session can settle in one
/// frame, newer first, and the frame's later record of a key is the one the
/// store keeps, so a frame carries one mark a session.
pub(crate) fn newest_each(marks: Vec<(String, Mark)>) -> Vec<(String, Mark)> {
    let mut out: Vec<(String, Mark)> = Vec::new();
    for (sid, m) in marks {
        match out.iter_mut().find(|(s, _)| *s == sid) {
            Some((_, kept)) if kept.through < m.through => *kept = m,
            Some(_) => {}
            None => out.push((sid, m)),
        }
    }
    out
}

/// The session's nodes after `position`, oldest first, and how many
/// records were read for them.
fn nodes_after(
    store: &crate::store::Store,
    session: &str,
    position: u64,
) -> Option<(Vec<(u64, Node)>, u64)> {
    let records = store
        .scope_after(session, position)
        .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "judge: the session's records were not read"))
        .ok()?;
    let nodes = records
        .iter()
        .filter(|r| r.kind == kinds::NODE)
        .filter_map(|r| Some((r.position, r.decode::<Node>().ok()?)))
        .collect();
    Some((nodes, records.len() as u64))
}

/// One `categorize.v1` judgment, in its own task. The service is held only
/// around the blocking half, never across the call.
async fn judge_categorize(me: Weak<JudgeService>, pack: Arc<Pack>, end: ExchangeEnd) {
    let today = spend::local_day(theseus_protocol::now_unix_ms());
    let Some(svc) = me.upgrade() else { return };
    let sid = end.session_id.clone();
    let prepared = tokio::task::spawn_blocking(move || svc.prepare_categorize(pack, &end))
        .await
        .ok()
        .flatten();
    // The reservation, between turns when it writes a frame; then the mark
    // moved, in memory, before the decision ends, so the next exchange end
    // reads it (the sink writes it beside the row).
    let granted = match &prepared {
        Some((p, _)) => super::reserve_between(&me, &today, p.need).await,
        None => false,
    };
    let Some(svc) = me.upgrade() else { return };
    if let (true, Some((_, moved))) = (granted, &prepared) {
        svc.categorize
            .unwritten
            .lock()
            .unwrap()
            .insert(moved.judgment.clone(), (sid.clone(), moved.clone()));
    }
    svc.categorize.deciding.lock().unwrap().remove(&sid);
    drop(svc);
    let Some((Prepared { built, ask, need }, _)) = prepared.filter(|_| granted) else {
        return;
    };
    let judgments = built
        .judge
        .judge(DecisionPoint {
            asks: vec![ask],
            urgency: Urgency::Shadow,
        })
        .await;
    tracing::debug!(session = %sid, "judge: categorize.v1 judged");
    let Some(svc) = me.upgrade() else { return };
    for j in &judgments {
        let (called, failed, unknown) = match &j.outcome {
            Outcome::Answered => (true, false, false),
            Outcome::Failed { usage_unknown, .. } => (true, true, *usage_unknown),
            Outcome::Skipped { .. } => (false, false, false),
        };
        let spent = j.cost_micros.unwrap_or(if unknown { need } else { 0 });
        svc.budget.settle(&today, need, spent, called, failed);
    }
}
