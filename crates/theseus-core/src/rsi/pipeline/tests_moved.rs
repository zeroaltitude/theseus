//! The start that follows a change sent back because main moved (issue 20): the pipeline starts
//! the item in the tree it finds, with the builder told what happened, and nothing of it counts
//! as a try.

use serde_json::json;
use theseus_protocol::tasks::{TaskEvidence, TaskState};

use super::tests::rig_with;
use crate::rsi::backlog::attempts;

/// A green branch is sent back (the join's own writes, as its conflict path makes them), and the
/// item's next start carries the paragraph that says so; a start after that does not.
#[tokio::test]
async fn a_change_sent_back_for_main_moving_starts_with_the_builder_told() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    let (branch, head) = r.built(&item, 1);
    let mut t = crate::task_graph::get(&r.core.store, &item)
        .unwrap()
        .unwrap();
    t.state = TaskState::Proposed;
    t.session = None;
    t.version += 1;
    for (identity, note) in [
        (
            format!("conflict:{branch}@{}", &head[..12]),
            "it conflicts with main in a.rs",
        ),
        (
            format!("main-merged:{branch}-2"),
            "it conflicts with main in a.rs",
        ),
    ] {
        t.evidence.push(TaskEvidence {
            node: None,
            identity,
            note: Some(note.into()),
            by: "self-join".into(),
            at_ms: theseus_protocol::now_unix_ms(),
        });
    }
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    r.core
        .rec(None)
        .record(&crate::fact::rsi_join::SelfJoinStopped {
            branch,
            head,
            phase: "merge".into(),
            why: "it conflicts with main in a.rs".into(),
            numbers: json!({"held": true, "conflict": true, "main_merged": true}),
        });
    assert!(attempts::tries(&t, 0).is_empty(), "sent back is no try");
    let tick = r.tick().await;
    assert_eq!(tick.started, std::slice::from_ref(&item), "{tick:?}");
    let objectives = r.fake.objectives.lock().unwrap().clone();
    let last = objectives.last().unwrap();
    assert!(
        last.contains("Main moved while your finished change waited") && last.contains("a.rs"),
        "{last}"
    );
    // The first start never carried it.
    assert!(!objectives[0].contains("Main moved"), "{}", objectives[0]);
}
