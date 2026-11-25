//! A finding filed from chat waits before its first build (issue 57): its note, written with it,
//! reaches the owner with its Undo before the work starts. A filing from the owner's own shell
//! does not wait, and a chat filing past the hold starts as any item does.

use theseus_protocol::tasks::TaskRecord;

use super::queues::CHAT_HOLD;
use super::tests::{rig_with, Rig};
use crate::rsi::backlog::tool::OWNER;

fn task(r: &Rig, item: &str) -> TaskRecord {
    crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn a_chat_filing_waits_before_its_first_build_and_a_shells_does_not() {
    let r = rig_with(5, 100.0);
    let chat = r
        .core
        .self_file_in_chat("The digest folds two kinds into one line.", None, OWNER)
        .unwrap()
        .item;
    let shell = r
        .core
        .self_file_in_chat("The gate runs clippy twice.", None, "the CLI")
        .unwrap()
        .item;
    let tick = r.tick().await;
    assert!(tick.started.contains(&shell), "{:?}", tick.started);
    assert!(!tick.started.contains(&chat), "held: {:?}", tick.started);
    // Filed a hold ago: it starts as any item does.
    let mut t = task(&r, &chat);
    for e in &mut t.evidence {
        e.at_ms -= CHAT_HOLD.as_millis() as u64 + 1;
    }
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    let tick = r.tick().await;
    assert!(tick.started.contains(&chat), "{:?}", tick.started);
}
