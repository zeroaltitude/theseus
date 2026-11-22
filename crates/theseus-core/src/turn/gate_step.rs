//! A turn's presence for the gate (issue 44, step 2; `crate::view_gate`):
//! computed once, after the turn's input is written, from the input's mark,
//! at the turn's one place (`turn_place`, its exchange's);
//! for a turn no person's message opened, from the turn that set it up (a
//! wake's `wake.at` call, a task's `task.create` call, the call that
//! started the task whose report this is, or, for a continuation, the turn it
//! continues: the call its result answers, or the turn it resumes), as the ledger wrote that turn's
//! ask and its mark. None found is `Unknown`. Held per session for the tools
//! the turn runs; nothing new is written. A steer the turn absorbs narrows
//! it at once ([`TurnRunner::gate_steered`]). With `[context] ledger` off
//! nothing is computed, and recall reads places as before.

use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use super::{Turn, TurnRunner};
use crate::node::Body;
use crate::stub::Kind;
use crate::session::SessionRecord;
use crate::view_gate::{self, Proof};

impl TurnRunner {
    pub(super) fn gate_asking(&self, t: &mut Turn<'_>) {
        if !self.cfg.context.ledger_on() {
            return;
        }
        let sid = t.tc.session_id;
        let today = self.place_of(sid);
        // The one place of the turn, its exchange's too (`turn_place`); unreadable, no place.
        let place = (self.turn_place(t)).unwrap_or_else(|| crate::exchange::rebuild::UNKNOWN.into());
        let mark = t.exchange.mark().cloned();
        let proof = match &mark {
            Some(m) => Proof::Mark(m),
            None => {
                let store = &self.store;
                let mut set_ups: Vec<Option<Present>> = t
                    .set_ups
                    .iter()
                    .map(|call| view_gate::set_up(store, &today, sid, call))
                    .collect();
                for r in &t.woke_by {
                    let task = r["task"].as_str().unwrap_or_default();
                    set_ups.push(task_set_up(store, &today, task));
                }
                // A turn no message, wake or report opened: what it continues set it up (issue 44).
                if t.exchange.occasion().is_some() {
                    set_ups.extend(continued(store, &today, sid, t.tc.turn_id));
                }
                if let Some(task) = t.tc.task {
                    let class = self.place_of(&task.parent_session);
                    set_ups.push(view_gate::set_up(
                        store,
                        &class,
                        &task.parent_session,
                        &task.by,
                    ));
                }
                Proof::SetUp(view_gate::narrowest(set_ups))
            }
        };
        view_gate::begin(self, sid, t.tc.turn_id, place, proof);
    }

    /// A steer the running turn absorbed, its submission's mark `arrival`:
    /// the turn's presence narrows to it (never widens).
    pub(super) fn gate_steered(&self, t: &Turn<'_>, arrival: Option<&crate::exchange::OwnerMark>) {
        if self.cfg.context.ledger_on() {
            view_gate::steered(self, t.tc.session_id, arrival);
        }
    }
}

/// What a turn no message opened continues, each one's presence read as a set-up's: the turn that made
/// each call its own results answer (a job's late result, an answered call), else the session's newest
/// turn before it (an interrupted one). None found is `Unknown`, as for any set-up.
fn continued(store: &crate::store::Store, class: &Place, sid: &str, turn: &str) -> Vec<Option<Present>> {
    let Ok(nodes) = store.transcript(sid) else {
        return vec![None];
    };
    let own: Vec<Option<Present>> = (nodes.iter())
        .filter(|(_, n)| n.turn_id.as_deref() == Some(turn) && n.kind == Kind::ToolResult)
        .map(|(_, n)| match &n.node().body {
            Body::ToolResult { tool_use_id, .. } => view_gate::set_up(store, class, sid, tool_use_id),
            _ => None,
        })
        .collect();
    if !own.is_empty() {
        return own;
    }
    let earlier = (nodes.iter().rev())
        .filter(|(_, n)| !n.kind.is_note())
        .find_map(|(_, n)| n.turn_id.as_deref().filter(|id| *id != turn));
    vec![earlier.and_then(|id| view_gate::continued(store, class, id))]
}

/// The presence of the turn that started the task `task` (its session), whose
/// report this session takes, at `class`, this session's place.
fn task_set_up(store: &crate::store::Store, class: &Place, task: &str) -> Option<Present> {
    let rec: SessionRecord = store.get_session(task).ok()??;
    let of = rec.task?;
    view_gate::set_up(store, class, &of.parent_session, &of.by)
}
