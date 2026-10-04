//! The task graph's pure rules (39a): the compare of compare-and-swap, a
//! scope's tasks, a task session's state as it reads, and the view's bound.

use super::*;

fn task(id: &str, origin: &str, parent: Option<&str>, state: TaskState) -> TaskRecord {
    NewTask {
        id: id.into(),
        title: &format!("chart the {id} soundings"),
        objective: "chart the harbour".into(),
        acceptance: vec!["every buoy has a depth".into()],
        parent: parent.map(String::from),
        deps: vec![],
        session: None,
        origin: TaskOrigin {
            session: origin.into(),
            principal: "operator".into(),
        },
        state,
    }
    .build(1_790_000_000_000)
}

/// An edit names the version it read: the same one passes, any other is
/// refused with the record as it is now, and the message says both.
#[test]
fn a_stale_version_is_refused_with_the_task_as_it_is_now() {
    let mut t = task("tsk_harbour", "ses_lighthouse", None, TaskState::Accepted);
    assert!(check(&t, 1).is_ok());
    t.version = 3;
    t.title = "chart the outer harbour".into();
    let stale = check(&t, 2).unwrap_err();
    assert_eq!((stale.named, stale.now.version), (2, 3));
    let m = stale.message();
    assert!(
        m.contains("tsk_harbour changed since you read it: v2 → v3"),
        "{m}"
    );
    assert!(m.contains("\"chart the outer harbour\""), "{m}");
    assert!(
        check(&t, 4).is_err(),
        "a version from the future is no better"
    );
}

/// A conversation sees the tasks it started and everything under them; a
/// task's own session its subtree and its parent's line; another
/// conversation nothing of them.
#[test]
fn a_scope_is_the_conversations_tasks_or_a_tasks_subtree() {
    let all = vec![
        task("tsk_aaaa01", "ses_lighthouse", None, TaskState::Accepted),
        task(
            "tsk_aaaa02",
            "ses_lighthouse",
            Some("tsk_aaaa01"),
            TaskState::Accepted,
        ),
        task(
            "tsk_bbbb01",
            "ses_lighthouse",
            Some("tsk_aaaa02"),
            TaskState::Done,
        ),
        task("tsk_cccc01", "ses_ferry", None, TaskState::Accepted),
    ];
    let ids = |v: Vec<&TaskRecord>| v.into_iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    assert_eq!(
        ids(scope(&all, "ses_lighthouse")),
        ["tsk_aaaa01", "tsk_aaaa02", "tsk_bbbb01"]
    );
    assert_eq!(ids(scope(&all, "ses_ferry")), ["tsk_cccc01"]);
    assert!(scope(&all, "ses_quay").is_empty());
    // A task session's own task: its parent's line, then its subtree.
    assert_eq!(
        ids(scope(&all, "ses_aaaa02")),
        ["tsk_aaaa01", "tsk_aaaa02", "tsk_bbbb01"]
    );
}

fn exec(state: theseus_kernel::ExecState, wake: Option<Wake>) -> Execution {
    let mut e: Execution = serde_json::from_value(serde_json::json!({
        "id": "exe_aaaa02", "schema": 2, "session_id": "ses_aaaa02", "kind": "task",
        "state": "running", "authority": {"principal": "operator", "ceilings": {}},
        "budget": {"limit_micros": 1000000, "spent_micros": 0, "reserved_micros": 0,
                   "held_unknown_micros": 0, "reservations": {}, "resets": 0},
        "outstanding": [], "queued_results": [], "turns": 0, "interrupted": 0,
        "resume_pending": false, "created_at_ms": 0, "updated_at_ms": 0
    }))
    .unwrap();
    e.state = state;
    e.wake = wake;
    e
}

/// A task with a session reads its execution's running state; its record's
/// own state once closed; a plan item always its own.
#[test]
fn a_task_sessions_state_follows_its_execution_when_read() {
    use theseus_kernel::ExecState as X;
    let mut t = task("tsk_aaaa02", "ses_lighthouse", None, TaskState::InProgress);
    t.session = Some("ses_aaaa02".into());
    assert_eq!(execution_of(&t).as_deref(), Some("exe_aaaa02"));
    let at = |e: &Execution| state_now(&t, Some(e));
    assert_eq!(at(&exec(X::Running, None)), TaskState::InProgress);
    assert_eq!(
        at(&exec(
            X::Waiting,
            Some(Wake::Confirm {
                confirm_id: "c".into()
            })
        )),
        TaskState::WaitingHuman
    );
    assert_eq!(
        at(&exec(
            X::Waiting,
            Some(Wake::Budget {
                correlation_id: "b".into()
            })
        )),
        TaskState::Suspended
    );
    assert_eq!(at(&exec(X::Cancelled, None)), TaskState::Suspended);
    t.state = TaskState::Done;
    assert_eq!(
        state_now(&t, Some(&exec(X::Running, None))),
        TaskState::Done
    );
    let item = task("tsk_cccc01", "ses_ferry", None, TaskState::Accepted);
    assert_eq!(
        state_now(&item, Some(&exec(X::Running, None))),
        TaskState::Accepted
    );
}

