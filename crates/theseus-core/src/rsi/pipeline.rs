//! The self pipeline (theseus-pw1q.16; rsi-plan §3, wave 3): the scheduler
//! between the backlog, the branch builder and the self-join. From the
//! ranked backlog it keeps up to `[self] pipeline_builds` builds running,
//! sends each finished branch to self-join's review, and hands the joins to
//! self-join one at a time, inside the self budget and the capacity guard.
//!
//! **One owner of three queues**, all derived from the record each tick
//! ([`queues`]), never kept only in memory:
//!
//! ```text
//!            the night files and ranks           drop (the owner)
//!                     │                         ┌──────────────► dropped: its build stopped,
//!                     ▼                         │                its trees deleted (cleared)
//!   ┌──────────► READY ── a slot and room ──► BUILDING ── its turn ends with no green branch
//!   │            (proposed,  (capacity.rs,    (in progress,      ──► ended (failed; trees
//!   │             rank order) budget, gate)    its turn runs)         deleted)
//!   │              ▲   │                           │ self.branch.built, green
//!   │   halt: the  │   │ held: slots, disk,        ▼
//!   │   build is   │   │ memory, CPU, budget  REVIEWING ── self-join: the planted revert,
//!   │   stopped ───┘   │ (a row when the           │       the reviewer on another model
//!   │   and returns    │  reason changes)          │
//!   │                  ▼                    accept │            reject, the first time
//!   │            (stays ready)                     ▼            ──► returned to READY with
//!   │                                          JOINING                the findings
//!   └── reject once ◄──────────────────────── (queued)          reject, the second time
//!                                                  │            ──► dropped with a notice
//!                       the join lock, one at a time │                (failed; `theseus self retry`)
//!                                                  ▼
//!                                   JOINED (self.joined): its trees deleted (cleared)
//!                                   or HELD at the keel, the gate, the signature: the owner's
//! ```
//!
//! **Starts.** A tick ([`Core::pipeline_tick`]) asks [`super::gate`] first
//! and before each start, and starts the top ready item while fewer than
//! `pipeline_builds` build, fewer self trees are open, the self budget's day
//! line is not reached, and the capacity guard ([`super::capacity`]) finds
//! room: every check every time, never skipped for a free slot. A held start
//! says why (`theseus self pipeline`, and a `self.pipeline.held` row when the
//! reason changes). A start is the [`Builder`]'s: [`Local`] (self-pr's
//! sandboxed tree, through the backlog's own start: a `"self"` session and
//! its first turn) is the only one. A tick that started at least one build
//! stages one `self_digest` line in the night's words. While the pipeline runs,
//! the night files and ranks and starts nothing itself.
//!
//! **Reviews and joins** are self-join's: [`Core::self_join_pass`] reviews
//! each green branch once per head and joins at most one a pass, under
//! `[self] join_lock`. Passes run one at a time, the pipeline's and the join
//! tender's alike (`Shared::joins`, taken inside the pass), so two reviews of
//! one head or two joins never run at once in a daemon, and the mkdir lock
//! keeps a self-join off the owner's own joins. A rejected review puts its
//! item back to proposed (self-join does, with the reasons in its evidence):
//! the first time the pipeline says so (`returned`) and builds it again, the
//! reviewer's findings in its first turn; the second time it drops it with a
//! notice carrying both findings and the retry command (`failed`).
//!
//! **The owner's words act at once**: a drop stops its build and deletes its
//! trees, a move reorders ready, and a halt stops every build in flight at
//! the next tick (each returns to ready) and starts nothing until the
//! owner's resume. Each wakes the tender ([`Shared::kick`]).
//!
//! **A closed item leaves no tree** (theseus-czlzg): each tick also closes the
//! tree of every item that is failed or done and whose branch waits on no
//! review, join or owner ([`queues`]'s `closed`), whatever ended it (the
//! backlog's sweep, a cancel, a drop): one `cleared` row, and its branch is
//! deleted when it holds no commit beyond main (one with commits stays, as
//! the evidence `theseus self branches` lists). A tree whose item is
//! building, reviewing, joining or held at the keel is never touched.
//!
//! Every transition is one `self.pipeline.*` row ([`crate::fact::rsi_pipeline`]),
//! written once: the META record `self.pipeline` keeps the keys of what was
//! said, with the tick's task records and rows in one frame.

