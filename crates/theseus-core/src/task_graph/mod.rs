//! The task graph (M7 step 39a, theseus-ext.6; spec §3.5, M7 §2.4): every
//! task is a record of the store's `TASK` kind, the latest of its id the task
//! as it is now.
//!
//! - **One record kind, two uses.** A task with a session is DD7's
//!   delegation (`task.create { brief }`): its record is written in
//!   `open_task`'s frame, with the session. A task without one is an item of
//!   a plan (`task.create` without `brief`, `task.split`). Old stores' task
//!   sessions have no record and still list from their executions.
//! - **CAS.** Every edit names the `version` it read; one that names another
//!   is refused with the record as it is now (`task.stale_refused`). The
//!   check and the write are under the task's lock (`Store::lock_task`),
//!   taken after a session's and before an execution's.
//! - **Three layers** (`tools.rs`): the plan (title, children, deps, owner)
//!   applies; evidence only appends; the objective and acceptance, and
//!   abandoning, are the operator's, and a change to them waits at the gate
//!   as the floor's calls do, until the operator answers it.
//! - **A session's task follows its execution when read** (`state_now`):
//!   running is `in_progress`, a card waiting `waiting_human`, a budget
//!   question or a cancel `suspended`. Its record is written at its own
//!   changes only: its report closes it `done` with the report as its
//!   evidence, and a failure `failed`, in the frame that ends the task.
//! - **The view** (`view.rs`): the graph a turn's scope holds, in the
//!   request's tail.

pub mod tools;
pub mod view;

use anyhow::Result;
use theseus_kernel::{ExecState, Execution, Wake};
use theseus_store::{kinds, NewRecord, Store as _};

pub use theseus_protocol::tasks::{TaskEvidence, TaskOrigin, TaskProposal, TaskRecord, TaskState};

use crate::store::Store;

/// Every task id begins so.
pub const ID_PREFIX: &str = "tsk_";
/// The owner of a task nobody else took: the agent.
pub const AGENT: &str = "agent";

/// A task's id from the id it was made by (a call's correlation id, or a
/// task session's id): `tsk_` and what follows the other's prefix, so a
/// task session's record is found from its session.
pub fn id_from(key: &str) -> String {
    let tail = key.split_once('_').map_or(key, |(_, t)| t);
    format!("{ID_PREFIX}{tail}")
}

/// The record id of a task session's task.
pub fn of_session(session_id: &str) -> String {
    id_from(session_id)
}

/// The execution of a task with a session (DD7's ids share their tail).
pub fn execution_of(rec: &TaskRecord) -> Option<String> {
    let s = rec.session.as_deref()?;
    let tail = s.split_once('_').map_or(s, |(_, t)| t);
    Some(format!("exe_{tail}"))
}

/// The record as one frame's record.
pub fn record(t: &TaskRecord) -> Result<NewRecord> {
    NewRecord::json(kinds::TASK, Some(&t.id), t)
}

/// A task as it is now, or None.
pub fn get(store: &Store, id: &str) -> Result<Option<TaskRecord>> {
    match store.inner().latest_by_key(kinds::TASK, id)? {
        Some(r) => Ok(Some(r.decode()?)),
        None => Ok(None),
    }
}

/// Every task, oldest first.
pub fn all(store: &Store) -> Result<Vec<TaskRecord>> {
    let mut v: Vec<TaskRecord> = store
        .inner()
        .latest_of_kind(kinds::TASK)?
        .iter()
        .map(|r| r.decode())
        .collect::<Result<_>>()?;
    v.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
    Ok(v)
}

/// A task's state as it reads now: a closed record's own, or for a task
/// with a session, its execution's running state (the default 39a takes:
/// derived when read, never written at each transition).
pub fn state_now(rec: &TaskRecord, exec: Option<&Execution>) -> TaskState {
    if rec.state.is_closed() || rec.session.is_none() {
        return rec.state;
    }
    let Some(e) = exec else { return rec.state };
    match e.state {
        ExecState::Queued | ExecState::Running => TaskState::InProgress,
        ExecState::Waiting => match &e.wake {
            Some(Wake::Confirm { .. }) => TaskState::WaitingHuman,
            Some(Wake::Budget { .. }) => TaskState::Suspended,
            _ => TaskState::InProgress,
        },
        ExecState::Blocked => TaskState::Blocked,
        ExecState::Cancelled | ExecState::BudgetExhausted => TaskState::Suspended,
        // An end the report did not close (a record from before it).
        ExecState::Complete => TaskState::Done,
        ExecState::Failed => TaskState::Failed,
    }
}

