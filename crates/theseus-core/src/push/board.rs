//! The execution board (theseus-in3, 9b): one `ExecutionView` per execution,
//! seeded by the store's terms and kept current by the frames the observer
//! hands the push. Moved out of `push.rs` unchanged when the work board
//! (`work.rs`, theseus-hkx30) joined it: both are fed by the push's one
//! applier pass.

use std::collections::HashMap;

use theseus_kernel::{Action, Execution, BUDGET_TOOL, PROVIDER_TOOL};
use theseus_protocol::{attention, ExecutionView, GateDecision, PendingConfirm};
use theseus_store::{kinds, Store as _};

use super::{hm, pending_of, view, Frame};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};
use crate::Core;

/// One execution on the board.
#[derive(Default)]
pub(super) struct Entry {
    /// Its view as last sent, or as seeded. None while only its questions
    /// are known: a seed that read them before its record.
    pub(super) view: Option<ExecutionView>,
    /// Its questions, a budget question first: the view's `pending`.
    pub(super) pending: Vec<PendingConfirm>,
    /// Its execution record's position: a record at or below it is older.
    pub(super) exec_pos: u64,
    /// The highest position of its records that the seed read, frozen: an
    /// action record at or below it is on the board already.
    pub(super) seed_pos: u64,
    /// The model's own calls among its `outstanding`, known by their action
    /// records: its view leaves them out (below, `tool_calls`).
    pub(super) models: Vec<String>,
}

/// The calls in flight a view shows: tool calls only, never the model's own,
/// as `park` counts them. No surface reads the model's call from a view, and
/// counting it sent two views more for every loop of every turn.
pub(super) fn tool_calls(e: &Execution, models: &mut Vec<String>) -> u32 {
    models.retain(|c| e.outstanding.contains(c));
    e.outstanding.iter().filter(|c| !models.contains(c)).count() as u32
}

#[derive(Default)]
pub(super) struct Board {
    pub(super) entries: HashMap<String, Entry>,
    /// The last frame applied.
    pub(super) position: u64,
    /// Each session's execution with a view (theseus-id8d): `session.wait`
    /// finds a session's view by it, never by a walk of the board.
    pub(super) by_session: HashMap<String, String>,
    /// The entries with a view, and their questions, kept as they change so
    /// health counts nothing (theseus-id8d).
    pub(super) views: u64,
    pub(super) questions: u64,
}

/// The seed (theseus-id8d): the actions not settled, then the executions
/// that need you or work and the most recently written (`hot`), each by the
/// store's terms, with their positions: never every action ever, nor every
/// execution. A frame that lands between the reads is applied from the
/// queue, each record newer than what the board holds of its entity, and
/// applying one again changes nothing, so the board matches the store either
/// way. A settled action shows in no view, so the seed needs none: one whose
/// record comes off the queue changes nothing the board holds, and its older
/// records on the queue come before it.
#[cfg(test)]
pub(super) fn seed(core: &Core) -> anyhow::Result<Board> {
    seed_recent(core, super::hot::RECENT)
}

/// The seed, with the `recent` most recently written executions.
#[cfg(test)]
pub(super) fn seed_recent(core: &Core, recent: usize) -> anyhow::Result<Board> {
    Ok(seed_inputs(core, recent)?.0)
}

/// The seed with what it read: the executions and the actions not settled,
/// each with its record's position.
pub(super) type Seeded = (Board, Vec<(u64, Execution)>, Vec<(u64, Action)>);

/// The seed, and what it read, which the work board seeds from too.
pub(super) fn seed_inputs(core: &Core, recent: usize) -> anyhow::Result<Seeded> {
    let actions = core.kernel.open_actions_at()?;
    let execs = super::hot::executions(core, &actions, recent)?;
    let newest = core.kernel.newest_action_position()?;
    let mut b = seed_refs(core, &execs, &actions)?;
    b.position = b.position.max(newest);
    Ok((b, execs, actions))
}

