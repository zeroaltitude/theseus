//! `TaskInfo.starts_at_ms` (theseus-rcgph) is additive and optional: a task
//! that has started serializes exactly as before, a deferred one adds the
//! field, and a peer from before it decodes either.

use serde_json::json;

use crate::TaskInfo;

/// A `task.list` entry as a daemon from before the field sent it.
fn old() -> serde_json::Value {
    json!({
        "task_id": "ses_t4sk01", "short": "t4sk01", "execution_id": "exe_t4sk01",
        "parent_session_id": "ses_p4rent", "parent_execution_id": "exe_p4rent",
        "title": "Draft the notes", "state": "waiting", "waiting_on": "a due time",
        "spent_usd": 0.0, "limit_usd": 1.5, "cost_usd": 0.0, "turns": 0,
        "created_at_ms": 1_000, "updated_at_ms": 1_000
    })
}

#[test]
fn a_task_info_with_no_start_is_the_bytes_it_always_was() {
    let t: TaskInfo = serde_json::from_value(old()).unwrap();
    assert_eq!(t.starts_at_ms, None);
    let back = serde_json::to_value(&t).unwrap();
    assert!(back.get("starts_at_ms").is_none(), "{back}");
    assert_eq!(back["pending_confirms"], 0);
    // Every key of the old line is still there, with its value.
    for (k, v) in old().as_object().unwrap() {
        assert_eq!(&back[k], v, "{k}");
    }
}

#[test]
fn a_deferred_tasks_start_rides_the_wire_and_an_old_peer_ignores_it() {
    let mut line = old();
    line["starts_at_ms"] = json!(1_790_000_120_000u64);
    let t: TaskInfo = serde_json::from_value(line.clone()).unwrap();
    assert_eq!(t.starts_at_ms, Some(1_790_000_120_000));
    assert_eq!(
        serde_json::to_value(&t).unwrap()["starts_at_ms"],
        1_790_000_120_000u64
    );
    // A peer that does not know the field reads the rest of the line.
    #[derive(serde::Deserialize)]
    struct Before {
        short: String,
        turns: u64,
    }
    let b: Before = serde_json::from_value(line).unwrap();
    assert_eq!((b.short.as_str(), b.turns), ("t4sk01", 0));
}