/// The view: a line per open task with its id, title, state, owner, deps, a
/// line of acceptance, and version; a closed subtree on one line with its
/// count; nothing for a scope without tasks.
#[test]
fn the_view_shows_open_tasks_and_folds_closed_subtrees() {
    let mut all = vec![
        task("tsk_aaaa01", "ses_lighthouse", None, TaskState::Accepted),
        task("tsk_bbbb01", "ses_lighthouse", None, TaskState::Done),
        task(
            "tsk_bbbb02",
            "ses_lighthouse",
            Some("tsk_bbbb01"),
            TaskState::Done,
        ),
        task(
            "tsk_bbbb03",
            "ses_lighthouse",
            Some("tsk_bbbb01"),
            TaskState::Abandoned,
        ),
    ];
    all[0].deps = vec!["tsk_bbbb01".into()];
    let v = view::render(&all, "ses_lighthouse").unwrap();
    let lines: Vec<&str> = v.text.lines().collect();
    assert!(lines[0].starts_with("[The task graph in this conversation's scope: 1 open, 3 closed."));
    assert_eq!(
        lines[1],
        "- tsk_aaaa01 \"chart the tsk_aaaa01 soundings\" [accepted] owner agent, deps tsk_bbbb01, \
         accept: every buoy has a depth, v1"
    );
    assert_eq!(
        lines[2],
        "- tsk_bbbb01 \"chart the tsk_bbbb01 soundings\" [done], and 2 under it, all closed, v1"
    );
    assert_eq!(lines.len(), 3);
    assert_eq!(
        (
            v.summary.open,
            v.summary.closed,
            v.summary.lines,
            v.summary.left_out
        ),
        (1, 3, 2, 0)
    );
    assert!(view::render(&all, "ses_quay").is_none());
}

/// Past about 1,500 tokens the view shows open tasks only, then as many as
/// fit, and counts what it left out.
#[test]
fn the_view_is_bounded_and_counts_what_it_left_out() {
    let mut all = Vec::new();
    for i in 0..200 {
        let state = if i % 2 == 0 {
            TaskState::Accepted
        } else {
            TaskState::Done
        };
        all.push(task(&format!("tsk_{i:06}"), "ses_lighthouse", None, state));
    }
    let v = view::render(&all, "ses_lighthouse").unwrap();
    assert!(v.summary.tokens <= view::MAX_TOKENS + 40, "{:?}", v.summary);
    assert_eq!(v.summary.open, 100);
    assert_eq!(v.summary.closed, 100);
    assert!(
        v.summary.left_out >= 100,
        "every closed one first: {:?}",
        v.summary
    );
    assert_eq!(v.summary.lines + v.summary.left_out, 200);
    assert!(!v.text.contains("[done]"), "open tasks only past the bound");
    assert!(
        v.text.ends_with(&format!(
            "({} more left out past the view's bound; task.list or `theseus tasks` shows them all.)",
            v.summary.left_out
        )),
        "{}",
        v.text
    );
}

/// The view goes last in the request's last message, and the block before
/// it takes the conversation's breakpoint, so the next request still begins
/// with cached bytes.
#[test]
fn the_view_is_the_last_block_and_the_breakpoint_sits_before_it() {
    let mut req = crate::provider::ProviderRequest {
        messages: vec![serde_json::json!({"role": "user", "content": "chart the harbour"})],
        cache_control: Some(serde_json::json!({"type": "ephemeral"})),
        ..Default::default()
    };
    let all = vec![task(
        "tsk_aaaa01",
        "ses_lighthouse",
        None,
        TaskState::Accepted,
    )];
    let v = view::render(&all, "ses_lighthouse").unwrap();
    view::attach_to(&mut req, &v);
    let blocks = req.messages[0]["content"].as_array().unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["text"], "chart the harbour");
    assert_eq!(blocks[0]["cache_control"]["type"], "ephemeral");
    assert_eq!(blocks[1]["text"], v.text.as_str());
    assert!(blocks[1].get("cache_control").is_none());
}
