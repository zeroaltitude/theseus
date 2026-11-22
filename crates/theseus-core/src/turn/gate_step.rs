//! A turn's presence for the gate (issue 44, step 2; `crate::view_gate`):
//! computed once, after the turn's input is written, from the input's mark;
//! for a turn no person's message opened, from the turn that set it up (a
//! wake's `wake.at` call, a task's `task.create` call, or the call that
//! started the task whose report this is), as the ledger wrote that turn's
//! ask and its mark. None found is `Unknown`. Held per session for the tools
//! the turn runs; nothing new is written. A steer the turn absorbs narrows
//! it at once ([`TurnRunner::gate_steered`]). With `[context] ledger` off
//! nothing is computed, and recall reads places as before.

use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use super::{Turn, TurnRunner};
use crate::session::SessionRecord;
use crate::view_gate::{self, Proof};

impl TurnRunner {
    pub(super) fn gate_asking(&self, t: &mut Turn<'_>) {
        if !self.cfg.context.ledger_on() {
            return;
        }
        let sid = t.tc.session_id;
        let today = self.place_of(sid);
        let target = self.outbox.try_target(sid).ok().flatten();
        let mark = t.exchange.mark().cloned();
        let place = match &mark {
            Some(m) => m.place.clone(),
            None => crate::exchange::place_of(target.as_deref(), t.tc.dir),
        };
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
        let asking = view_gate::begin(self, sid, t.tc.turn_id, place, proof);
        // The trigger of step 3b, in shadow and off the turn's path: the
        // owner's own message where he is not alone (`judge::leans_on`).
        if let Some(m) = &mark {
            if crate::judge::leans_on::asked_of(m, asking.present()) {
                let history = self.tools.history.clone();
                let (turn, node) = (t.tc.turn_id.to_string(), m.node.clone());
                let then = Box::new(move |answer: Option<String>| {
                    if let Some(core) = history.core() {
                        tokio::spawn(crate::release_card::after_trigger(core, turn, node, answer));
                    }
                });
                self.judge.at_leans_on(sid, t.tc.turn_id, &m.node, then);
            }
        }
    }

    /// A steer the running turn absorbed, its submission's mark `arrival`:
    /// the turn's presence narrows to it (never widens).
    pub(super) fn gate_steered(&self, t: &Turn<'_>, arrival: Option<&crate::exchange::OwnerMark>) {
        if self.cfg.context.ledger_on() {
            view_gate::steered(self, t.tc.session_id, arrival);
        }
    }
}

/// The presence of the turn that started the task `task` (its session), whose
/// report this session takes, at `class`, this session's place.
fn task_set_up(store: &crate::store::Store, class: &Place, task: &str) -> Option<Present> {
    let rec: SessionRecord = store.get_session(task).ok()??;
    let of = rec.task?;
    view_gate::set_up(store, class, &of.parent_session, &of.by)
}
