//! The work board (theseus-hkx30, the substrate design's §4): one `WorkView`
//! per conversation, task, plan step, job, open question and pending wake,
//! in one tree, kept as a projection of the frames the kernel already
//! writes. It is fed by the push's one applier pass, after the execution
//! board (`board.rs`) has taken the same frame, and writes nothing: no frame,
//! no record, no timer.
//!
//! - **Inputs, not views, are kept.** An execution's `ExecutionView` (the
//!   board's), its decoded wakes and parent; each open action; each plan
//!   step's task record; each session's title; a few rows (`turn.started`,
//!   `loop.ended`, `tool.job_started`, `wake.fired`, the confirm rows). A frame
//!   marks the entities it touched dirty; they are built again, each view is
//!   compared with the one held, and the ancestors of what changed are
//!   rolled up again (`below`, `progress`), deepest first.
//! - **One entity per action.** A question's id is its action's
//!   correlation id; an approved `proc.run` goes on as the job under the
//!   same id, so the answer retracts the ping on the view the owner is
//!   looking at.
//! - **Notices** are `notices::policy(prev, next)` for each changed view, at
//!   most one a view a frame, kept in a ring of the last `RING` with their
//!   positions (`notices_since`).
//! - **Ended children** stay in a root's tree, the 50 newest ended per root;
//!   older ones leave (`gone`) and stay counted in the root's `below`.
//! - **No timer.** A question's reminder (rule 2 of the policy) is a view
//!   against itself later; nothing wakes the board for it.

mod digest;
mod eta;
mod forecast;
mod horizon;
mod place;
mod progress;
mod system;
mod threads;
mod views;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use theseus_kernel::{Action, ActionState, Execution, PendingWake, PROVIDER_TOOL};
use theseus_protocol::notices::{policy, Notice, Rules};
use theseus_protocol::tasks::{TaskRecord, TaskState};
use theseus_protocol::work::{Question, Rollup, Woke, WorkKind, WorkState, WorkView};
use theseus_protocol::{ExecutionView, Level, SessionKind};
use theseus_store::kinds;

pub use digest::DigestRead;
pub use place::Scope;

use super::board::Board;
use super::{hm, Frame};
use crate::ledger::LedgerRow;
use crate::session::SessionRecord;
use crate::Core;

use eta::EtaBook;

use crate::forecast::Outcome;

/// The notices the board keeps for `work-digest`.
pub const RING: usize = 4096;
/// The `wake.fired` rows the board keeps for the digest.
pub const FIRED: usize = 1024;

/// The rows kept for an action the board has not seen yet.
const EARLY: usize = 256;

/// The ended children a root keeps in its tree.
pub const ENDED_KEPT: usize = 50;

/// The ledger rows the work board reads: a cheap check of a row's kind,
/// made on the committing thread (`wanted`), so a frame's other rows are
/// never copied.
const ROWS: &[&str] = &[
    "execution.queued",
    "turn.started",
    "loop.ended",
    "tool.job_started",
    "tool.confirm_requested",
    "action.confirmed",
    // Who answered, when an agent did (theseus-5rvss): the kernel's own row names the operator.
    "action.confirm_answered",
    "action.declined",
    "action.denied",
    "action.expired",
    "wake.",
    "watch.",
    "task.",
    // The daemon's own work (theseus-jn17m): `system::KINDS`.
    "import.",
    "synthesis.proposed",
    "index.rebuild",
    "judge.call",
];

/// Whether a ledger record's payload is a row the work board reads. The kind
/// is the row's second field, so a few dozen bytes are looked at; a payload
/// whose kind cannot be found this way is kept, so a change of layout
/// costs speed and never a row.
pub(super) fn wanted(payload: &[u8]) -> bool {
    let head = &payload[..payload.len().min(160)];
    const KEY: &[u8] = b"\"kind\":\"";
    let Some(at) = head.windows(KEY.len()).position(|w| w == KEY) else {
        return true;
    };
    let rest = &head[at + KEY.len()..];
    if rest.starts_with(b"judge.call") {
        // Only the loop pack's judgments are read (a task's forecast words
        // the judge's latest read of it); the rest are not copied.
        const PACK: &[u8] = b"\"pack\":\"loop.";
        return payload.windows(PACK.len()).any(|w| w == PACK);
    }
    ROWS.iter().any(|p| rest.starts_with(p.as_bytes()))
}

/// A root that needs you or works, itself or below. The system root is
/// listed with its jobs while it holds any, ended ones among them (they stay
/// an hour).
fn is_active(v: &WorkView) -> bool {
    matches!(v.attention.level, Level::NeedsYou | Level::Working)
        || v.below.is_some_and(|b| b.needs_you > 0 || b.working > 0)
        || (v.kind == WorkKind::System && v.below.is_some())
}

