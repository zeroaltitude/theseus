//! The push (theseus-in3, design `stage2` §2): every execution as every
//! surface shows it. `view` builds an `ExecutionView` from the kernel's
//! records, with its `attention` from theseus-protocol's one function, so the
//! web UI, the CLI, the cockpit, and the TUI show the same words.
//!
//! The board (9b) keeps one view per execution, from the frames
//! `Kernel::commit` hands its observer, and sends `execution.changed` for
//! each frame that changes one. It is seeded on first need, so nothing is on
//! the start path, and the commit path pays one load until something watches.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use theseus_kernel::{Action, Committed, ExecState, Execution, Observer, Wake, BUDGET_TOOL};
use theseus_protocol::{
    attention, Attention, Event, ExecutionView, GateDecision, Level, Message, PendingConfirm,
    PushStatus, WaitingOn,
};
use theseus_store::kinds;
use tokio::sync::{mpsc, watch, OnceCell};

use crate::Core;

pub mod bench;
mod board;
mod hot;
mod last_line;
mod short;
mod work;

use board::{parent_session, seed_inputs, Board};
#[cfg(test)]
use board::{seed, seed_from, seed_recent};
use work::WorkBoard;
pub use work::{DigestRead as WorkDigestRead, Out as WorkOut, Scope as WorkScope};

/// A time on the daemon's clock, `14:00`: how a label says a due time. One
/// a day or more ahead names its weekday, `Fri 09:00`, so a deferred task's
/// start (theseus-rcgph) reads as a day, not as a time of day it may not be.
pub fn hm(unix_ms: u64) -> String {
    let l = crate::wake::local(unix_ms);
    match unix_ms.saturating_sub(theseus_protocol::now_unix_ms()) {
        d if d >= 20 * 3_600_000 => format!("{} {}", l.weekday(), l.hm()),
        _ => l.hm(),
    }
}

/// The kernel's wake, typed for the wire.
pub fn waiting_on(w: &Wake) -> WaitingOn {
    match w {
        Wake::DueAt { at_ms } => WaitingOn::DueAt { at_ms: *at_ms },
        Wake::Actions { correlation_ids } => WaitingOn::Actions {
            correlation_ids: correlation_ids.clone(),
        },
        Wake::Execution { execution_id } => WaitingOn::Execution {
            execution_id: execution_id.clone(),
        },
        Wake::Confirm { confirm_id } => WaitingOn::Confirm {
            confirm_id: confirm_id.clone(),
        },
        Wake::Input => WaitingOn::Input,
        Wake::Budget { correlation_id } => WaitingOn::Budget {
            correlation_id: correlation_id.clone(),
        },
    }
}

/// A question waiting for the operator, in brief: its action, and for a tool
/// call the gate's decision on its node, which holds the reason and the
/// floor. A question that expires (an `owner_ask`, an extension's ack, a
/// pack's promotion) says `confirm_ttl_ms` from its plan, as `confirm.list`
/// does; a budget question and a call's, which is held past that life and
/// never expires (theseus-v2dnc), hold until they are answered and say none.
pub fn pending_of(
    a: &Action,
    decision: Option<&GateDecision>,
    turn_id: Option<String>,
    confirm_ttl_ms: u64,
) -> PendingConfirm {
    let budget = a.tool == BUDGET_TOOL;
    let holds = budget || crate::rpc::held_past_life(a);
    PendingConfirm {
        correlation_id: a.correlation_id.clone(),
        tool: a
            .proposal
            .as_ref()
            .map(|p| p.tool.clone())
            .unwrap_or_else(|| a.tool.clone()),
        reason: decision.map(|d| d.reason.clone()).unwrap_or_default(),
        floor: decision.is_some_and(|d| d.floor),
        budget,
        expires_at_ms: if holds {
            0
        } else {
            crate::ask::expiry_of(a, confirm_ttl_ms)
        },
        turn_id: turn_id.or_else(|| budget.then(|| crate::rpc::budget_turn(a)).flatten()),
    }
}