/// The record as surfaces show it: its state as it reads now.
pub fn shown(kernel: &theseus_kernel::Kernel, mut rec: TaskRecord) -> TaskRecord {
    let exec = execution_of(&rec).and_then(|id| kernel.execution(&id).ok().flatten());
    rec.state = state_now(&rec, exec.as_ref());
    rec
}

/// Every task as surfaces show it.
pub fn all_shown(store: &Store, kernel: &theseus_kernel::Kernel) -> Result<Vec<TaskRecord>> {
    Ok(all(store)?.into_iter().map(|r| shown(kernel, r)).collect())
}

/// The task a model or a person means by `name`: its id, or the end of it
/// (at least four characters), when exactly one task matches.
pub fn resolve<'a>(tasks: &'a [TaskRecord], name: &str) -> Result<&'a TaskRecord, String> {
    let n = name.trim().trim_start_matches('…');
    if let Some(t) = tasks.iter().find(|t| t.id == n) {
        return Ok(t);
    }
    if n.len() < 4 {
        return Err(format!(
            "`{name}` is too short to name a task: give at least four characters of its id"
        ));
    }
    let ends: Vec<&TaskRecord> = tasks.iter().filter(|t| t.id.ends_with(n)).collect();
    match ends.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("no task is named `{name}`")),
        many => Err(format!(
            "`{name}` names {} tasks ({}): give more of its id",
            many.len(),
            many.iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// An edit that named a version the task has moved past.
#[derive(Debug, Clone)]
pub struct Stale {
    pub named: u64,
    pub now: Box<TaskRecord>,
}

impl Stale {
    /// What the model reads: the record as it is now, so it reads it again
    /// and decides.
    pub fn message(&self) -> String {
        format!(
            "Refused: task {} changed since you read it: v{} → v{}. It is now: {}\nRead it \
             again (the task graph below, or task.update with the new version) and decide.",
            self.now.id,
            self.named,
            self.now.version,
            line(&self.now)
        )
    }
}

/// Compare and swap's compare: the version named is the one the task has.
pub fn check(rec: &TaskRecord, version: u64) -> Result<(), Stale> {
    if rec.version == version {
        Ok(())
    } else {
        Err(Stale {
            named: version,
            now: Box::new(rec.clone()),
        })
    }
}

/// A task's children, oldest first.
pub fn children<'a>(tasks: &'a [TaskRecord], id: &str) -> Vec<&'a TaskRecord> {
    tasks
        .iter()
        .filter(|t| t.parent.as_deref() == Some(id))
        .collect()
}

/// A task and every task under it, depth first.
pub fn subtree<'a>(tasks: &'a [TaskRecord], id: &str) -> Vec<&'a TaskRecord> {
    let mut out = Vec::new();
    let mut stack: Vec<&str> = vec![id];
    let mut seen = std::collections::HashSet::new();
    while let Some(at) = stack.pop() {
        if !seen.insert(at.to_string()) {
            continue;
        }
        if let Some(t) = tasks.iter().find(|t| t.id == at) {
            out.push(t);
        }
        for c in children(tasks, at).into_iter().rev() {
            stack.push(&c.id);
        }
    }
    out
}

/// The tasks a session's turns see (§2.4): a conversation, the tasks it
/// started and everything under them; a task's own session, its subtree and
/// its parent's line. Depth first, roots oldest first.
pub fn scope<'a>(tasks: &'a [TaskRecord], session_id: &str) -> Vec<&'a TaskRecord> {
    let own = of_session(session_id);
    if let Some(me) = tasks.iter().find(|t| t.id == own) {
        let mut out: Vec<&TaskRecord> = Vec::new();
        if let Some(p) = me
            .parent
            .as_deref()
            .and_then(|p| tasks.iter().find(|t| t.id == p))
        {
            out.push(p);
        }
        out.extend(subtree(tasks, &me.id));
        return out;
    }
    let mut out = Vec::new();
    for root in tasks.iter().filter(|t| {
        t.origin.session == session_id
            && t.parent
                .as_deref()
                .and_then(|p| tasks.iter().find(|x| x.id == p))
                .is_none_or(|p| p.origin.session != session_id)
    }) {
        for t in subtree(tasks, &root.id) {
            if !out.iter().any(|o: &&TaskRecord| o.id == t.id) {
                out.push(t);
            }
        }
    }
    out
}