/// An execution's inputs: what the board's view lacks.
struct ExecIn {
    x: ExecutionView,
    exec_pos: u64,
    kind: WorkKind,
    parent: Option<String>,
    wakes: Vec<PendingWake>,
    created_at_ms: u64,
    ended_reason: Option<String>,
    title: String,
    turn_started_ms: u64,
    /// When the newest of the execution's calls settled: the model has been thinking since its result
    /// came back, not since the turn began (theseus-ztkhg). In memory only.
    last_settled_ms: u64,
    loop_index: u32,
    /// The newest `say` of a `loop.ended` row: the model's own line before
    /// its tool calls (theseus-xlias). Kept until the next one.
    said: Option<String>,
    /// The profile a checkpoint moved the task to, off its base: the newest
    /// `loop.ended` row's `on`, none once it is back (theseus-ypnxt.9).
    on: Option<String>,
    woke: Option<Woke>,
    /// A task's forecast, said once (theseus-zz752), and the loops it has run.
    forecast: Option<Outcome>,
    loops_total: u32,
    /// When the task began to work: its creation, unless it was asleep
    /// (a deferred start), then the frame that woke it; 0 while asleep.
    ran_from_ms: u64,
    /// Jev's latest read of it (`loop.v1`'s `work_state` and its confidence).
    read: Option<(String, f32)>,
    /// Its row joined the table of finished tasks.
    recorded: bool,
    /// The owner asked to hear when this task is done (`task.create
    /// { notify_done }`, theseus-j7cje).
    flagged: bool,
    /// The task was started expecting to run long (theseus-bh3t.1).
    expect: Option<theseus_protocol::task_start::Expect>,
    /// The owner holds this task (`task.pause`).
    paused: bool,
}

/// An action the board shows: a question, or a job.
struct ActIn {
    a: Action,
    /// The position of the record it was read from.
    pos: u64,
    /// Its argv, from `tool.job_started` or the proposal.
    argv: Vec<String>,
    /// It was a question at some point: its view stays a question's until it
    /// goes on as a job.
    asked: bool,
    /// `approved`, `declined`, or `expired`, once known.
    answer: Option<(String, Option<String>)>,
    /// The confirm row's words, when it came.
    detail: Option<String>,
    change: bool,
    floor: bool,
    expires_at_ms: Option<u64>,
    /// Its time joined its program's.
    noted: bool,
}

/// A wake that left an execution's list.
struct EndedWake {
    exec: String,
    wake: PendingWake,
    at_ms: u64,
    fired: bool,
}

/// A frame's inputs, decoded once.
struct Taken {
    execs: Vec<(u64, Execution)>,
    actions: Vec<(u64, Action)>,
    rows: Vec<LedgerRow>,
    /// The frame's `wake.fired` rows with their own positions (the digest).
    fired: Vec<(u64, LedgerRow)>,
    tasks: Vec<(u64, TaskRecord)>,
    sessions: Vec<SessionRecord>,
    /// Routine records the frame carried (horizon).
    routines: Vec<theseus_protocol::routines::RoutineSpec>,
}

/// What a frame changed on the board: the work board's `work.changed`, and
/// the notices the policy made of it.
#[derive(Debug, Default)]
pub struct Out {
    pub position: u64,
    pub at_ms: u64,
    pub views: Vec<WorkView>,
    /// Ids that left the board, with the root each was under.
    pub gone: Vec<(String, String)>,
    pub notices: Vec<Notice>,
    /// The thread groups that changed (a thread's write, its session's bind).
    pub threads: Vec<theseus_protocol::ThreadGroup>,
}

#[derive(Default)]
pub(super) struct WorkBoard {
    execs: HashMap<String, ExecIn>,
    by_session: HashMap<String, String>,
    acts: HashMap<String, ActIn>,
    acts_by_exec: HashMap<String, BTreeSet<String>>,
    steps: HashMap<String, TaskRecord>,
    /// Task records with a session, by their execution: a task's title and
    /// its report.
    task_of_exec: HashMap<String, TaskRecord>,
    ended_wakes: HashMap<String, EndedWake>,
    /// Rows about an action that came before the action did (a turn's rows
    /// and the kernel's frames are written by different threads), by
    /// correlation id, the newest 256.
    early: VecDeque<LedgerRow>,
    views: HashMap<String, WorkView>,
    kids: HashMap<String, BTreeSet<String>>,
    /// What a root's pruned children were, by level.
    pruned: HashMap<String, Rollup>,
    eta: EtaBook,
    /// The jobs running now, by id, with their sessions: what the last-line
    /// sampler walks (theseus-e1hu4).
    running: HashMap<String, String>,
    /// A running job's last output line as last sampled; its view carries it.
    last: HashMap<String, String>,
    /// The tasks that passed their forecast's p90, once each (theseus-zz752).
    past: HashSet<String>,
    /// Views the warm-up changed in place: the next frame sends them again.
    stale: BTreeSet<String>,
    /// The daemon's own jobs (theseus-jn17m), from the import's, the
    /// consolidation's and the rebuild's rows.
    system: system::SystemJobs,
    position: u64,
    /// The position the board was seeded at: notices before it are unknown.
    seeded_at: u64,
    ring: VecDeque<Notice>,
    /// The newest position of a notice that left the ring.
    evicted: u64,
    /// The last `FIRED` `wake.fired` rows with their positions, and the newest
    /// position of one that left: what `work.digest` tells a wake from, the
    /// one fact the ledger's path reads too (theseus-qkoi3).
    fired: VecDeque<(u64, LedgerRow)>,
    fired_evicted: u64,
    rules: Rules,
    /// The threads' groups by slug, and the thread each session serves
    /// (`threads.rs`).
    thread_groups: std::collections::BTreeMap<String, theseus_protocol::ThreadGroup>,
    thread_of: HashMap<String, String>,
    /// Routines and watches by their wake's id (`horizon.rs`).
    routines: HashMap<String, horizon::RoutineIn>,
    watches: HashMap<String, horizon::WatchIn>,
    /// The newest frame's time: the clock a view's windows count from.
    now_ms: u64,
    /// Series that fired and went on: if one later leaves, it was run out.
    fired_series: HashSet<String>,
    /// The ids of the views that belong to the future (`horizon::is_future`).
    futures: HashSet<String>,
    /// The earliest a question's reminder may be due; 0 until a first look
    /// (`remind`, theseus-j7cje).
    remind_at: u64,
}

