//! `status.short` (theseus-8kn12) against a real core: a session that asks a
//! question counts as needing you, an answer moves it to working, the counts
//! are the full read's (`executions.watch`) recounted, and asking makes no
//! subscription.

use super::*;
use theseus_protocol::StatusShortResult;

fn recount(snap: &ExecutionsWatchResult) -> (u32, u32, u32) {
    let n =
        |f: &dyn Fn(&ExecutionView) -> bool| snap.executions.iter().filter(|v| f(v)).count() as u32;
    let needs = |v: &ExecutionView| v.attention.level == theseus_protocol::Level::NeedsYou;
    (
        n(&|v| needs(v) && v.state != "failed"),
        n(&|v| v.attention.level == theseus_protocol::Level::Working),
        n(&|v| needs(v) && v.state == "failed"),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_boards_counts_follow_a_question_and_its_answer_with_no_subscription() {
    let r = rig_full(
        vec![
            Scripted::tools(
                "",
                &[("t1", "fs_write", json!({"path": "a.txt", "content": "a"}))],
            ),
            Scripted::text("Not written, then."),
        ],
        true,
        false,
    );
    let mut c = Raw::connect(&r.core, "short", 1 << 20);
    let none: StatusShortResult = serde_json::from_value(
        c.call(1, method::STATUS_SHORT, json!({}))
            .await
            .result
            .unwrap(),
    )
    .unwrap();
    assert_eq!((none.needs_you, none.working, none.failed.len()), (0, 0, 0));
    assert_eq!(none.first, None);

    let asked = turn_result(&r.core, None).await;
    let exec = asked.execution_id.clone().unwrap();
    let mid: StatusShortResult = serde_json::from_value(
        c.call(2, method::STATUS_SHORT, json!({}))
            .await
            .result
            .unwrap(),
    )
    .unwrap();
    assert_eq!((mid.needs_you, mid.working), (1, 0));
    assert_eq!(mid.first.as_deref(), Some(exec.as_str()));
    assert!(mid.position > none.position);

    // What the full read says of the same board.
    let mut w = Raw::connect(&r.core, "full", 1 << 20);
    let snap: ExecutionsWatchResult = serde_json::from_value(
        w.call(3, method::EXECUTIONS_WATCH, json!({"limit": 0}))
            .await
            .result
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        (mid.needs_you, mid.working, mid.failed.len() as u32),
        recount(&snap)
    );
    assert_eq!(
        r.core.bus.all_watchers(),
        1,
        "only the full read subscribed"
    );
}
