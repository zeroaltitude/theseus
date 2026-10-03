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
use theseus_store::{kinds, Store as _};
use tokio::sync::{mpsc, watch, OnceCell};

use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::Core;

/// A time of day on the daemon's clock, `14:00`: how a label says a due time.
pub fn hm(unix_ms: u64) -> String {
    crate::wake::local(unix_ms).hm()
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
/// floor. A tool call's question holds `confirm_ttl_ms` from its plan, as
/// `confirm.list` says; a budget question holds until it is answered.
pub fn pending_of(
    a: &Action,
    decision: Option<&GateDecision>,
    confirm_ttl_ms: u64,
) -> PendingConfirm {
    let budget = a.tool == BUDGET_TOOL;
    // A budget question and a held post's hold until answered (M4 19c); an
    // L1 job's credential request, until its job's deadline (M4 18d).
    let holds = budget || a.tool == theseus_protocol::HELD_POST_TOOL;
    let cred = a.tool == theseus_protocol::CRED_TOOL;
    PendingConfirm {
        correlation_id: a.correlation_id.clone(),
        tool: a
            .proposal
            .as_ref()
            .map(|p| p.tool.clone())
            .unwrap_or_else(|| a.tool.clone()),
        reason: match cred {
            true => crate::cred::reason_of(a),
            false => decision.map(|d| d.reason.clone()).unwrap_or_default(),
        },
        floor: decision.is_some_and(|d| d.floor),
        budget,
        expires_at_ms: match (holds, cred) {
            (true, _) => 0,
            (_, true) => a.deadline_at_ms,
            _ => a.planned_at_ms + confirm_ttl_ms,
        },
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
struct Frame {
    at_ms: u64,
    committed: Instant,
    position: u64,
    records: Vec<(u16, u64, Vec<u8>)>,
    nodes: Vec<u64>,
}

/// The push's observer: on the committing thread, under the execution's
/// lock, it keeps what the board reads and sends it on, never waiting. A
/// frame with no execution or action record costs one pass over its kinds.
fn observer(tx: mpsc::UnboundedSender<Option<Frame>>) -> Observer {
    Arc::new(move |c: Committed<'_>| {
        if !c
            .records
            .iter()
            .any(|r| r.kind == kinds::EXECUTION || r.kind == kinds::ACTION)
        {
            return;
        }
        let mut records = Vec::new();
        let mut nodes = Vec::new();
        for (r, p) in c.records.iter().zip(c.positions) {
            match r.kind {
                kinds::EXECUTION | kinds::ACTION | kinds::LEDGER => {
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

/// One execution on the board.
#[derive(Default)]
struct Entry {
    /// Its view as last sent, or as seeded. None while only its questions
    /// are known: a seed that read them before its record.
    view: Option<ExecutionView>,
    /// Its questions, a budget question first: the view's `pending`.
    pending: Vec<PendingConfirm>,
    /// Its execution record's position: a record at or below it is older.
    exec_pos: u64,
    /// The highest position of its records that the seed read, frozen: an
    /// action record at or below it is on the board already.
    seed_pos: u64,
}

#[derive(Default)]
struct Board {
    entries: HashMap<String, Entry>,
    /// The last frame applied.
    position: u64,
}

/// The push (design `stage2` §2.7): the board, the seed, and the feed that
/// `session.wait` parks on.
pub struct Push {
    board: Mutex<Board>,
    seed: OnceCell<()>,
    /// The board's position after each frame it applies.
    feed: watch::Sender<u64>,
    seed_us: AtomicU64,
    events: AtomicU64,
    /// Notifications every connection's backlog cap dropped (9c).
    pub lost: Arc<AtomicU64>,
    /// `session.wait` calls parked now, and each connection's (9c).
    waits: Mutex<HashMap<String, usize>>,
    /// Ends the thread that applies frames: a stop sends `None`
    /// (theseus-hanu).
    applier: Mutex<Option<mpsc::UnboundedSender<Option<Frame>>>>,
}

impl Default for Push {
    fn default() -> Self {
        Self {
            board: Mutex::default(),
            seed: OnceCell::new(),
            feed: watch::Sender::new(0),
            seed_us: AtomicU64::new(0),
            events: AtomicU64::new(0),
            lost: Arc::default(),
            waits: Mutex::default(),
            applier: Mutex::default(),
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
                if !core.kernel.observe(observer(tx)) {
                    anyhow::bail!("the kernel's observer is installed already");
                }
                let c = core.clone();
                let board = tokio::task::spawn_blocking(move || seed(&c)).await??;
                *self.board.lock().unwrap() = board;
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
                    "push: board seeded"
                );
                Ok(())
            })
            .await
            .map(|_| ())
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
    /// watchers and to every `executions.watch` subscriber.
    fn apply(&self, core: &Core, f: Frame) {
        let (changed, position) = {
            let mut b = self.board.lock().unwrap();
            let changed = b.apply(core, &f);
            b.position = b.position.max(f.position);
            (changed, b.position)
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
        self.feed.send_replace(position);
    }

    /// The board as `executions.watch` answers it: its position, every view
    /// that needs you or works (needs you first, the longest waiting first),
    /// then the `limit` most recently active of the rest, and how many it
    /// holds.
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

    /// A session's view, if the board holds one: `session.wait` reads it.
    pub fn view_of_session(&self, session_id: &str) -> Option<ExecutionView> {
        self.board
            .lock()
            .unwrap()
            .entries
            .values()
            .filter_map(|e| e.view.as_ref())
            .find(|v| v.session_id == session_id)
            .cloned()
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
            board: b.entries.values().filter(|e| e.view.is_some()).count() as u64,
            questions: b.entries.values().map(|e| e.pending.len() as u64).sum(),
            watchers: watchers as u64,
            events: self.events.load(Ordering::Relaxed),
            waiting: self.waits.lock().unwrap().values().sum::<usize>() as u64,
            lost: self.lost.load(Ordering::Relaxed),
            position: b.position,
        }
    }
}

/// The seed: every execution, then every action, with their positions. The
/// order matters: a frame that lands between the two reads has its
/// execution record applied from the queue (its position is above the one
/// read) and its actions skipped (the second read saw them), so the board
/// matches the store either way.
fn seed(core: &Core) -> anyhow::Result<Board> {
    let execs = core.kernel.executions_at()?;
    let actions = core.kernel.actions_at()?;
    let mut b = Board::default();
    let session_of: HashMap<&str, &str> = execs
        .iter()
        .map(|(_, e)| (e.id.as_str(), e.session_id.as_str()))
        .collect();
    for (p, e) in &execs {
        let entry = b.entries.entry(e.id.clone()).or_default();
        entry.exec_pos = *p;
        entry.seed_pos = *p;
        b.position = b.position.max(*p);
    }
    let mut waiting = Vec::new();
    for (p, a) in actions {
        let entry = b.entries.entry(a.execution_id.clone()).or_default();
        entry.seed_pos = entry.seed_pos.max(p);
        b.position = b.position.max(p);
        if a.awaits_confirm() {
            waiting.push(a);
        }
    }
    // As `Kernel::pending_confirms` orders them: a budget question first.
    waiting.sort_by_key(|a| {
        (
            a.tool != BUDGET_TOOL,
            a.planned_at_ms,
            a.correlation_id.clone(),
        )
    });
    let mut pending = core.pending_by_execution(&waiting, None);
    for (_, e) in &execs {
        let entry = b.entries.get_mut(&e.id).expect("seeded above");
        entry.pending = pending.remove(&e.id).unwrap_or_default();
        let parent = e
            .parent
            .as_deref()
            .and_then(|x| session_of.get(x))
            .map(|s| s.to_string());
        entry.view = Some(view(
            e,
            entry.pending.clone(),
            parent,
            entry.seed_pos,
            e.updated_at_ms,
        ));
    }
    // The questions of an execution whose record came after the first read:
    // kept until that record comes off the queue.
    for (id, ps) in pending {
        b.entries.entry(id).or_default().pending = ps;
    }
    Ok(b)
}

impl Board {
    /// Apply a frame: each record newer than what the board holds of its
    /// entity, then each touched execution's view, built again. Returns the
    /// views that changed in what a surface shows, with `previous` set.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn apply(&mut self, core: &Core, f: &Frame) -> Vec<ExecutionView> {
        let mut execs: Vec<Execution> = Vec::new();
        let mut touched: Vec<String> = Vec::new();
        let mut why: HashMap<String, String> = HashMap::new();
        let mut asks: Vec<Action> = Vec::new();
        let mut actions: Vec<Action> = Vec::new();
        for (kind, pos, payload) in &f.records {
            match *kind {
                kinds::EXECUTION => {
                    let Ok(e) = core.kernel.decode_execution(payload) else {
                        continue;
                    };
                    let entry = self.entries.entry(e.id.clone()).or_default();
                    if *pos <= entry.exec_pos {
                        continue;
                    }
                    entry.exec_pos = *pos;
                    touch(&mut touched, &e.id);
                    execs.retain(|x| x.id != e.id);
                    execs.push(e);
                }
                kinds::ACTION => {
                    let Ok(a) = serde_json::from_slice::<Action>(payload) else {
                        continue;
                    };
                    let entry = self.entries.entry(a.execution_id.clone()).or_default();
                    if *pos <= entry.seed_pos {
                        continue;
                    }
                    // A frame may hold one action several times (planned,
                    // authorized, dispatched: `plan_and_dispatch`): its last
                    // record is what it is.
                    actions.retain(|x: &Action| x.correlation_id != a.correlation_id);
                    actions.push(a);
                }
                kinds::LEDGER => {
                    let Ok(row) = serde_json::from_slice::<LedgerRow>(payload) else {
                        continue;
                    };
                    let id = row.data["execution_id"].as_str();
                    if row.kind == "execution.queued" {
                        if let (Some(id), Some(w)) = (id, row.data["why"].as_str()) {
                            why.insert(id.to_string(), w.to_string());
                        }
                    } else if row.kind.starts_with("action.")
                        && row.data["execution_state"] == "queued"
                    {
                        // A job's result queued it: its row is the action's.
                        if let Some(id) = id {
                            why.entry(id.to_string()).or_insert_with(|| "result".into());
                        }
                    }
                }
                _ => {}
            }
        }
        for a in actions {
            let entry = self.entries.entry(a.execution_id.clone()).or_default();
            let at = entry
                .pending
                .iter()
                .position(|p| p.correlation_id == a.correlation_id);
            match (a.awaits_confirm(), at) {
                (true, None) => asks.push(a),
                (false, Some(i)) => {
                    entry.pending.remove(i);
                    touch(&mut touched, &a.execution_id);
                }
                _ => {}
            }
        }
        if !asks.is_empty() {
            let nodes = read_nodes(core, &f.nodes);
            let ttl = core.kernel.config().confirm_ttl_ms;
            for a in asks {
                let decision = nodes.iter().find_map(|n| decision_on(n, &a.correlation_id));
                let p = pending_of(&a, decision.as_ref(), ttl);
                let entry = self.entries.entry(a.execution_id.clone()).or_default();
                // A budget question first, as `Kernel::pending_confirms` orders them.
                let at = if p.budget { 0 } else { entry.pending.len() };
                entry.pending.insert(at, p);
                touch(&mut touched, &a.execution_id);
            }
        }
        let mut out = Vec::new();
        for id in touched {
            let latest = execs.iter().find(|e| e.id == id);
            let parent = latest
                .and_then(|e| e.parent.as_deref())
                .and_then(|p| self.entries.get(p))
                .and_then(|p| p.view.as_ref())
                .map(|v| v.session_id.clone());
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            let mut new = match (latest, &entry.view) {
                (Some(e), old) => {
                    let mut v = view(
                        e,
                        entry.pending.clone(),
                        parent.or_else(|| old.as_ref().and_then(|o| o.parent_session_id.clone())),
                        f.position,
                        f.at_ms,
                    );
                    v.why = why.remove(&id);
                    v
                }
                (None, Some(old)) => {
                    let mut v = old.clone();
                    v.pending = entry.pending.clone();
                    v.position = f.position;
                    v.at_ms = f.at_ms;
                    v
                }
                // A question of an execution not seen yet: kept for its record.
                (None, None) => continue,
            };
            if new.state != "queued" {
                new.why = None;
            }
            new.attention = attention(&new, &hm);
            match &entry.view {
                Some(old) if same(old, &new) => continue,
                Some(old) => {
                    new.previous = Some(old.state.clone());
                    if old.attention.level == new.attention.level {
                        new.attention.since_ms = old.attention.since_ms;
                    }
                }
                None => {}
            }
            entry.view = Some(new.clone());
            out.push(new);
        }
        out
    }
}

fn touch(touched: &mut Vec<String>, id: &str) {
    if !touched.iter().any(|t| t == id) {
        touched.push(id.to_string());
    }
}

/// Whether two views show the same: the state, what it waits on, its
/// questions, its turns, how it ended, its attention, and its spend and limit
/// to the cent. A frame that changes none of these sends nothing.
fn same(a: &ExecutionView, b: &ExecutionView) -> bool {
    let cents = |x: f64| (x * 100.0).round() as i64;
    a.state == b.state
        && a.waiting_on == b.waiting_on
        && a.pending == b.pending
        && a.turns == b.turns
        && a.ended_reason == b.ended_reason
        && a.attention.level == b.attention.level
        && a.attention.label == b.attention.label
        && cents(a.spent_usd) == cents(b.spent_usd)
        && cents(a.limit_usd) == cents(b.limit_usd)
        && a.wake_at_ms == b.wake_at_ms
        && a.parent_session_id == b.parent_session_id
        && a.why == b.why
}

/// The frame's nodes, read from the store by position: only for a new
/// question, whose reason and floor its tool-call node holds.
fn read_nodes(core: &Core, positions: &[u64]) -> Vec<Node> {
    positions
        .iter()
        .filter_map(|p| core.store.inner().get(*p).ok().flatten())
        .filter_map(|r| r.decode::<Node>().ok())
        .collect()
}

/// The gate's decision on the tool call `correlation_id` names, if `n` is
/// that call's node.
fn decision_on(n: &Node, correlation_id: &str) -> Option<GateDecision> {
    match &n.body {
        Body::ToolCall {
            correlation_id: Some(c),
            gate,
            ..
        } if c == correlation_id => gate.as_ref().and_then(|g| g.decision.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_kernel::{ActionState, Authority, Budget, RetryClass, SessionKind};

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
            stopped: None,
            turns: 3,
            interrupted: 0,
            resume_pending: false,
            cancel: None,
            ended_reason: None,
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
    /// gate's reason, the expiry `confirm.list` gives, and its parent.
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
        let p = pending_of(&action("proc.run"), Some(&decision), 300_000);
        assert_eq!(p.expires_at_ms, 301_000);
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
        let q = pending_of(&action(BUDGET_TOOL), None, 300_000);
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