/// Entities marked for building again.
#[derive(Default)]
struct Dirty {
    execs: BTreeSet<String>,
    acts: BTreeSet<String>,
    wakes: BTreeSet<String>,
    steps: BTreeSet<String>,
    /// System jobs, and the root.
    system: BTreeSet<String>,
}

impl Dirty {
    fn is_empty(&self) -> bool {
        self.execs.is_empty()
            && self.acts.is_empty()
            && self.wakes.is_empty()
            && self.steps.is_empty()
            && self.system.is_empty()
    }
}

impl WorkBoard {
    pub(super) fn len(&self) -> usize {
        self.views.len()
    }

    // ------------------------------------------------------------- reads

    /// Every question open on the board, the longest waiting first. Memory
    /// only: no store read (`work-answer` resolves against these).
    pub(super) fn open_questions(&self) -> Vec<Question> {
        let mut qs: Vec<Question> = self
            .views
            .values()
            .filter(|v| v.kind == WorkKind::Question && !v.state.ended())
            .filter_map(|v| v.question.clone())
            .collect();
        qs.sort_by(|a, b| (a.asked_at_ms, &a.id).cmp(&(b.asked_at_ms, &b.id)));
        qs
    }

    /// The notices after `position`, oldest first, and whether the ring
    /// holds every notice since: false when the board did not exist at
    /// `position` or the ring let go of one after it.
    pub(super) fn notices_since(&self, position: u64) -> (Vec<Notice>, bool) {
        let out = self
            .ring
            .iter()
            .filter(|n| n.position > position)
            .cloned()
            .collect();
        let reaches = self.evicted.max(self.fired_evicted).max(self.seeded_at);
        (out, position >= reaches)
    }

    /// The `wake.fired` rows after `position`, oldest first.
    pub(super) fn fired_since(&self, position: u64) -> Vec<(u64, LedgerRow)> {
        self.fired
            .iter()
            .filter(|(p, _)| *p > position)
            .cloned()
            .collect()
    }