pub(crate) mod queues;
mod tender;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_fetch;
#[cfg(test)]
mod tests_notify;
#[cfg(test)]
mod tests_trees;
mod view;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_protocol::tasks::{TaskEvidence, TaskState};
use theseus_store::{kinds, NewRecord};

use self::queues::{Entry, Queues};
use super::capacity::{self, Hold, Probe};
use super::{gate, tree, Gate};
use crate::fact::rsi_pipeline::{
    Cleared, Ended, Held, Moved, Parked, Queued, Returned, Started, Transition,
};
use crate::rpc::Core;

/// The META record of what the pipeline said.
pub const MARK_KEY: &str = "self.pipeline";
/// How long a started build counts as building before its turn has begun.
pub const GRACE: Duration = Duration::from_secs(5 * 60);
/// The keys the mark keeps (oldest dropped first).
const KEEP: usize = 4_096;

/// What the pipeline holds in memory: nothing that must survive a restart.
#[derive(Default)]
pub struct Shared {
    /// One tick at a time.
    tick: tokio::sync::Mutex<()>,
    /// One self-join pass at a time, the pipeline's and the join tender's.
    pub(crate) joins: tokio::sync::Mutex<()>,
    /// The tender runs (mode act, after serving): the night starts nothing.
    runs: AtomicBool,
    /// A join pass is spawned and not yet over.
    joining: AtomicBool,
    /// The builds in flight were stopped for the halt now in force.
    halt_swept: AtomicBool,
    /// Wakes the tender: a drop, a move, a halt, a night, a built branch.
    kick: Arc<tokio::sync::Notify>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("runs", &self.runs.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Shared {
    /// Whether the pipeline's tender runs, so it, not the night, starts.
    pub fn runs(&self) -> bool {
        self.runs.load(Ordering::Relaxed)
    }

    /// Wake the tender for a word that acts at once.
    pub fn kick(&self) {
        self.kick.notify_one();
    }
}

/// Who builds a ready item.
///
/// [`Local`] is the only one: self-pr's sandboxed tree on this machine. The
/// seam is for a **remote builder**, which would start the same `"self"`
/// work elsewhere and report back the same `self.branch.built` row (seam
/// S4) with a branch pushed to `[self] remote`: an AWS hand (`aws.hands.run`
/// with the repo, a build image and the item's brief; it needs the hands'
/// account and its spend line) or a cloud session (a branch and a brief, as
/// the outer loop launches them; it needs that service's credential and its
/// spend). Each needs a credential or a spend line, which is the hard keel:
/// the owner's decision, so none is built here.
pub trait Builder: Send + Sync {
    /// Its name, as `theseus self pipeline` and the rows say it.
    fn name(&self) -> &'static str;

    /// Start a build of `item` (a proposed backlog item), with a rejected
    /// review's `findings` when it is built again. The session it runs in;
    /// none when the item was no longer proposed.
    fn start(&self, core: &Arc<Core>, item: &str, findings: Option<&str>)
        -> Result<Option<String>>;
}

/// Self-pr's sandboxed tree on this machine: the backlog's own start (a
/// `"self"` session) and its first turn, which calls `self.propose_change`.
pub struct Local;

impl Builder for Local {
    fn name(&self) -> &'static str {
        "local"
    }

    fn start(
        &self,
        core: &Arc<Core>,
        item: &str,
        findings: Option<&str>,
    ) -> Result<Option<String>> {
        let Some(mut s) = core.backlog_start_item(item)? else {
            return Ok(None);
        };
        if let Some(f) = findings {
            s.objective.push_str(&format!(
                "\n\nAn independent review rejected the last branch for this item: {f}\n\
                 Address what it found."
            ));
        }
        let session = s.session_id.clone();
        let c = core.clone();
        tokio::spawn(async move { c.backlog_run(s).await });
        Ok(Some(session))
    }
}

/// What a tick reaches outside the core.
#[derive(Clone)]
pub struct Seams {
    pub builder: Arc<dyn Builder>,
    pub probe: Arc<dyn Probe>,
    pub join: super::join::Seams,
    /// How long a started build counts as building before its turn begins.
    pub grace: Duration,
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            builder: Arc::new(Local),
            probe: Arc::new(capacity::Host),
            join: super::join::Seams::default(),
            grace: GRACE,
        }
    }
}