/// An execution as every surface shows it, from its record and its
/// questions (a budget question first). `position` and `at_ms` are the
/// frame's it comes from; a list's view has the record's own time.
pub fn view(
    e: &Execution,
    pending: Vec<PendingConfirm>,
    parent_session_id: Option<String>,
    position: u64,
    at_ms: u64,
) -> ExecutionView {
    use theseus_kernel::micros_to_usd as usd;
    let mut v = ExecutionView {
        position,
        at_ms,
        execution_id: e.id.clone(),
        session_id: e.session_id.clone(),
        kind: e.kind,
        parent_session_id,
        state: e.state.as_str().into(),
        previous: None,
        waiting_on: (e.state == ExecState::Waiting)
            .then(|| e.wake.as_ref().map(waiting_on))
            .flatten(),
        pending,
        turns: e.turns,
        outstanding: e.outstanding.len() as u32,
        spent_usd: usd(e.budget.spent_micros),
        limit_usd: usd(e.budget.limit_micros),
        ended_reason: e.ended_reason.clone(),
        why: None,
        wake_at_ms: e.wakes.iter().map(|w| w.due_at_ms).min(),
        dismissed: e.dismissal().is_some(),
        retry_of: e.retry_of.clone(),
        attention: Attention {
            level: theseus_protocol::Level::Idle,
            label: String::new(),
            since_ms: at_ms,
        },
    };
    v.attention = attention(&v, &hm);
    v
}

// ---------------------------------------------------------------- the board

/// A frame the kernel committed, as the observer hands it over: its
/// EXECUTION, ACTION, and LEDGER records with their positions, and where its
/// nodes are, which the board reads only for a new question's reason.
#[derive(Clone)]
struct Frame {
    at_ms: u64,
    committed: Instant,
    position: u64,
    records: Vec<(u16, u64, Vec<u8>)>,
    nodes: Vec<u64>,
}

/// The push's observer: on the committing thread, under the execution's
/// lock, it keeps what the boards read and sends it on, never waiting. A
/// frame with no record a board reads costs one pass over its kinds, and a
/// ledger row nobody reads is not copied (`work::wanted`). The kernel
/// calls it for its frames, and the core's store for its own (a turn's rows,
/// a session's and a task's record).
fn observer(tx: mpsc::UnboundedSender<Option<Frame>>) -> Observer {
    Arc::new(move |c: Committed<'_>| {
        if !c.records.iter().any(|r| match r.kind {
            kinds::EXECUTION | kinds::ACTION | kinds::TASK | kinds::SESSION => true,
            kinds::LEDGER => work::wanted(&r.payload),
            _ => false,
        }) {
            return;
        }
        let mut records = Vec::new();
        let mut nodes = Vec::new();
        for (r, p) in c.records.iter().zip(c.positions) {
            match r.kind {
                kinds::EXECUTION | kinds::ACTION | kinds::TASK | kinds::SESSION => {
                    records.push((r.kind, *p, r.payload.clone()))
                }
                kinds::LEDGER if work::wanted(&r.payload) => {
                    records.push((r.kind, *p, r.payload.clone()))
                }
                kinds::NODE => nodes.push(*p),
                _ => {}
            }
        }
        let _ = tx.send(Some(Frame {
            at_ms: theseus_protocol::now_unix_ms(),
            committed: Instant::now(),
            position: c.positions.iter().copied().max().unwrap_or(0),
            records,
            nodes,
        }));
    })
}

/// The push (design `stage2` §2.7): the board, the seed, and the feed that
/// `session.wait` parks on.
pub struct Push {
    board: Mutex<Board>,
    /// The work board (theseus-hkx30). Taken after `board`, never before.
    work: Mutex<WorkBoard>,
    seed: OnceCell<()>,
    /// The board's position after each frame it applies.
    feed: watch::Sender<u64>,
    seed_us: AtomicU64,
    events: AtomicU64,
    /// Notifications every connection's backlog cap dropped (9c).
    pub lost: Arc<AtomicU64>,
    /// The backlog cap a new connection takes in a test:
    /// `outbound::BACKLOG_CAP`, which the test may lower, so that a few
    /// hundred events overflow it and a loaded machine runs the lag prove in
    /// seconds (theseus-0u6g). A release build has no such knob: every
    /// connection takes `BACKLOG_CAP` (design `stage2` §3.2).
    #[cfg(test)]
    pub(crate) backlog_cap: std::sync::atomic::AtomicUsize,
    /// `session.wait` calls parked now, and each connection's (9c).
    waits: Mutex<HashMap<String, usize>>,
    /// Ends the thread that applies frames: a stop sends `None`
    /// (theseus-hanu).
    applier: Mutex<Option<mpsc::UnboundedSender<Option<Frame>>>>,
    /// Wakes the last-line sampler when a job runs (`last_line.rs`).
    sampler: Arc<tokio::sync::Notify>,
}

