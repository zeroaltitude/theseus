//! A stale in-progress item is settled (issue 6, condition 8): in progress, and in no queue.

use theseus_protocol::tasks::{TaskEvidence, TaskState};

use super::tests::rig_with;

fn in_progress_with_nothing_behind_it(r: &super::tests::Rig, item: &str) {
    let mut t = crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap();
    t.state = TaskState::InProgress;
    t.session = None;
    t.evidence.retain(|e| !e.identity.starts_with("session:"));
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
}

/// KEY: an item the record calls in progress, with no session and no queue, is failed once with
/// its reason; one genuinely building is left alone.
#[tokio::test]
async fn an_item_in_progress_with_no_build_behind_it_is_settled_as_failed() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    let building = r.fake.starts.lock().unwrap()[0].0.clone();
    // Another item, never started, put in progress by a record nothing builds.
    let order = r.view();
    let stale = order.ready[0].item.clone();
    assert_ne!(stale, building);
    in_progress_with_nothing_behind_it(&r, &stale);
    r.tick().await;
    assert_eq!(r.state(&stale), TaskState::Failed);
    assert_eq!(
        r.state(&building),
        TaskState::InProgress,
        "a real build stays"
    );
    let t = crate::task_graph::get(&r.core.store, &stale)
        .unwrap()
        .unwrap();
    let settled: Vec<&TaskEvidence> = t
        .evidence
        .iter()
        .filter(|e| e.identity == "settled:stale")
        .collect();
    assert_eq!(settled.len(), 1);
    assert_eq!(
        settled[0].note.as_deref(),
        Some("it was in progress with no build behind it")
    );
    let ended = r.rows("self.pipeline.ended");
    assert_eq!(ended.len(), 1, "{ended:?}");
    assert_eq!(ended[0]["reason"], "stale");
    // Said once, however often it ticks, and no try was counted against it.
    r.tick().await;
    assert_eq!(r.rows("self.pipeline.ended").len(), 1);
    assert!(crate::rsi::backlog::attempts::tries(&t, 0).is_empty());
}