/// What one tick did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Tick {
    pub gate: String,
    /// Items started, in order.
    pub started: Vec<String>,
    /// The hold on the next start, if one held it.
    pub hold: Option<String>,
    /// The `self.pipeline.*` kinds written, in order.
    pub said: Vec<String>,
    /// Builds stopped (a halt, a drop).
    pub stopped: Vec<String>,
    /// A branch waits on a review or a join.
    pub join_due: bool,
}

/// What the pipeline said, kept with its rows.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Mark {
    /// Keys of transitions said (`queued:<branch>@<head>`, …).
    #[serde(default)]
    seen: Vec<String>,
    /// The hold last said; none while starts go.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hold: Option<String>,
}

impl Mark {
    fn read(store: &crate::store::Store) -> Result<Mark> {
        Ok(store.get_meta::<Mark>(MARK_KEY)?.unwrap_or_default())
    }

    fn record(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(MARK_KEY), self)
    }

    fn said(&self, key: &str) -> bool {
        self.seen.iter().any(|k| k == key)
    }

    fn note(&mut self, key: String) {
        self.seen.push(key);
        if self.seen.len() > KEEP {
            let cut = self.seen.len() - KEEP;
            self.seen.drain(..cut);
        }
    }
}

/// A tick's writes: task records, rows, and the mark, in one frame.
struct Write {
    mark: Mark,
    frame: Vec<NewRecord>,
    said: Vec<Transition>,
    dirty: bool,
    /// Notes to the owner, staged once the tick's frame is written.
    told: Vec<String>,
}

impl Write {
    fn say(&mut self, key: String, t: Transition) -> Result<()> {
        self.frame.push(t.row()?);
        self.said.push(t);
        self.mark.note(key);
        self.dirty = true;
        Ok(())
    }

    fn task(&mut self, t: &theseus_protocol::tasks::TaskRecord) -> Result<()> {
        self.frame.push(crate::task_graph::record(t)?);
        self.dirty = true;
        Ok(())
    }
}

/// The note to the owner for an item two reviews rejected: both findings
/// and the command back. Nothing waits for an answer.
pub(crate) fn rejected_twice_text(e: &Entry) -> String {
    use crate::owner_note::{note, Kind};
    let mut body = format!("Two independent reviews rejected {}:\n", e.title);
    for (i, f) in e.findings.iter().enumerate() {
        let f: String = f.chars().take(300).collect();
        body.push_str(&format!("- review {}: {f}\n", i + 1));
    }
    body.push_str(&format!(
        "It left the pipeline (item {}); nothing waits for you. Bring it back with `{}`.",
        e.item,
        theseus_protocol::rsi::retry_command(&e.item)
    ));
    note(
        Kind::Fyi,
        "Self-improvement: an item was dropped after two rejections",
        "the pipeline's reviews",
        &body,
    )
}

fn evidence(identity: String, note: String, now: u64) -> TaskEvidence {
    TaskEvidence {
        node: None,
        identity,
        note: Some(note),
        by: "self-pipeline".into(),
        at_ms: now,
    }
}

impl Core {
    /// Write a tick's frame and say its rows.
    fn pipeline_write(&self, w: &mut Write) -> Result<Vec<String>> {
        if !w.dirty {
            return Ok(vec![]);
        }
        w.frame.push(w.mark.record()?);
        self.store.append(&std::mem::take(&mut w.frame))?;
        w.dirty = false;
        let rec = self.rec(None);
        let mut kinds = Vec::new();
        for t in w.said.drain(..) {
            t.announce(&rec);
            kinds.push(kind_of(&t).to_string());
        }
        Ok(kinds)
    }

