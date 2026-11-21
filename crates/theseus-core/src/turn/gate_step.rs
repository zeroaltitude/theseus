//! A turn's presence for the gate (issue 44, step 2; `crate::view_gate`):
//! computed once, after the turn's input is written, from the input's mark;
//! for a turn no person's message opened, from what the turn that set it up
//! recorded: a wake's `wake.at` call, a task's `task.create` call, or the
//! call that started the task whose report this is. None recorded is
//! `Unknown`. Held per session for the tools the turn runs, and recorded as
//! `presence.<turn>` (riding a frame the turn writes anyway) for the wakes
//! and tasks it sets up. With `[context] ledger` off nothing is computed,
//! and recall reads places as before.

use theseus_memory::gate::Present;

use super::{Turn, TurnRunner};
use crate::session::SessionRecord;
use crate::view_gate::{self, Asking};

impl TurnRunner {
    pub(super) fn gate_asking(&self, t: &mut Turn<'_>) {
        if !self.cfg.context.ledger_on() {
            return;
        }
        let sid = t.tc.session_id;
        let today = self.place_of(sid);
        let target = self.outbox.try_target(sid).ok().flatten();
        let mark = t.exchange.mark();
        let place = match mark {
            Some(m) => m.place.clone(),
            None => crate::exchange::place_of(target.as_deref(), t.tc.dir),
        };
        let present = match mark {
            Some(m) => view_gate::present_of_mark(&today, m),
            None => {
                let store = &self.store;
                let mut set_ups: Vec<Option<Present>> = t
                    .set_ups
                    .iter()
                    .map(|call| view_gate::set_up(store, sid, call))
                    .collect();
                for r in &t.woke_by {
                    let task = r["task"].as_str().unwrap_or_default();
                    set_ups.push(task_set_up(store, task));
                }
                if let Some(task) = t.tc.task {
                    set_ups.push(view_gate::set_up(store, &task.parent_session, &task.by));
                }
                view_gate::present(
                    &today,
                    Some(&place),
                    view_gate::Input::SetUp(view_gate::narrowest(set_ups)),
                    store,
                )
            }
        };
        if let Ok(r) = view_gate::presence_record(t.tc.turn_id, present) {
            if let Err(e) = t.tc.store.defer(r) {
                tracing::warn!(error = %format!("{e:#}"), "a turn's presence was not recorded");
            }
        }
        self.presence.set(
            sid,
            Asking {
                turn_id: t.tc.turn_id.into(),
                present,
                place,
            },
        );
    }
}

/// The presence the turn that started the task `task` (its session) recorded.
fn task_set_up(store: &crate::store::Store, task: &str) -> Option<Present> {
    let rec: SessionRecord = store.get_session(task).ok()??;
    let of = rec.task?;
    view_gate::set_up(store, &of.parent_session, &of.by)
}