/// The board from these executions and actions, with their positions: the
/// seed's, and its test's oracle, which hands it every action.
#[cfg(test)]
pub(super) fn seed_from(
    core: &Core,
    execs: Vec<(u64, Execution)>,
    actions: Vec<(u64, Action)>,
) -> anyhow::Result<Board> {
    seed_refs(core, &execs, &actions)
}

fn seed_refs(
    core: &Core,
    execs: &[(u64, Execution)],
    actions: &[(u64, Action)],
) -> anyhow::Result<Board> {
    let mut b = Board::default();
    let session_of: HashMap<&str, &str> = execs
        .iter()
        .map(|(_, e)| (e.id.as_str(), e.session_id.as_str()))
        .collect();
    for (p, e) in execs {
        let entry = b.entries.entry(e.id.clone()).or_default();
        entry.exec_pos = *p;
        entry.seed_pos = *p;
        b.position = b.position.max(*p);
    }
    let mut waiting = Vec::new();
    for (p, a) in actions {
        let (p, a) = (*p, a);
        let entry = b.entries.entry(a.execution_id.clone()).or_default();
        entry.seed_pos = entry.seed_pos.max(p);
        b.position = b.position.max(p);
        // Only one in flight: the seed reads every call an execution made.
        if a.tool == PROVIDER_TOOL && !a.state.is_settled() {
            entry.models.push(a.correlation_id.clone());
        }
        if a.awaits_confirm() {
            waiting.push(a.clone());
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
    for (_, e) in execs {
        let entry = b.entries.get_mut(&e.id).expect("seeded above");
        entry.pending = pending.remove(&e.id).unwrap_or_default();
        let parent = e
            .parent
            .as_deref()
            .and_then(|x| session_of.get(x))
            .map(|s| s.to_string());
        let mut v = view(
            e,
            entry.pending.clone(),
            parent,
            entry.seed_pos,
            e.updated_at_ms,
        );
        v.outstanding = tool_calls(e, &mut entry.models);
        v.attention = attention(&v, &hm);
        b.by_session.insert(e.session_id.clone(), e.id.clone());
        entry.view = Some(v);
    }
    // The questions of an execution whose record came after the first read:
    // kept until that record comes off the queue.
    for (id, ps) in pending {
        b.entries.entry(id).or_default().pending = ps;
    }
    (b.views, b.questions) = b.recount();
    Ok(b)
}

impl Board {
    /// A cold execution's view, from its record at `pos`, unless the board
    /// has one by now: a frame applied since the read made it.
    pub(super) fn load(
        &mut self,
        pos: u64,
        e: &Execution,
        parent: Option<String>,
    ) -> Option<ExecutionView> {
        let entry = self.entries.entry(e.id.clone()).or_default();
        if let Some(v) = &entry.view {
            return Some(v.clone());
        }
        if pos < entry.exec_pos {
            return None;
        }
        entry.exec_pos = pos;
        entry.seed_pos = entry.seed_pos.max(pos);
        let mut v = view(e, entry.pending.clone(), parent, pos, e.updated_at_ms);
        v.outstanding = tool_calls(e, &mut entry.models);
        v.attention = attention(&v, &hm);
        entry.view = Some(v.clone());
        self.views += 1;
        self.by_session.insert(e.session_id.clone(), e.id.clone());
        Some(v)
    }

    /// The views and questions, counted entry by entry: the seed's counts,
    /// and what the kept ones must equal.
    pub(super) fn recount(&self) -> (u64, u64) {
        let views = self.entries.values().filter(|e| e.view.is_some()).count();
        let questions: usize = self.entries.values().map(|e| e.pending.len()).sum();
        (views as u64, questions as u64)
    }

    /// Apply a frame: each record newer than what the board holds of its
    /// entity, then each touched execution's view, built again. Returns the
    /// views that changed in what a surface shows, with `previous` set.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub(super) fn apply(&mut self, core: &Core, f: &Frame) -> Vec<ExecutionView> {
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
                    // Every queue writes its row, a result's included
                    // (theseus-2xep): the why is read from it alone.
                    let id = row.data["execution_id"].as_str();
                    if row.kind == "execution.queued" {
                        if let (Some(id), Some(w)) = (id, row.data["why"].as_str()) {
                            why.insert(id.to_string(), w.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        for a in actions {
            let entry = self.entries.entry(a.execution_id.clone()).or_default();
            if a.tool == PROVIDER_TOOL
                && !a.state.is_settled()
                && !entry.models.contains(&a.correlation_id)
            {
                entry.models.push(a.correlation_id.clone());
            }
            let at = entry
                .pending
                .iter()
                .position(|p| p.correlation_id == a.correlation_id);
            match (a.awaits_confirm(), at) {
                (true, None) => asks.push(a),
                (false, Some(i)) => {
                    entry.pending.remove(i);
                    self.questions = self.questions.saturating_sub(1);
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
                self.questions += 1;
                touch(&mut touched, &a.execution_id);
            }
        }
        // A touched execution the board left cold, with no record here:
        // read, so its view shows what the frame changed (theseus-id8d).
        let mut loaded = Vec::new();
        for id in &touched {
            let cold = self.entries.get(id).is_none_or(|e| e.view.is_none());
            if cold && !execs.iter().any(|e| &e.id == id) {
                if let Ok(Some((pos, e))) = core.kernel.execution_at(id) {
                    let entry = self.entries.entry(id.clone()).or_default();
                    entry.exec_pos = entry.exec_pos.max(pos);
                    execs.push(e);
                    loaded.push(id.clone());
                }
            }
        }
        let mut out = Vec::new();
        for id in touched {
            let latest = execs.iter().find(|e| e.id == id);
            let parent = latest
                .and_then(|e| e.parent.as_deref())
                .and_then(|p| self.entries.get(p))
                .and_then(|p| p.view.as_ref())
                .map(|v| v.session_id.clone())
                .or_else(|| {
                    loaded
                        .contains(&id)
                        .then(|| parent_session(core, latest?))
                        .flatten()
                });
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
                    v.outstanding = tool_calls(e, &mut entry.models);
                    // A view still queued keeps the why it had, unless
                    // this frame wrote it a new one.
                    v.why = why
                        .remove(&id)
                        .or_else(|| old.as_ref().and_then(|o| o.why.clone()));
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
                None => self.views += 1,
            }
            self.by_session
                .insert(new.session_id.clone(), new.execution_id.clone());
            entry.view = Some(new.clone());
            out.push(new);
        }
        out
    }
}

/// The session of `e`'s parent, read from the store: a cold execution's
/// parent may be cold too.
pub(super) fn parent_session(core: &Core, e: &Execution) -> Option<String> {
    let parent = core.kernel.execution(e.parent.as_deref()?).ok()??;
    Some(parent.session_id)
}

pub(super) fn touch(touched: &mut Vec<String>, id: &str) {
    if !touched.iter().any(|t| t == id) {
        touched.push(id.to_string());
    }
}

/// Whether two views show the same: the state, what it waits on, its
/// questions, its turns, its tool calls in flight, how it ended, its
/// attention, and its spend and limit to the cent. A frame that changes none
/// of these sends nothing.
pub(super) fn same(a: &ExecutionView, b: &ExecutionView) -> bool {
    let cents = |x: f64| (x * 100.0).round() as i64;
    a.state == b.state
        && a.waiting_on == b.waiting_on
        && a.pending == b.pending
        && a.turns == b.turns
        && a.outstanding == b.outstanding
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
pub(super) fn read_nodes(core: &Core, positions: &[u64]) -> Vec<Node> {
    positions
        .iter()
        .filter_map(|p| core.store.inner().get(*p).ok().flatten())
        .filter_map(|r| r.decode::<Node>().ok())
        .collect()
}

/// The gate's decision on the tool call `correlation_id` names, if `n` is
/// that call's node.
pub(super) fn decision_on(n: &Node, correlation_id: &str) -> Option<GateDecision> {
    match &n.body {
        Body::ToolCall {
            correlation_id: Some(c),
            gate,
            ..
        } if c == correlation_id => gate.as_ref().and_then(|g| g.decision.clone()),
        _ => None,
    }
}