impl Default for Push {
    fn default() -> Self {
        Self {
            board: Mutex::default(),
            work: Mutex::default(),
            seed: OnceCell::new(),
            feed: watch::Sender::new(0),
            seed_us: AtomicU64::new(0),
            events: AtomicU64::new(0),
            lost: Arc::default(),
            #[cfg(test)]
            backlog_cap: std::sync::atomic::AtomicUsize::new(crate::outbound::BACKLOG_CAP),
            waits: Mutex::default(),
            applier: Mutex::default(),
            sampler: Arc::default(),
        }
    }
}

/// The waits one connection may hold at once (design `stage2` §2.6).
pub const MAX_WAITS: usize = 64;

/// A parked `session.wait`'s place among its connection's: given back when
/// the wait ends, however it ends.
pub struct WaitSlot<'a> {
    push: &'a Push,
    conn: String,
}

impl Drop for WaitSlot<'_> {
    fn drop(&mut self) {
        let mut w = self.push.waits.lock().unwrap();
        if let Some(n) = w.get_mut(&self.conn) {
            *n -= 1;
            if *n == 0 {
                w.remove(&self.conn);
            }
        }
    }
}

impl Push {
    /// Seed the board, once; the first caller waits for it alone. The
    /// observer goes in first, so frames start to queue; then every
    /// execution and every action is read, in that order, on a blocking
    /// thread; then what queued meanwhile is applied, and a thread of the
    /// runtime's blocking pool applies the rest as they come. Not a task: a
    /// burst of requests whose handlers write the store holds every runtime
    /// worker, and a task behind them let the board fall seconds behind the
    /// commits (theseus-hanu). It holds the core weakly, so a stopped core
    /// is dropped and its kernel's observer with it, which ends the thread;
    /// the runtime's drop waits for it, so the store still closes before the
    /// process ends.
    pub async fn ensure(&self, core: &Arc<Core>) -> anyhow::Result<()> {
        self.seed
            .get_or_try_init(|| async {
                let t = Instant::now();
                let (tx, mut rx) = mpsc::unbounded_channel::<Option<Frame>>();
                *self.applier.lock().unwrap() = Some(tx.clone());
                // The kernel's frames, and what the core writes beside them
                // (a turn's rows, a session's and a task's record).
                let o = observer(tx);
                if !core.kernel.observe(o.clone()) || !core.store.observe(o) {
                    anyhow::bail!("the kernel's observer is installed already");
                }
                let c = core.clone();
                let (board, work) = tokio::task::spawn_blocking(move || {
                    let (board, execs, actions) = seed_inputs(&c, hot::RECENT)?;
                    let work = work::seed(&c, &board, &execs, &actions);
                    anyhow::Ok((board, work))
                })
                .await??;
                *self.board.lock().unwrap() = board;
                *self.work.lock().unwrap() = work;
                // Jobs running as the daemon started are sampled too.
                self.wake_sampler();
                while let Ok(Some(f)) = rx.try_recv() {
                    self.apply(core, f);
                }
                let weak = Arc::downgrade(core);
                tokio::task::spawn_blocking(move || {
                    while let Some(Some(f)) = rx.blocking_recv() {
                        let Some(core) = weak.upgrade() else { break };
                        core.push.apply(&core, f);
                    }
                });
                self.seed_us
                    .store(t.elapsed().as_micros() as u64, Ordering::Relaxed);
                let position = self.board.lock().unwrap().position;
                self.feed.send_replace(position);
                tracing::info!(
                    elapsed_us = t.elapsed().as_micros() as u64,
                    executions = self.board.lock().unwrap().entries.len(),
                    work = self.work.lock().unwrap().len(),
                    "push: board seeded"
                );
                Self::warm_work(core);
                Ok(())
            })
            .await
            .map(|_| ())
    }

    /// The forecast of each task open on the work board, once the finished
    /// tasks are read (theseus-zz752). Nothing if the board is not seeded:
    /// its own warm-up asks again when it is.
    pub fn forecast_open(&self, core: &Core) {
        if self.seeded() {
            let now = theseus_protocol::now_unix_ms();
            self.work.lock().unwrap().forecast_open(core, now);
        }
    }

    /// Whether the board is seeded.
    pub fn seeded(&self) -> bool {
        self.seed.initialized()
    }

    /// End the thread that applies frames, at a stop (theseus-hanu): the
    /// runtime's drop waits for every thread of its blocking pool, and this
    /// one would wait for the kernel's last frame until the core drops.
    pub fn stop(&self) {
        if let Some(tx) = self.applier.lock().unwrap().take() {
            let _ = tx.send(None);
        }
    }