    /// One tick of the pipeline: the module's doc.
    pub async fn pipeline_tick(self: &Arc<Self>, seams: &Seams) -> Result<Tick> {
        let _one = self.rsi.pipeline.tick.lock().await;
        let mut tick = Tick::default();
        let g = gate(&self.self_ctx());
        tick.gate = g.name().into();
        match g {
            Gate::Off => return Ok(tick),
            Gate::Halted { .. } => {
                if !self.rsi.pipeline.halt_swept.swap(true, Ordering::SeqCst) {
                    self.pipeline_halt(seams, &mut tick).await?;
                }
                return Ok(tick);
            }
            Gate::Allowed => self.rsi.pipeline.halt_swept.store(false, Ordering::SeqCst),
        }
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Ok(tick);
        };
        let place = self.tools.self_pr.place(&repo);
        let Some(q) = theseus_store::blocking(|| self.pipeline_queues(&place, seams.grace))? else {
            return Ok(tick);
        };
        let mut w = Write {
            mark: Mark::read(&self.store)?,
            frame: vec![],
            said: vec![],
            dirty: false,
            told: vec![],
        };
        self.pipeline_moves(&place, &q, &mut w, &mut tick).await?;
        tick.said.extend(self.pipeline_write(&mut w)?);
        for text in std::mem::take(&mut w.told) {
            self.stage_self_digest(&text);
        }
        tick.join_due = !q.reviewing.is_empty() || !q.joining.is_empty();
        let started = self.pipeline_starts(seams, &place, &q, &mut w, &mut tick);
        // One line a tick, naming every build it started, even when a later
        // start was held or failed (theseus-o1h3f.6).
        let told: Vec<(String, String)> = tick
            .started
            .iter()
            .map(|i| {
                let title = q
                    .ready
                    .iter()
                    .find(|e| &e.item == i)
                    .map(|e| e.title.clone());
                (i.clone(), title.unwrap_or_else(|| i.clone()))
            })
            .collect();
        self.pipeline_tell(&told);
        started?;
        Ok(tick)
    }

    /// The transitions that start nothing: parked, returned, queued, ended,
    /// dropped and joined.
    async fn pipeline_moves(
        &self,
        place: &tree::Place,
        q: &Queues,
        w: &mut Write,
        tick: &mut Tick,
    ) -> Result<()> {
        self.pipeline_reviewed(q, w)?;
        self.pipeline_ended(place, q, w)?;
        for e in &q.dropped {
            if let (Some(session), Some(exec)) = (&e.session, &e.execution) {
                let key = format!("stopped:{}:{session}", e.item);
                if e.live && !w.mark.said(&key) {
                    self.stop_execution(exec, "self-pipeline: dropped").await?;
                    tick.stopped.push(e.item.clone());
                    let m = Moved::new(&e.item, format!("the build of {} stopped", e.item))
                        .with("session", session.clone())
                        .with("reason", "dropped")
                        .why("the owner dropped it");
                    w.say(key, Transition::Ended(Ended(m)))?;
                }
            }
            clear(place, e, "dropped", w)?;
        }
        for e in &q.joined {
            clear(place, e, "joined", w)?;
        }
        // Whatever else ended an item without a join leaves no tree.
        for e in &q.closed {
            clear(place, e, "closed", w)?;
        }
        Ok(())
    }

    /// What the reviews moved: parked, returned and queued.
    fn pipeline_reviewed(&self, q: &Queues, w: &mut Write) -> Result<()> {
        let now = theseus_protocol::now_unix_ms();
        for e in &q.parking {
            let key = format!("parked:{}", e.item);
            let Some(mut t) = crate::task_graph::get(&self.store, &e.item)? else {
                continue;
            };
            // Dropped, never parked (theseus-o1h3f.6): failed, with both
            // reviews' findings in its evidence, and the owner's retry
            // brings it back. Nothing waits for a word.
            t.state = TaskState::Failed;
            t.version += 1;
            t.updated_at_ms = now;
            t.evidence.push(evidence(
                "dropped:rejected-twice".into(),
                format!(
                    "two independent reviews rejected it: {}",
                    e.findings.join(" | ")
                ),
                now,
            ));
            w.task(&t)?;
            let m = Moved::new(
                &e.item,
                format!("{} was dropped after two rejections", e.item),
            )
            .with("rejections", e.rejections)
            .with("findings", e.findings.clone())
            .why("an independent review rejected it twice")
            .numbers(json!({"rejections": e.rejections}))
            .undo(theseus_protocol::rsi::retry_command(&e.item));
            w.say(key, Transition::Parked(Parked(m)))?;
            w.told.push(rejected_twice_text(e));
        }
        for e in &q.ready {
            let (Some(branch), Some(head)) = (&e.branch, &e.head) else {
                continue;
            };
            let key = format!("returned:{branch}@{head}");
            if e.rejections == 0 || w.mark.said(&key) {
                continue;
            }
            let m = Moved::new(
                &e.item,
                format!("{} is ready again after a rejection", e.item),
            )
            .with("branch", branch.clone())
            .with("head", head.clone())
            .with("findings", e.why.clone().unwrap_or_default())
            .why("an independent review rejected its branch, the first time")
            .numbers(json!({"rejections": e.rejections}));
            w.say(key, Transition::Returned(Returned(m)))?;
        }
        for e in &q.joining {
            let (Some(branch), Some(head)) = (&e.branch, &e.head) else {
                continue;
            };
            let key = format!("queued:{branch}@{head}");
            if w.mark.said(&key) {
                continue;
            }
            let m = Moved::new(&e.item, format!("{branch} waits for the join lock"))
                .with("branch", branch.clone())
                .with("head", head.clone())
                .why("its review passed")
                .numbers(json!({"place": e.place}));
            w.say(key, Transition::Queued(Queued(m)))?;
        }
        Ok(())
    }

    /// Builds whose turn ended with no green branch: failed, trees deleted.
    fn pipeline_ended(&self, place: &tree::Place, q: &Queues, w: &mut Write) -> Result<()> {
        let now = theseus_protocol::now_unix_ms();
        for e in &q.ended {
            if let Some(mut t) = crate::task_graph::get(&self.store, &e.item)? {
                t.state = TaskState::Failed;
                t.version += 1;
                t.updated_at_ms = now;
                t.evidence.push(evidence(
                    "ended:no-branch".into(),
                    "its build's turn ended with no green branch".into(),
                    now,
                ));
                w.task(&t)?;
            }
            let session = e.session.clone().unwrap_or_default();
            let m = Moved::new(&e.item, format!("the build of {} ended", e.item))
                .with("session", session.clone())
                .with("reason", "no green branch")
                .why("its turn ended with no green branch");
            w.say(
                format!("ended:{}:{session}", e.item),
                Transition::Ended(Ended(m)),
            )?;
            clear(place, e, "ended", w)?;
        }
        Ok(())
    }

    /// The next start's hold, if any: the budget's day line, then the guard.
    fn pipeline_hold(
        &self,
        seams: &Seams,
        place: &tree::Place,
        building: usize,
        waiting: &[Entry],
    ) -> (capacity::Room, Option<(String, String)>) {
        let cfg = &self.cfg.self_improve;
        let at = [place.root(), place.state.clone()]
            .into_iter()
            .find(|p| p.exists())
            .unwrap_or_else(|| place.state.clone());
        // A ready item's own tree (a halted build's) is no slot taken: its
        // start uses it.
        let waits = waiting
            .iter()
            .filter(|e| {
                place
                    .trees()
                    .join(tree::slug(&e.item))
                    .join(".git")
                    .exists()
            })
            .count();
        let trees = capacity::trees_open(&place.trees()).saturating_sub(waits);
        let (room, hold) = capacity::check(
            &*seams.probe,
            &at,
            building,
            trees,
            cfg.pipeline_builds,
            cfg.min_free_gb,
        );
        if let Some(b) = self.runner.self_budget.view() {
            if b.day_spent_usd >= b.day_line_usd {
                let why = format!(
                    "the self budget's day line is reached: ${:.2} of ${:.2} today",
                    b.day_spent_usd, b.day_line_usd
                );
                return (room, Some(("budget".into(), why)));
            }
        }
        let hold = hold.map(|h: Hold| (h.name().to_string(), h.why(&room)));
        (room, hold)
    }

    /// The next start waits: say why once, until the reason changes.
    fn pipeline_held(
        &self,
        w: &mut Write,
        tick: &mut Tick,
        name: &str,
        why: String,
        numbers: serde_json::Value,
    ) -> Result<()> {
        tick.hold = Some(name.to_string());
        if w.mark.hold.as_deref() != Some(name) {
            w.mark.hold = Some(name.to_string());
            let m = Moved::new("", format!("the next start waits: {name}"))
                .with("hold", name)
                .why(why)
                .numbers(numbers);
            w.say(format!("held:{name}"), Transition::Held(Held(m)))?;
            tick.said.extend(self.pipeline_write(w)?);
        }
        Ok(())
    }

    /// Start the top ready items while there is room.
    fn pipeline_starts(
        self: &Arc<Self>,
        seams: &Seams,
        place: &tree::Place,
        q: &Queues,
        w: &mut Write,
        tick: &mut Tick,
    ) -> Result<()> {
        let mut building = q.building.len();
        for (i, e) in q.ready.iter().enumerate() {
            if gate(&self.self_ctx()) != Gate::Allowed {
                break;
            }
            let (room, hold) = self.pipeline_hold(seams, place, building, &q.ready[i..]);
            if let Some((name, why)) = hold {
                return self.pipeline_held(w, tick, &name, why, json!(room));
            }
            let findings = (e.rejections > 0).then(|| e.why.clone()).flatten();
            let core = self.clone();
            let (b, item) = (seams.builder.clone(), e.item.clone());
            let started =
                theseus_store::blocking(move || b.start(&core, &item, findings.as_deref()));
            let session = match started {
                Ok(s) => s,
                // No tree, no start: the hold says why, as the guard's do.
                Err(e) if e.is::<super::backlog::NoTree>() => {
                    return self.pipeline_held(w, tick, "tree", e.to_string(), json!(room));
                }
                Err(e) => return Err(e),
            };
            let Some(session) = session else {
                continue;
            };
            building += 1;
            w.mark.hold = None;
            let m = Moved::new(
                &e.item,
                format!("started {} on the {} builder", e.item, seams.builder.name()),
            )
            .with("builder", seams.builder.name())
            .with("session", session)
            .why(match e.rejections {
                0 => "the top ready item, with a slot and room".to_string(),
                _ => "built again after a rejection, with the reviewer's findings".to_string(),
            })
            .numbers(
                json!({"building": building, "slots": room.slots, "trees": room.trees,
                            "free_gb": room.free_gb, "rank": e.place}),
            );
            w.say(
                format!("started:{}:{}", e.item, theseus_protocol::now_unix_ms()),
                Transition::Started(Started(m)),
            )?;
            tick.said.extend(self.pipeline_write(w)?);
            tick.started.push(e.item.clone());
        }
        if q.ready.is_empty() && w.mark.hold.is_some() {
            // Nothing waits, so nothing is held: say the next hold again.
            w.mark.hold = None;
            w.dirty = true;
            self.pipeline_write(w)?;
        }
        Ok(())
    }

    /// The halt: every build in flight is stopped and returns to ready, so
    /// the owner's resume starts it again.
    async fn pipeline_halt(&self, seams: &Seams, tick: &mut Tick) -> Result<()> {
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Ok(());
        };
        let place = self.tools.self_pr.place(&repo);
        let Some(q) = theseus_store::blocking(|| self.pipeline_queues(&place, seams.grace))? else {
            return Ok(());
        };
        let mut w = Write {
            mark: Mark::read(&self.store)?,
            frame: vec![],
            said: vec![],
            dirty: false,
            told: vec![],
        };
        let now = theseus_protocol::now_unix_ms();
        for e in &q.building {
            if let (Some(exec), true) = (&e.execution, e.live) {
                self.stop_execution(exec, "self-pipeline: halted").await?;
            }
            if let Some(mut t) = crate::task_graph::get(&self.store, &e.item)? {
                t.state = TaskState::Proposed;
                t.session = None;
                t.version += 1;
                t.updated_at_ms = now;
                t.evidence.push(evidence(
                    "halted:".to_string() + e.session.as_deref().unwrap_or(""),
                    "the kill switch stopped its build; it is ready again".into(),
                    now,
                ));
                w.task(&t)?;
            }
            let session = e.session.clone().unwrap_or_default();
            let m = Moved::new(&e.item, format!("the build of {} stopped", e.item))
                .with("session", session.clone())
                .with("reason", "halted")
                .why("the kill switch is on");
            w.say(
                format!("stopped:{}:{session}", e.item),
                Transition::Ended(Ended(m)),
            )?;
            tick.stopped.push(e.item.clone());
        }
        tick.said.extend(self.pipeline_write(&mut w)?);
        Ok(())
    }

    /// One self-join pass for the pipeline, unless one runs already: the
    /// pass itself takes `Shared::joins`.
    pub async fn pipeline_join(
        self: &Arc<Self>,
        seams: &Seams,
    ) -> Result<Option<super::join::Pass>> {
        if self.rsi.pipeline.joining.swap(true, Ordering::SeqCst) {
            return Ok(None);
        }
        let pass = self.self_join_pass(&seams.join).await;
        self.rsi.pipeline.joining.store(false, Ordering::SeqCst);
        pass.map(Some)
    }
}