/// How deep a task sits under the scope's roots, for indenting.
pub fn depth(tasks: &[&TaskRecord], t: &TaskRecord) -> usize {
    let mut d = 0;
    let mut at = t.parent.as_deref();
    while let Some(p) = at {
        match tasks.iter().find(|x| x.id == p) {
            Some(x) if d < 16 => {
                d += 1;
                at = x.parent.as_deref();
            }
            _ => break,
        }
    }
    d
}

/// How the view marks the owner's tasks, whose layer-1 changes wait
/// (theseus-ext.10).
pub const OWNERS_MARK: &str = "the operator's objective";

/// One task on one line: id, title, state, owner, the owner's mark, deps, a
/// line of acceptance, and version.
pub fn line(t: &TaskRecord) -> String {
    let mut out = format!(
        "{} \"{}\" [{}] owner {}",
        t.id,
        t.title,
        t.state.as_str(),
        t.owner
    );
    if t.is_owners() {
        out.push_str(&format!(", {OWNERS_MARK}"));
    }
    if !t.deps.is_empty() {
        out.push_str(&format!(", deps {}", t.deps.join(" ")));
    }
    if let Some(a) = t.acceptance.first() {
        let a: String = a
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .take(120)
            .collect();
        let more = match t.acceptance.len() {
            1 => String::new(),
            n => format!(" (+{} more)", n - 1),
        };
        out.push_str(&format!(", accept: {a}{more}"));
    }
    if t.proposal.is_some() {
        out.push_str(", a change waits for the operator");
    }
    out.push_str(&format!(", v{}", t.version));
    out
}

/// How many tasks are open.
pub fn open_count(tasks: &[TaskRecord]) -> u32 {
    tasks.iter().filter(|t| !t.state.is_closed()).count() as u32
}

/// What a session's principal is: its execution's authority, else the
/// operator.
pub fn principal_of(kernel: &theseus_kernel::Kernel, execution_id: &str) -> String {
    kernel
        .execution(execution_id)
        .ok()
        .flatten()
        .map_or_else(|| "operator".into(), |e| e.authority.principal)
}

/// The record a new task session's frame writes (DD7's `task.create` with a
/// brief): its title, its objective and acceptance (the arrangement's pieces
/// when given), its origin, and its session.
pub struct NewTask<'a> {
    pub id: String,
    pub title: &'a str,
    pub objective: String,
    pub acceptance: Vec<String>,
    pub parent: Option<String>,
    pub deps: Vec<String>,
    pub session: Option<String>,
    pub origin: TaskOrigin,
    pub state: TaskState,
}

impl NewTask<'_> {
    pub fn build(self, now_ms: u64) -> TaskRecord {
        TaskRecord {
            id: self.id,
            version: 1,
            title: self.title.to_string(),
            objective: self.objective,
            acceptance: self.acceptance,
            state: self.state,
            parent: self.parent,
            deps: self.deps,
            owner: AGENT.into(),
            session: self.session,
            origin: self.origin,
            evidence: vec![],
            proposal: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }
}

/// A task session's close by its report (§2.4, M7 Q13): in the frame that
/// ends it, its record, closed `done` with the report as its evidence, or
/// `failed`. A cancel closes nothing: its task reads `suspended`. None for
/// a session with no record (a store from before 39a) or one closed already.
pub fn closed_by_report(
    store: &Store,
    e: &Execution,
    report_node: Option<&str>,
) -> Result<Option<TaskRecord>> {
    let outcome = match e.state {
        ExecState::Complete => TaskState::Done,
        ExecState::Failed | ExecState::BudgetExhausted => TaskState::Failed,
        _ => return Ok(None),
    };
    let Some(mut rec) = get(store, &of_session(&e.session_id))? else {
        return Ok(None);
    };
    if rec.state.is_closed() {
        return Ok(None);
    }
    let now = theseus_protocol::now_unix_ms();
    rec.version += 1;
    rec.state = outcome;
    rec.updated_at_ms = now;
    rec.evidence.push(TaskEvidence {
        node: report_node.map(String::from),
        identity: format!("report:{}", e.session_id),
        note: e
            .ended_reason
            .clone()
            .filter(|_| outcome == TaskState::Failed),
        by: "report".into(),
        at_ms: now,
    });
    Ok(Some(rec))
}

#[cfg(test)]
mod tests;