    /// A place for one more wait on `conn`, or None at `MAX_WAITS`.
    pub fn wait_slot(&self, conn: &str) -> Option<WaitSlot<'_>> {
        let mut w = self.waits.lock().unwrap();
        let n = w.entry(conn.to_string()).or_default();
        if *n >= MAX_WAITS {
            return None;
        }
        *n += 1;
        Some(WaitSlot {
            push: self,
            conn: conn.to_string(),
        })
    }

    /// Apply one frame, and send what it changed: each view to its session's
    /// watchers and to every `executions.watch` subscriber, and the work
    /// board's frame to every `work.watch` subscriber.
    fn apply(&self, core: &Core, f: Frame) {
        let (changed, position, work) = {
            let mut b = self.board.lock().unwrap();
            let changed = b.apply(core, &f);
            b.position = b.position.max(f.position);
            let work = self.work.lock().unwrap().apply(core, &f, &mut b, &changed);
            (changed, b.position, work)
        };
        let n = changed.len() as u64;
        for v in changed {
            let session = v.session_id.clone();
            core.bus
                .publish(&session, &Message::from(Event::ExecutionChanged(v)), None);
        }
        if n > 0 {
            self.events.fetch_add(n, Ordering::Relaxed);
            core.telemetry().record_push(n, f.committed.elapsed());
        }
        if let Some(mut out) = work {
            core.stamp_data(&mut out.views);
            if out
                .views
                .iter()
                .any(|v| v.kind == theseus_protocol::work::WorkKind::Job && v.state.working())
            {
                self.wake_sampler();
            }
            core.bus.publish_work(&out, &core.tools.scrubber);
        }
        self.feed.send_replace(position);
    }

    /// The driver's tick: tell the questions that reached half their life
    /// that they still wait (the policy's rule 2). Nothing on an unseeded
    /// board, and nothing read until the earliest reminder is due.
    pub fn remind_questions(&self, core: &Core, now: u64) {
        if !self.seeded() {
            return;
        }
        let position = self.board.lock().unwrap().position;
        let out = self.work.lock().unwrap().remind(now, position);
        if let Some(out) = out {
            core.bus.publish_work(&out, &core.tools.scrubber);
        }
    }

    /// After the seed, off the serving path: the usual times of jobs, and
    /// the commands of the jobs running now (a thread of the blocking
    /// pool, holding the core weakly).
    fn warm_work(core: &Arc<Core>) {
        let weak = Arc::downgrade(core);
        tokio::task::spawn_blocking(move || {
            let Some(core) = weak.upgrade() else { return };
            let open = core.push.work.lock().unwrap().open_jobs();
            let (runs, argvs) = work::read_warm(&core, &open);
            core.push.work.lock().unwrap().warm(runs, argvs);
            // The daemon's own jobs of the last hour (theseus-jn17m).
            let now = theseus_protocol::now_unix_ms();
            let rows = work::read_system(&core, now);
            let out = core.push.work.lock().unwrap().warm_system(rows, now);
            if let Some(out) = out {
                core.bus.publish_work(&out, &core.tools.scrubber);
            }
            // The finished tasks a forecast is made from (theseus-zz752),
            // then the forecast of each task that is open now.
            core.tools
                .forecasts
                .loaded(crate::forecast::read_history(&core));
            core.push.work.lock().unwrap().forecast_open(&core, now);
        });
    }

    /// The work board as `work.watch` answers it (see `WorkBoard::snapshot`).
    pub fn work_snapshot(
        &self,
        limit: usize,
        scope: &WorkScope,
    ) -> (u64, Vec<theseus_protocol::work::WorkView>, u64) {
        self.work.lock().unwrap().snapshot(limit, scope)
    }

    /// Every view the work board holds, by id: a test's oracle for what a
    /// snapshot (roots and their open subtrees) does not list.
    #[cfg(test)]
    pub fn work_all(&self) -> std::collections::BTreeMap<String, theseus_protocol::work::WorkView> {
        self.work.lock().unwrap().all()
    }

    /// Take the thread groups (a thread's write or bind, or a person's read
    /// of their stats) and send what changed to every `work.watch` subscriber
    /// that reads private text. Memory only: no store read.
    pub fn set_threads(&self, core: &Core, groups: Vec<theseus_protocol::ThreadGroup>) {
        let out = self
            .work
            .lock()
            .unwrap()
            .set_threads(groups, theseus_protocol::now_unix_ms());
        if let Some(out) = out {
            core.bus.publish_work(&out, &core.tools.scrubber);
        }
    }

    /// The thread groups the work board holds.
    pub fn work_threads(&self) -> Vec<theseus_protocol::ThreadGroup> {
        self.work.lock().unwrap().threads()
    }

    /// `work.agenda`: every occurrence in a window, from the board's wakes in
    /// memory (see `WorkBoard::agenda`).
    pub fn work_agenda(
        &self,
        p: &theseus_protocol::work::horizon::AgendaParams,
        scope: &WorkScope,
        zone: &theseus_kernel::TimeZone,
    ) -> theseus_protocol::work::horizon::AgendaResult {
        self.work.lock().unwrap().work_agenda(p, scope, zone)
    }

    /// The questions open on the work board, the longest waiting first: from
    /// memory, never the store (`work.answer` resolves against them).
    pub fn open_questions(&self) -> Vec<theseus_protocol::work::Question> {
        self.work.lock().unwrap().open_questions()
    }

    /// The notices after `position`, and whether the ring holds every one
    /// since (`work-digest`).
    pub fn notices_since(&self, position: u64) -> (Vec<theseus_protocol::notices::Notice>, bool) {
        self.work.lock().unwrap().notices_since(position)
    }

    /// The board as `work.digest` reads it (`WorkBoard::digest_read`).
    pub fn work_digest_read(
        &self,
        since: u64,
        scope: &WorkScope,
        named: &std::collections::HashSet<String>,
        scrubber: &crate::scrub::Scrubber,
    ) -> WorkDigestRead {
        self.work
            .lock()
            .unwrap()
            .digest_read(since, scope, named, scrubber)
    }

    /// The board as `executions.watch` answers it: its position, every view
    /// that needs you or works (needs you first, the longest waiting first),
    /// then the `limit` most recently active of the rest, and how many it
    /// holds: a caller adds the cold ones (`Kernel::count_executions`).
    pub fn snapshot(&self, limit: usize) -> (u64, Vec<ExecutionView>, u64) {
        let b = self.board.lock().unwrap();
        let (mut active, mut rest): (Vec<ExecutionView>, Vec<ExecutionView>) = b
            .entries
            .values()
            .filter_map(|e| e.view.clone())
            .partition(|v| matches!(v.attention.level, Level::NeedsYou | Level::Working));
        let total = (active.len() + rest.len()) as u64;
        active.sort_by_key(|v| (v.attention.level.rank(), v.attention.since_ms));
        rest.sort_by_key(|v| std::cmp::Reverse(v.at_ms));
        rest.truncate(limit);
        active.extend(rest);
        (b.position, active, total)
    }

    /// The short line's facts (theseus-8kn12): the counts the board keeps as
    /// its views change, and the failures with their positions.
    pub fn status_short(&self) -> theseus_protocol::StatusShortResult {
        let mut r = {
            let b = self.board.lock().unwrap();
            b.short.result(b.position)
        };
        let (next, failed) = self.work.lock().unwrap().future_stats();
        r.next_due_ms = next;
        r.routines_failed = failed;
        r
    }

    /// A session's view, loading its execution when the board left it cold
    /// (theseus-id8d): `session.wait` reads it.
    pub fn view_or_load(&self, core: &Core, session_id: &str) -> Option<ExecutionView> {
        if let Some(v) = self.view_of_session(session_id) {
            return Some(v);
        }
        let rec: crate::session::SessionRecord = core.store.get_session(session_id).ok()??;
        let (pos, e) = core
            .kernel
            .execution_at(rec.execution_id.as_deref()?)
            .ok()??;
        let parent = parent_session(core, &e);
        let mut b = self.board.lock().unwrap();
        b.load(pos, &e, parent)
    }

    /// A session's view, if the board holds one.
    pub fn view_of_session(&self, session_id: &str) -> Option<ExecutionView> {
        let b = self.board.lock().unwrap();
        let id = b.by_session.get(session_id)?;
        b.entries.get(id)?.view.clone()
    }

    /// The feed: the board's position, after each frame.
    pub fn feed(&self) -> watch::Receiver<u64> {
        self.feed.subscribe()
    }

    /// For health: `watchers` is the bus's `executions.watch` count.
    pub fn status(&self, watchers: usize) -> PushStatus {
        let b = self.board.lock().unwrap();
        PushStatus {
            seeded: self.seeded(),
            seed_us: self.seed_us.load(Ordering::Relaxed),
            board: b.views,
            questions: b.questions,
            watchers: watchers as u64,
            work_watchers: 0,
            interactive: 0,
            events: self.events.load(Ordering::Relaxed),
            waiting: self.waits.lock().unwrap().values().sum::<usize>() as u64,
            lost: self.lost.load(Ordering::Relaxed),
            position: b.position,
            connections: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_kernel::{ActionState, Authority, Budget, RetryClass, SessionKind};

    mod seed;
    mod short;
    mod why;
    mod work;

    fn execution(state: ExecState, wake: Option<Wake>) -> Execution {
        Execution {
            id: "exe_0000a1b2c3".into(),
            schema: 2,
            session_id: "ses_0000d4e5f6".into(),
            kind: SessionKind::Task,
            state,
            authority: Authority::default(),
            budget: Budget::new(2_000_000),
            wake,
            outstanding: vec![],
            queued_results: vec![],
            parent: Some("exe_parent".into()),
            reports_to: None,
            reports: vec![],
            wakes: vec![],
            wake_parent: false,
            report_wakes: vec![],
            helper: None,
            stopped: None,
            turns: 3,
            interrupted: 0,
            resume_pending: false,
            cancel: None,
            ended_reason: None,
            dismissed: None,
            retry_of: None,
            created_at_ms: 1,
            updated_at_ms: 2,
        }
    }

    fn action(tool: &str) -> Action {
        Action {
            correlation_id: format!("cor_{tool}"),
            schema: 2,
            execution_id: "exe_0000a1b2c3".into(),
            session_id: "ses_0000d4e5f6".into(),
            tool: tool.into(),
            args_digest: String::new(),
            proposal: None,
            resource: None,
            retry_class: RetryClass::NonRepeatable,
            state: ActionState::Planned,
            deadline_at_ms: 0,
            planned_at_ms: 1_000,
            authorized_at_ms: None,
            dispatched_at_ms: None,
            settled_at_ms: None,
            external_op_id: None,
            result_ref: None,
            confirm: None,
            cancel: None,
            verdict: None,
            reservation_id: None,
            reserved_micros: 0,
            resolution: None,
            completions_seen: 0,
            detail: None,
            parent: None,
        }
    }

    /// A task parked on a tool call's question: its view says so, with the
    /// gate's reason, the expiry `confirm.list` gives (none for a call's
    /// question, which is held, never expired; an `owner_ask`'s is its plan's
    /// time and the TTL), and its parent.
    #[test]
    fn a_task_parked_on_a_question_needs_you_with_the_gates_reason() {
        let e = execution(
            ExecState::Waiting,
            Some(Wake::Confirm {
                confirm_id: "cor_proc.run".into(),
            }),
        );
        let decision = GateDecision {
            reason: "run cargo test".into(),
            floor: true,
            ..Default::default()
        };
        let p = pending_of(&action("proc.run"), Some(&decision), None, 300_000);
        assert_eq!(p.expires_at_ms, 0, "a held call says no deadline");
        let ask = pending_of(&action(crate::ask::OWNER_ASK), None, None, 300_000);
        assert_eq!(ask.expires_at_ms, 301_000, "an ask still expires");
        let v = view(&e, vec![p], Some("ses_parent".into()), 48, 9);
        assert_eq!(v.attention.level, theseus_protocol::Level::NeedsYou);
        assert_eq!(
            v.attention.label,
            "confirm proc.run: run cargo test · floor"
        );
        assert_eq!(v.attention.since_ms, 9);
        assert_eq!(v.parent_session_id.as_deref(), Some("ses_parent"));
        assert_eq!(
            v.waiting_on,
            Some(WaitingOn::Confirm {
                confirm_id: "cor_proc.run".into()
            })
        );
        assert_eq!((v.spent_usd, v.limit_usd), (0.0, 2.0));
    }

    /// A budget question holds until it is answered; a running execution
    /// keeps no `waiting_on`, though its record may still hold a wake.
    #[test]
    fn a_budget_question_has_no_expiry_and_only_waiting_has_a_wake() {
        let q = pending_of(&action(BUDGET_TOOL), None, None, 300_000);
        assert!(q.budget && q.expires_at_ms == 0 && q.reason.is_empty());
        let v = view(
            &execution(ExecState::Running, Some(Wake::Input)),
            vec![],
            None,
            1,
            1,
        );
        assert_eq!(v.waiting_on, None);
        assert_eq!(v.attention.label, "turn 3");
    }
}