/// Delete an item's trees (its worktree and its build tree: regenerable,
/// never moved to a trash) and its branch when it holds no commit, with one
/// `cleared` row when there was any.
fn clear(place: &tree::Place, e: &Entry, reason: &str, w: &mut Write) -> Result<()> {
    // git and the disk, off the runtime's worker (theseus_store::blocking).
    let gone = theseus_store::blocking(|| -> Result<Vec<String>> {
        let mut gone = Vec::new();
        if let Some(t) = tree::find(place, &e.item) {
            tree::close(place, &t)?;
            gone.push(t.dir.display().to_string());
            // A branch with no commit of its own is nothing to keep; one
            // with commits stays, as the evidence `self branches` lists.
            if tree::commits_beyond_base(place, &t.branch)? == 0 {
                tree::delete_branch(place, &t.branch)?;
                gone.push(format!("branch {}", t.branch));
            }
        }
        let target = place.targets().join(tree::slug(&e.item));
        if target.exists() {
            std::fs::remove_dir_all(&target)?;
            gone.push(target.display().to_string());
        }
        Ok(gone)
    })?;
    if gone.is_empty() {
        return Ok(());
    }
    let m = Moved::new(&e.item, format!("the trees of {} are deleted", e.item))
        .with("trees", gone.clone())
        .with("reason", reason)
        .why(format!("its build {reason}"))
        .numbers(json!({"trees": gone.len()}));
    let key = format!("cleared:{}:{}", e.item, theseus_protocol::now_unix_ms());
    w.say(key, Transition::Cleared(Cleared(m)))
}

fn kind_of(t: &Transition) -> &'static str {
    match t {
        Transition::Started(_) => "self.pipeline.started",
        Transition::Held(_) => "self.pipeline.held",
        Transition::Returned(_) => "self.pipeline.returned",
        Transition::Parked(_) => "self.pipeline.parked",
        Transition::Queued(_) => "self.pipeline.queued",
        Transition::Cleared(_) => "self.pipeline.cleared",
        Transition::Ended(_) => "self.pipeline.ended",
    }
}
