//! A task that ended before any sync saw it running is owed one thread, or post, in its final state
//! (theseus-hzdlf); one that ended before this binding started is never back-filled, and a task that
//! has its own is never given a second.

use super::*;
use crate::runtime::voice_status::tests::view;

/// A task of `session`, born at `born_ms`.
fn task(id: &str, state: &str, born_ms: u64) -> WorkView {
    let mut v = view(id, "task", state, "Chart the reef", None);
    v.session_id = Some(format!("ses_{id}"));
    v.started_at_ms = born_ms;
    v
}

#[test]
fn a_task_that_ended_before_its_first_sync_gets_one_post() {
    let t = TaskThreads::default();
    let after = t.since() + 1;
    // Seen over for the first time: one post, made in its final state (`make_task_thread` names it
    // with the done mark), however the sync raced the task.
    assert_eq!(
        t.next(&task("exe_fast", "done", after), None, true),
        Next::Create
    );
    // While it is made the same sync, run again, makes no second.
    assert_eq!(
        t.next(&task("exe_fast", "done", after), None, true),
        Next::Nothing
    );
    t.settle("exe_fast");
    // Its thread is on the list: the one rename, as for any task, and nothing after it.
    let made = Made {
        task: "exe_fast".into(),
        session: "ses_exe_fast".into(),
        thread: 900_000_000_000_500_001,
        kind: PlaceKind::Thread,
        parent: 223_456_789_012_345_678,
        parent_session: "ses_parent".into(),
        name: task_name(Mark::Done, "exe_fast", "Chart the reef"),
        guild: None,
        private: true,
        ceiling: None,
        created_ms: theseus_protocol::now_unix_ms(),
        ended_ms: None,
    };
    t.add(made);
    assert!(matches!(
        t.next(&task("exe_fast", "done", after), None, true),
        Next::Finish(_, Mark::Done)
    ));
    t.update("exe_fast", |m| m.ended_ms = Some(1));
    t.settle("exe_fast");
    // The place and its row go after the grace: the task is still never made another.
    t.retire_row("exe_fast");
    assert_eq!(
        t.next(&task("exe_fast", "done", after), None, true),
        Next::Nothing
    );
}

#[test]
fn an_old_ended_task_gets_no_post_at_start() {
    let t = TaskThreads::default();
    let start = t.since();
    // Born before the binding began (an upgrade: every task the store remembers), ended or not: none.
    assert_eq!(
        t.next(&task("exe_old", "done", start - 60_000), None, true),
        Next::Nothing
    );
    assert_eq!(
        t.next(&task("exe_old", "failed", start - 60_000), None, true),
        Next::Nothing
    );
    // A task with no known birth is no new task.
    assert_eq!(
        t.next(&task("exe_none", "done", 0), None, true),
        Next::Nothing
    );
    // A place that makes no threads makes none for a task over.
    assert_eq!(
        t.next(&task("exe_dm", "done", start + 1), None, false),
        Next::Nothing
    );
}