    /// The board as `work.watch` answers it: its position, every root that
    /// needs you or works with its open subtree (needs you first, the
    /// longest waiting first), then the `limit` most recently active other
    /// roots alone; and how many roots it holds. `scope` cuts it to a
    /// connection's roots.
    pub(super) fn snapshot(&self, limit: usize, scope: &Scope) -> (u64, Vec<WorkView>, u64) {
        let (mut active, mut rest): (Vec<&WorkView>, Vec<&WorkView>) = self
            .views
            .values()
            .filter(|v| v.parent.is_none() && scope.sees(&v.id))
            .partition(|v| is_active(v));
        let total = (active.len() + rest.len()) as u64;
        active.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.clone()));
        rest.sort_by_key(|v| (std::cmp::Reverse(v.at_ms), v.id.clone()));
        rest.truncate(limit);
        let mut out = Vec::new();
        for r in active {
            out.push(r.clone());
            self.open_below(&r.id, &mut out);
        }
        out.extend(rest.into_iter().cloned());
        // The future is kept beside the tree, whatever its root is doing.
        let held: HashSet<String> = out.iter().map(|v| v.id.clone()).collect();
        out.extend(
            self.future(scope)
                .into_iter()
                .filter(|v| !held.contains(&v.id))
                .cloned(),
        );
        (self.position, out, total)
    }

    /// The views below `id` that are not over, in tree order.
    fn open_below(&self, id: &str, out: &mut Vec<WorkView>) {
        let Some(kids) = self.kids.get(id) else {
            return;
        };
        for k in kids {
            let Some(v) = self.views.get(k) else {
                continue;
            };
            if v.state.ended() && v.attention.level != Level::NeedsYou && v.kind != WorkKind::System
            {
                continue;
            }
            out.push(v.clone());
            self.open_below(k, out);
        }
    }

    /// Every view, for a test's oracle.
    #[cfg(test)]
    pub(crate) fn all(&self) -> std::collections::BTreeMap<String, WorkView> {
        self.views
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// The usual time of a program, for a test.
    #[cfg(test)]
    pub(super) fn usual(&self, key: &str) -> Option<theseus_protocol::work::Eta> {
        self.eta.usual(key)
    }

    /// The jobs running now: (id, session), for the sampler.
    pub(super) fn running_jobs(&self) -> Vec<(String, String)> {
        self.running
            .iter()
            .map(|(id, s)| (id.clone(), s.clone()))
            .collect()
    }

    /// The usual time of a job's program, from its view.
    pub(super) fn usual_of(&self, id: &str) -> Option<u64> {
        self.views.get(id)?.eta.map(|e| e.usual_ms)
    }

    /// A running job's newest output line, sampled: its view says it from
    /// now on. Returns what changed on the board, none when the line is the
    /// one held or the job is not running. The view keeps its position (no
    /// frame was written) and takes the sample's time, so a client that
    /// compares (position, time) applies the newer sample and never an older
    /// snapshot (theseus-e1hu4).
    pub(super) fn sample(&mut self, id: &str, line: Option<String>, at_ms: u64) -> Option<Out> {
        if !self.running.contains_key(id) || self.last.get(id) == line.as_ref() {
            return None;
        }
        match &line {
            Some(l) => self.last.insert(id.to_string(), l.clone()),
            None => self.last.remove(id),
        };
        let v = self.views.get_mut(id)?;
        v.doing.as_mut()?.last_line = line;
        v.at_ms = at_ms;
        Some(Out {
            position: self.position,
            at_ms,
            views: vec![v.clone()],
            gone: Vec::new(),
            notices: Vec::new(),
            threads: Vec::new(),
        })
    }

    /// `work.agenda` (see `horizon.rs`).
    pub(super) fn work_agenda(
        &self,
        p: &theseus_protocol::work::horizon::AgendaParams,
        scope: &Scope,
        zone: &theseus_kernel::TimeZone,
    ) -> theseus_protocol::work::horizon::AgendaResult {
        self.agenda(p, scope, zone)
    }

    // ------------------------------------------------------------- apply

    /// Take one frame the execution board has applied: `changed` are the
    /// views it changed, and `board` holds the rest of its state. Returns
    /// what changed on this board, none when nothing did.
    pub(super) fn apply(
        &mut self,
        core: &Core,
        f: &Frame,
        board: &mut Board,
        changed: &[ExecutionView],
    ) -> Option<Out> {
        self.position = self.position.max(f.position);
        self.now_ms = self.now_ms.max(f.at_ms);
        let taken = Self::decode(core, f);
        for row in &taken.fired {
            if self.fired.len() == FIRED {
                if let Some((p, _)) = self.fired.pop_front() {
                    self.fired_evicted = self.fired_evicted.max(p);
                }
            }
            self.fired.push_back(row.clone());
        }
        let mut d = Dirty::default();
        for spec in &taken.routines {
            self.take_routine_spec(spec.clone(), &mut d);
        }
        self.take_rows(&taken.rows, &mut d);
        for t in &taken.tasks {
            self.take_task(core, t, &mut d);
        }
        for s in &taken.sessions {
            self.take_session(s, &mut d);
        }
        for (pos, a) in &taken.actions {
            self.take_action(board, *pos, a, &mut d);
        }
        self.take_execs(core, board, &taken, changed, &mut d);
        self.system.sweep(f.at_ms, &mut d.system);
        if d.is_empty() {
            return None;
        }
        let out = self.settle(f, d);
        (!out.views.is_empty() || !out.gone.is_empty()).then_some(out)
    }

    fn decode(core: &Core, f: &Frame) -> Taken {
        let mut t = Taken {
            execs: Vec::new(),
            actions: Vec::new(),
            rows: Vec::new(),
            fired: Vec::new(),
            tasks: Vec::new(),
            sessions: Vec::new(),
            routines: Vec::new(),
        };
        for (kind, pos, payload) in &f.records {
            match *kind {
                kinds::META => {
                    // A routine's record: its payload begins with the
                    // session id and holds its wake's.
                    if payload.starts_with(br#"{"session_id":""#)
                        && payload.windows(10).any(|w| w == br#""wake_id":"#)
                    {
                        if let Ok(r) = serde_json::from_slice(payload) {
                            t.routines.push(r);
                        }
                    }
                }
                kinds::EXECUTION => {
                    if let Ok(e) = core.kernel.decode_execution(payload) {
                        t.execs.retain(|(_, x)| x.id != e.id);
                        t.execs.push((*pos, e));
                    }
                }
                kinds::ACTION => {
                    if let Ok(a) = serde_json::from_slice::<Action>(payload) {
                        t.actions
                            .retain(|(_, x)| x.correlation_id != a.correlation_id);
                        t.actions.push((*pos, a));
                    }
                }
                kinds::LEDGER => {
                    if let Ok(r) = serde_json::from_slice::<LedgerRow>(payload) {
                        if r.kind == "wake.fired" {
                            t.fired.push((*pos, r.clone()));
                        }
                        t.rows.push(r);
                    }
                }
                kinds::TASK => {
                    if let Ok(r) = serde_json::from_slice::<TaskRecord>(payload) {
                        t.tasks.push((*pos, r));
                    }
                }
                kinds::SESSION => {
                    if let Ok(r) = serde_json::from_slice::<SessionRecord>(payload) {
                        t.sessions.push(r);
                    }
                }
                _ => {}
            }
        }
        t
    }

    /// The rows a frame carries: what an execution did, a job's command,
    /// a question's words, a wake's firing.
    fn take_rows(&mut self, rows: &[LedgerRow], d: &mut Dirty) {
        for r in rows {
            let s = |k: &str| r.data[k].as_str();
            match r.kind.as_str() {
                "turn.started" => {
                    if let Some(ei) = r.session_id.as_deref().and_then(|s| self.exec_of(s)) {
                        let id = ei.to_string();
                        if let Some(e) = self.execs.get_mut(&id) {
                            e.turn_started_ms = r.at_unix_ms;
                            e.loop_index = 0;
                            d.execs.insert(id);
                        }
                    }
                }
                "loop.ended" => {
                    if let Some(id) = r.session_id.as_deref().and_then(|s| self.exec_of(s)) {
                        let id = id.to_string();
                        if let Some(e) = self.execs.get_mut(&id) {
                            e.loop_index += 1;
                            if let Some(say) = s("say").filter(|x| !x.is_empty()) {
                                e.said = Some(say.to_string());
                            }
                            e.on = s("on").map(str::to_string);
                            e.loops_total += 1;
                            d.execs.insert(id);
                        }
                    }
                }
                "tool.job_started"
                | "tool.confirm_requested"
                | "action.declined"
                | "action.denied"
                | "action.expired"
                | "action.confirm_answered"
                | "action.confirmed" => {
                    let Some(corr) = s("correlation_id") else {
                        continue;
                    };
                    if !self.act_row(r, corr, d) {
                        if self.early.len() == EARLY {
                            self.early.pop_front();
                        }
                        self.early.push_back(r.clone());
                    }
                }
                "task.paused" | "task.resumed" => {
                    let on = r.kind == "task.paused";
                    if let Some(id) = r.session_id.as_deref().and_then(|s| self.exec_of(s)) {
                        let id = id.to_string();
                        if let Some(e) = self.execs.get_mut(&id) {
                            e.paused = on;
                            d.execs.insert(id);
                        }
                    }
                }
                "wake.fired" => {
                    self.take_horizon_row(r, d);
                    self.take_fired(r, d);
                }
                "wake.skipped" | "watch.checked" | "watch.met" | "watch.expired"
                | "watch.failing" => self.take_horizon_row(r, d),
                "judge.call" => self.take_read(r, d),
                k if system::KINDS.contains(&k) => self.system.take(r, &mut d.system),
                _ => {}
            }
        }
    }

    /// A row about an action, applied to it when the board has it. False
    /// when it does not (yet).
    fn act_row(&mut self, r: &LedgerRow, corr: &str, d: &mut Dirty) -> bool {
        let Some(ai) = self.acts.get_mut(corr) else {
            return false;
        };
        let s = |k: &str| r.data[k].as_str();
        match r.kind.as_str() {
            "tool.job_started" => {
                ai.argv = r.data["argv"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            "tool.confirm_requested" => {
                ai.detail = s("reason").filter(|x| !x.is_empty()).map(str::to_string);
                ai.change = !r.data["change"].is_null();
                ai.floor |= r.data["floor"].as_bool().unwrap_or(false);
            }
            "action.declined" | "action.denied" => {
                let by = s("by").map(|b| format!("by {b}"));
                ai.answer = Some(("declined".into(), by));
            }
            "action.expired" => ai.answer = Some(("expired".into(), None)),
            // Only an agent's answer changes what the kernel's row said, so
            // an owner's answer reads as it always did.
            "action.confirm_answered" => {
                if let Some(by) = s("by").filter(|b| theseus_protocol::agent_of(b).is_some()) {
                    let outcome = match r.data["approved"].as_bool() {
                        Some(false) => "declined",
                        _ => "approved",
                    };
                    ai.answer = Some((outcome.into(), Some(format!("by {by}"))));
                }
            }
            _ => {
                let by = s("by").map(|b| format!("by {b}"));
                ai.answer = Some(("approved".into(), by));
            }
        }
        d.acts.insert(corr.to_string());
        true
    }

    /// A wake fired: the execution's view says so (`woke`) until its turn
    /// rests, and the wake's own view ends done.
    fn take_fired(&mut self, r: &LedgerRow, d: &mut Dirty) {
        let (Some(exec), Some(wake)) =
            (r.data["execution_id"].as_str(), r.data["wake_id"].as_str())
        else {
            return;
        };
        let Some(ei) = self.execs.get_mut(exec) else {
            return;
        };
        let Some(w) = ei.wakes.iter().find(|w| w.id == wake).cloned() else {
            return;
        };
        ei.woke = Some(views::woke_of(&w));
        d.execs.insert(exec.to_string());
        if w.repeat.is_some() {
            // A series goes on under the same id: it has not ended.
            self.fired_series.insert(wake.to_string());
            return;
        }
        self.ended_wakes.insert(
            wake.to_string(),
            EndedWake {
                exec: exec.to_string(),
                wake: w,
                at_ms: r.at_unix_ms,
                fired: true,
            },
        );
        d.wakes.insert(wake.to_string());
    }

    fn exec_of(&self, session: &str) -> Option<&str> {
        self.by_session.get(session).map(String::as_str)
    }

    fn take_session(&mut self, s: &SessionRecord, d: &mut Dirty) {
        let Some(id) = self.by_session.get(&s.session_id).cloned() else {
            return;
        };
        if let Some(e) = self.execs.get_mut(&id) {
            let title = title_of(s, self.task_of_exec.get(&id));
            if e.title != title {
                e.title = title;
                d.execs.insert(id);
            }
        }
    }

    fn take_task(&mut self, core: &Core, (_, t): &(u64, TaskRecord), d: &mut Dirty) {
        if let Some(exec) = crate::task_graph::execution_of(t) {
            // A task with a session: its title and report are the execution's.
            self.task_of_exec.insert(exec.clone(), t.clone());
            if let Some(e) = self.execs.get_mut(&exec) {
                if !t.title.is_empty() && e.title != t.title {
                    e.title = t.title.clone();
                }
                d.execs.insert(exec.clone());
            }
            self.ensure_forecast(core, &exec);
            return;
        }
        // A plan step, and the steps that hang from it.
        self.steps.insert(t.id.clone(), t.clone());
        d.steps.insert(t.id.clone());
        for (id, s) in &self.steps {
            if s.parent.as_deref() == Some(&t.id) {
                d.steps.insert(id.clone());
            }
        }
        // The execution it counts toward gets a new `progress`.
        if let Some(exec) = self.owner_exec(t) {
            d.execs.insert(exec);
        }
    }

    /// The execution a plan step is under: its parent task's (a task with a
    /// session), else the session that made it.
    fn owner_exec(&self, t: &TaskRecord) -> Option<String> {
        let mut at = t;
        for _ in 0..16 {
            match at.parent.as_deref().and_then(|p| self.steps.get(p)) {
                Some(p) => at = p,
                None => break,
            }
        }
        if let Some(p) = at.parent.as_deref() {
            // The parent is a task with a session: `tsk_<tail>` is its id.
            let tail = p.strip_prefix(crate::task_graph::ID_PREFIX)?;
            return Some(format!("exe_{tail}"));
        }
        self.by_session.get(&at.origin.session).cloned()
    }

    fn take_action(&mut self, board: &Board, pos: u64, a: &Action, d: &mut Dirty) {
        let corr = a.correlation_id.clone();
        // A record the seed read already.
        match self.acts.get(&corr) {
            Some(ai) if pos <= ai.pos => return,
            None if board
                .entries
                .get(&a.execution_id)
                .is_some_and(|e| pos <= e.seed_pos) =>
            {
                return
            }
            _ => {}
        }
        d.execs.insert(a.execution_id.clone());
        if let (true, Some(at), Some(e)) = (
            a.state.is_settled() && a.tool != PROVIDER_TOOL,
            a.settled_at_ms,
            self.execs.get_mut(&a.execution_id),
        ) {
            e.last_settled_ms = e.last_settled_ms.max(at);
        }
        let known = self.acts.contains_key(&corr);
        let relevant = a.tool != PROVIDER_TOOL
            && (a.awaits_confirm() || is_job(a) || known || !a.state.is_settled());
        if !relevant {
            return;
        }
        self.track(a, pos);
        d.acts.insert(corr.clone());
        if !known {
            let rows: Vec<LedgerRow> = self.early.drain(..).collect();
            for r in rows {
                let again = r.data["correlation_id"].as_str() == Some(corr.as_str());
                if again {
                    self.act_row(&r, &corr, d);
                } else {
                    self.early.push_back(r);
                }
            }
        }
        let ai = self.acts.get_mut(&corr).expect("tracked above");
        // A job that settled: its time joins its program's, once.
        if is_job(&ai.a)
            && matches!(ai.a.state, ActionState::Succeeded | ActionState::Failed)
            && !ai.noted
        {
            ai.noted = true;
            if let (Some(key), Some(ms)) = (eta::key_of(&ai.argv), ai.duration_ms()) {
                self.eta.note(&key, ms);
            }
        }
        // A call that was never a job or a question leaves no trace.
        let ai = &self.acts[&corr];
        if ai.a.state.is_settled() && !ai.asked && !is_job(&ai.a) {
            self.acts.remove(&corr);
            if let Some(set) = self.acts_by_exec.get_mut(&a.execution_id) {
                set.remove(&corr);
            }
        }
    }

    /// Keep an action's inputs: the open ones, and an ended job or question
    /// until its view is built.
    fn track(&mut self, a: &Action, pos: u64) {
        let argv = a
            .proposal
            .as_ref()
            .and_then(|p| p.args["argv"].as_array())
            .map(|v| {
                v.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let ai = self
            .acts
            .entry(a.correlation_id.clone())
            .or_insert_with(|| ActIn {
                a: a.clone(),
                pos,
                argv: argv.clone(),
                asked: false,
                answer: None,
                detail: None,
                change: false,
                floor: false,
                expires_at_ms: None,
                noted: false,
            });
        ai.a = a.clone();
        ai.pos = pos;
        if ai.argv.is_empty() {
            ai.argv = argv;
        }
        if a.awaits_confirm() {
            ai.asked = true;
        }
        self.acts_by_exec
            .entry(a.execution_id.clone())
            .or_default()
            .insert(a.correlation_id.clone());
    }
}

/// A dispatched `proc.run` is a job: it runs on its own, past its call.
pub(super) fn is_job(a: &Action) -> bool {
    a.tool == "proc.run" && a.dispatched_at_ms.is_some()
}

impl ActIn {
    fn duration_ms(&self) -> Option<u64> {
        Some(
            self.a
                .settled_at_ms?
                .saturating_sub(self.a.dispatched_at_ms?),
        )
    }
}

/// A session's title for a view: its task's title, else its label, else the
/// first words of its first prompt.
fn title_of(s: &SessionRecord, task: Option<&TaskRecord>) -> String {
    if let Some(t) = task.filter(|t| !t.title.is_empty()) {
        return t.title.clone();
    }
    s.label
        .clone()
        .filter(|l| !l.is_empty())
        .or_else(|| s.title.clone().filter(|t| !t.is_empty()))
        .unwrap_or_default()
}

impl WorkBoard {
    /// Executions: the board's changed views, and the records of the
    /// executions whose views did not change in what a surface shows.
    fn take_execs(
        &mut self,
        core: &Core,
        board: &mut Board,
        taken: &Taken,
        changed: &[ExecutionView],
        d: &mut Dirty,
    ) {
        let mut seen: HashSet<String> = HashSet::new();
        for x in changed {
            seen.insert(x.execution_id.clone());
            let rec = taken.execs.iter().find(|(_, e)| e.id == x.execution_id);
            self.take_exec(core, rec.map(|(p, e)| (*p, e)), Some(x.clone()), d);
        }
        for (pos, e) in &taken.execs {
            if seen.contains(&e.id) || !self.execs.contains_key(&e.id) {
                continue;
            }
            let _ = board;
            self.take_exec(core, Some((*pos, e)), None, d);
        }
    }

    fn take_exec(
        &mut self,
        core: &Core,
        rec: Option<(u64, &Execution)>,
        x: Option<ExecutionView>,
        d: &mut Dirty,
    ) {
        let id = match (&x, rec) {
            (Some(x), _) => x.execution_id.clone(),
            (None, Some((_, e))) => e.id.clone(),
            _ => return,
        };
        if !self.execs.contains_key(&id) {
            // Not on the board yet: it needs its record, and its view.
            let (pos, e) = match rec {
                Some((p, e)) => (p, e.clone()),
                None => match core.kernel.execution_at(&id) {
                    Ok(Some(x)) => x,
                    _ => return,
                },
            };
            let Some(x) = x else {
                return;
            };
            let task =
                crate::task_graph::get(&core.store, &crate::task_graph::of_session(&e.session_id))
                    .ok()
                    .flatten();
            self.add_exec(core, pos, &e, x, task, d);
            return;
        }
        let ei = self.execs.get_mut(&id).expect("checked above");
        if let Some(x) = x {
            if ei.ran_from_ms == 0 && matches!(x.state.as_str(), "queued" | "running") {
                ei.ran_from_ms = x.at_ms;
            }
            ei.x = x;
        }
        if let Some((pos, e)) = rec {
            if pos > ei.exec_pos {
                ei.exec_pos = pos;
                ei.parent = e.parent.clone();
                ei.ended_reason = e.ended_reason.clone();
                let old: Vec<String> = ei.wakes.iter().map(|w| w.id.clone()).collect();
                let new: Vec<String> = e.wakes.iter().map(|w| w.id.clone()).collect();
                let gone: Vec<PendingWake> = ei
                    .wakes
                    .iter()
                    .filter(|w| !new.contains(&w.id))
                    .cloned()
                    .collect();
                ei.wakes = e.wakes.clone();
                for w in e.wakes.iter().filter(|w| !old.contains(&w.id)) {
                    d.wakes.insert(w.id.clone());
                }
                for w in e.wakes.iter().filter(|w| w.id.is_empty()) {
                    let _ = w;
                }
                for w in gone {
                    let wid = w.id.clone();
                    // Fired this frame, or cancelled: the rows said which.
                    self.ended_wakes
                        .entry(wid.clone())
                        .or_insert_with(|| EndedWake {
                            exec: id.clone(),
                            fired: self.fired_series.contains(&wid),
                            wake: w,
                            at_ms: e.updated_at_ms,
                        });
                    d.wakes.insert(wid);
                }
                // A wake that moved is the same wake with a new due time.
                for w in &e.wakes {
                    d.wakes.insert(w.id.clone());
                }
            }
        }
        self.ensure_forecast(core, &id);
        self.note_ended(core, &id);
        // A rested execution is no longer the wake's turn.
        let ei = self.execs.get_mut(&id).expect("checked above");
        if ei.woke.is_some() && !matches!(ei.x.state.as_str(), "queued" | "running") {
            ei.woke = None;
        }
        self.touch_routine(&id, d);
        d.execs.insert(id);
    }

    fn add_exec(
        &mut self,
        core: &Core,
        pos: u64,
        e: &Execution,
        x: ExecutionView,
        task: Option<TaskRecord>,
        d: &mut Dirty,
    ) {
        let rec: Option<SessionRecord> = core.store.get_session(&e.session_id).ok().flatten();
        let title = match (&rec, &task) {
            (Some(r), t) => title_of(r, t.as_ref()),
            (None, Some(t)) => t.title.clone(),
            _ => String::new(),
        };
        if let Some(t) = task {
            self.task_of_exec.insert(e.id.clone(), t);
        }
        self.by_session.insert(e.session_id.clone(), e.id.clone());
        for w in &e.wakes {
            d.wakes.insert(w.id.clone());
        }
        // A task asleep for a deferred start has not begun to run.
        let asleep = matches!(
            x.waiting_on,
            Some(theseus_protocol::WaitingOn::DueAt { at_ms })
                if at_ms < theseus_protocol::task_pause::PAUSED_AT_MS
        );
        let ran_from = if asleep { 0 } else { e.created_at_ms };
        self.execs.insert(
            e.id.clone(),
            ExecIn {
                x,
                exec_pos: pos,
                kind: match e.kind {
                    SessionKind::Conversation => WorkKind::Conversation,
                    SessionKind::Task => WorkKind::Task,
                },
                parent: e.parent.clone(),
                wakes: e.wakes.clone(),
                created_at_ms: e.created_at_ms,
                ended_reason: e.ended_reason.clone(),
                title,
                turn_started_ms: 0,
                last_settled_ms: 0,
                loop_index: 0,
                said: None,
                on: None,
                woke: None,
                forecast: None,
                loops_total: 0,
                ran_from_ms: ran_from,
                read: None,
                recorded: false,
                flagged: rec
                    .as_ref()
                    .and_then(|r| r.task.as_ref())
                    .is_some_and(|t| t.notify_done),
                expect: core
                    .store
                    .get_meta(&crate::task::expect_key(&e.session_id))
                    .ok()
                    .flatten(),
                paused: e.kind == SessionKind::Task && core.runner.task_paused(&e.session_id),
            },
        );
        self.ensure_forecast(core, &e.id);
        d.execs.insert(e.id.clone());
        self.touch_routine(&e.id, d);
        // What was waiting for it: its steps and its actions.
        let waiting: Vec<String> = self
            .steps
            .iter()
            .filter(|(_, s)| self.owner_exec(s).as_deref() == Some(&e.id))
            .map(|(id, _)| id.clone())
            .collect();
        d.steps.extend(waiting);
        if let Some(acts) = self.acts_by_exec.get(&e.id) {
            d.acts.extend(acts.iter().cloned());
        }
    }
}

mod seed;
mod settle;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_forecast;
#[cfg(test)]
mod tests_horizon;
#[cfg(test)]
mod tests_step;
#[cfg(test)]
mod tests_system;

pub(super) use seed::{read_system, read_warm, seed};

/// A question's end as a state.
fn answered(outcome: &str) -> WorkState {
    match outcome {
        "approved" => WorkState::Done,
        "declined" => WorkState::Cancelled,
        _ => WorkState::Expired,
    }
}

/// The policy's notice over two versions of a view.
fn notice_of(prev: Option<&WorkView>, next: &WorkView, rules: &Rules) -> Option<Notice> {
    policy(prev, next, rules, &hm)
}

/// Whether a record's step is held, not under way: a suspended record is a
/// cancelled or out-of-budget execution, recoverable, so its step sets the
/// view's `paused` as an owner-paused task does, and attention reads both the
/// same way ("paused", at rest; theseus-g00vv).
pub(super) fn step_paused(s: TaskState) -> bool {
    s == TaskState::Suspended
}

/// A task record's state, as the work board says it.
fn step_state(s: TaskState) -> WorkState {
    match s {
        TaskState::Proposed | TaskState::Accepted => WorkState::Queued,
        TaskState::InProgress => WorkState::Running,
        TaskState::Blocked | TaskState::WaitingHuman => WorkState::NeedsYou,
        TaskState::Suspended => WorkState::Waiting,
        TaskState::Done => WorkState::Done,
        TaskState::Failed => WorkState::Failed,
        TaskState::Abandoned => WorkState::Cancelled,
    }
}
