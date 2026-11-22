//! A turn's presence for the gate (issue 44, step 2; `crate::view_gate`):
//! computed once, after the turn's input is written, from the input's mark,
//! at the turn's one place (`turn_place`, its exchange's);
//! for a turn no person's message opened, from the turn that set it up (a
//! wake's `wake.at` call, a task's `task.create` call, the call that
//! started the task whose report this is, or, for a continuation, the turn it
//! continues: the call its result answers, or the turn it resumes, `resume_step`), as the ledger wrote
//! that turn's ask and its mark, or what that turn was given when no marked message opened it
//! (`exchange::given`, kept here for each such turn). None found is `Unknown`. Held per session for the tools
//! the turn runs. A steer the turn absorbs narrows it at once ([`TurnRunner::gate_steered`]).

use theseus_memory::gate::Present;
use theseus_memory::recall::Place;

use super::{Turn, TurnRunner};
use crate::exchange::given::Given;
use crate::session::SessionRecord;
use crate::view_gate::{self, Proof};

impl TurnRunner {
    pub(super) fn gate_asking(&self, t: &mut Turn<'_>) {
        let sid = t.tc.session_id;
        let today = self.place_of(sid);
        // The one place of the turn, its exchange's too (`turn_place`); unreadable, no place.
        let place =
            (self.turn_place(t)).unwrap_or_else(|| crate::exchange::rebuild::UNKNOWN.into());
        let mark = t.exchange.mark().cloned();
        // A turn no message, wake or report opened: what it continues set it up (`resume_step`).
        let continued = match (&mark, t.exchange.occasion()) {
            (None, Some(_)) => self.continued_presence(t, &today),
            _ => Vec::new(),
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
                set_ups.extend(continued);
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
        // No person's message to read it from again (a wake, a report, a continuation): what it was given
        // is kept for a turn that continues it.
        let unopened =
            t.exchange.occasion().is_some() || !t.wakes.is_empty() || !t.woke_by.is_empty();
        if mark.is_none() && unopened {
            let continues = (t.exchange.continues()).and_then(|c| c.first().cloned().flatten());
            let given = Given::new(t.tc.turn_id, asking.present(), continues);
            self.exchange_given(t, &given);
        }
    }

    /// A steer the running turn absorbed, its submission's mark `arrival`:
    /// the turn's presence narrows to it (never widens).
    pub(super) fn gate_steered(&self, t: &Turn<'_>, arrival: Option<&crate::exchange::OwnerMark>) {
        view_gate::steered(self, t.tc.session_id, arrival);
    }
}

/// The presence of the turn that started the task `task` (its session), whose
/// report this session takes, at `class`, this session's place.
fn task_set_up(store: &crate::store::Store, class: &Place, task: &str) -> Option<Present> {
    let rec: SessionRecord = store.get_session(task).ok()??;
    let of = rec.task?;
    view_gate::set_up(store, class, &of.parent_session, &of.by)
}
