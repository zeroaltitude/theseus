//! A child's Google-private mark, taken from its parent after it started
//! (theseus-y8k3u), at every level above it: the child's next call is held by the
//! floor once the parent (or any task above it) is marked, and made when none is.
//! Carried over, unchanged, from the China-origin lane's follow-ups
//! (`tests_cn_follow`, retired with the open view, theseus-9ymz0). Every name
//! here is invented.

use serde_json::json;
use theseus_protocol::task_start::TaskStartParams;

use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::test_support::until;
use crate::tests_tasks::{answers_a_call, exec, first_user, rig};

fn record(core: &crate::Core, sid: &str) -> SessionRecord {
    core.store.get_session(sid).unwrap().unwrap()
}

/// theseus-y8k3u 1: a task started from a plain parent is private once its
/// parent is, however late the parent took the mark: the child's next call
/// is held by the floor (the default provider here is of EU origin, so only
/// the mark can refuse it). A child whose parent stays plain makes it.
#[tokio::test]
async fn a_child_started_plain_is_private_once_its_parent_is() {
    use crate::tests_private_background::{mark, private_on};
    const CHILD: &str = "Count the quays of the harbour";
    for marked in [false, true] {
        let mut r = rig(
            |req| {
                if !first_user(req).contains(CHILD) {
                    return Scripted::text("ok");
                }
                match answers_a_call(req) {
                    true => Scripted::text("Counted 3 quays."),
                    false => Scripted::tools("", &[("t1", "tools_find", json!({"query": "quay"}))]),
                }
            },
            private_on,
        );
        let home = r.core.home().unwrap().session_id;
        *r.model.hold.lock().unwrap() = Some(CHILD.into());
        let p = TaskStartParams {
            brief: CHILD.into(),
            ..Default::default()
        };
        let child = r
            .core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
        r.entered.recv().await.expect("the child's first call");
        *r.model.hold.lock().unwrap() = None;
        if marked {
            mark(&r.core, &home);
        }
        r.model.gate.add_permits(1);
        let id = child.session_id.replacen("ses_", "exe_", 1);
        until("the child's turn to end", || {
            !matches!(
                exec(&r.core, &id).state,
                theseus_kernel::ExecState::Queued | theseus_kernel::ExecState::Running
            )
        })
        .await;
        let calls = r
            .model
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|q| first_user(q).contains(CHILD))
            .count();
        assert_eq!(
            calls,
            if marked { 1 } else { 2 },
            "marked {marked}: the child's calls that reached the model"
        );
    }
}

/// theseus-y8k3u, every level: a task whose grandparent takes the mark after
/// it started is private too, though its parent never read a Google result.
/// Here the task's parent (the owner's Home) is itself a task of a third
/// session, and that session is marked while the task waits on its first
/// call: the task's next call is held by the floor. Unmarked, it makes it.
#[tokio::test]
async fn a_grandchild_started_plain_is_private_once_its_grandparent_is() {
    use crate::tests_private_background::{mark, private_on};
    const CHILD: &str = "Count the quays of the harbour";
    const TOP: &str = "ses_the_grandparent";
    for marked in [false, true] {
        let mut r = rig(
            |req| {
                if !first_user(req).contains(CHILD) {
                    return Scripted::text("ok");
                }
                match answers_a_call(req) {
                    true => Scripted::text("Counted 3 quays."),
                    false => Scripted::tools("", &[("t1", "tools_find", json!({"query": "quay"}))]),
                }
            },
            private_on,
        );
        let home = r.core.home().unwrap();
        *r.model.hold.lock().unwrap() = Some(CHILD.into());
        let p = TaskStartParams {
            brief: CHILD.into(),
            ..Default::default()
        };
        let child = r
            .core
            .task_start(&p, crate::tests_task_brief::cli())
            .unwrap();
        r.entered.recv().await.expect("the child's first call");
        *r.model.hold.lock().unwrap() = None;
        // The child's parent is itself a task of TOP.
        let mut middle = record(&r.core, &home.session_id);
        middle.task = Some(crate::session::TaskOf {
            parent_session: TOP.into(),
            parent_execution: "exe_the_grandparent".into(),
            by: "t0".into(),
            target: None,
            arrangement: None,
            check: None,
            notify_done: false,
        });
        r.core.store.put_session(&home.session_id, &middle).unwrap();
        if marked {
            mark(&r.core, TOP);
        }
        r.model.gate.add_permits(1);
        let id = child.session_id.replacen("ses_", "exe_", 1);
        until("the child's turn to end", || {
            !matches!(
                exec(&r.core, &id).state,
                theseus_kernel::ExecState::Queued | theseus_kernel::ExecState::Running
            )
        })
        .await;
        let calls = r
            .model
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|q| first_user(q).contains(CHILD))
            .count();
        assert_eq!(
            calls,
            if marked { 1 } else { 2 },
            "marked {marked}: the grandchild's calls that reached the model"
        );
    }
}
