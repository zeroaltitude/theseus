//! A task's own place (issue 44; the design's §7, "a task's own view"): a task's turns arrive in
//! `task:<task id>`, the place its exchanges are written at and its view is anchored to. No binding names
//! it and nothing but the task's own turns is written there, so its window is the task's own earlier
//! exchanges. The name is the task's thing key (`joined::thing_of`): one name for one task.

use crate::session::SessionRecord;
use crate::store::Store;

/// What every task's place begins with.
pub const PREFIX: &str = "task:";

/// The place of the task whose session is `session_id`.
pub fn of_session(session_id: &str) -> String {
    super::joined::thing_of(&super::of_session(session_id))
}

/// The task a place names, by its id; none for a place that is not a task's.
pub fn task_of(place: &str) -> Option<&str> {
    place
        .strip_prefix(PREFIX)
        .filter(|id| id.starts_with(super::ID_PREFIX))
}

/// The place of `session` when it is a task's session; none for any other, or one whose record cannot be
/// read.
pub fn of_task_session(store: &Store, session: &str) -> Option<String> {
    let rec: SessionRecord = store.get_session(session).ok()??;
    rec.task.map(|_| of_session(session))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tasks_place_names_its_task_and_nothing_else_reads_as_one() {
        let place = super::of_session("ses_abc");
        assert_eq!(place, "task:tsk_abc");
        assert_eq!(super::task_of(&place), Some("tsk_abc"));
        for other in [
            "discord:dm:1",
            "local:/w",
            "task:",
            "task:ses_abc",
            "unknown",
        ] {
            assert_eq!(super::task_of(other), None, "{other}");
        }
    }
}
