//! Setting one item aside (issue 6, condition 6): nothing starts it, nothing is forbidden,
//! and the owner's bring-back puts it back; a drop, beside it, forbids the cause.

use theseus_protocol::rsi::{SelfDropParams, SelfRetryParams};
use theseus_protocol::tasks::TaskState;

use super::tests::{causes, rig_with, Rig};
use crate::approval::Answerer;

fn aside(r: &Rig, item: &str) -> theseus_protocol::rsi::SelfDropResult {
    r.core
        .self_drop(
            &SelfDropParams {
                item: item.into(),
                set_aside: true,
                why: Some("it keeps failing the same way".into()),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap()
}

fn cause_of(r: &Rig, item: &str) -> String {
    crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap()
        .evidence
        .iter()
        .find_map(|e| e.identity.strip_prefix("cause:").map(str::to_string))
        .unwrap()
}

fn items_of(r: &Rig, cause: &str) -> usize {
    crate::task_graph::all(&r.core.store)
        .unwrap()
        .iter()
        .filter(|t| {
            t.evidence
                .iter()
                .any(|e| e.identity == format!("cause:{cause}"))
        })
        .count()
}

/// KEY: a ready item set aside is never started; the night's evidence goes on its item and no
/// new one is filed; bringing it back lets it start; and a drop of another cause is the contrast.
#[tokio::test]
async fn a_set_aside_item_starts_nothing_and_forbids_nothing() {
    let r = rig_with(1, 30.0);
    let top = r.view().ready[0].item.clone();
    let said = aside(&r, &top);
    assert!(said.changed && said.set_aside, "{said:?}");
    assert_eq!(r.state(&top), TaskState::WaitingHuman);
    // The pipeline starts the next one, never this one.
    let t = r.tick().await;
    assert_eq!(t.started.len(), 1, "{t:?}");
    assert_ne!(t.started[0], top);
    let view = r.view();
    let held = view
        .held
        .iter()
        .find(|h| h.item == top)
        .expect("it is shown as held");
    assert!(
        held.why
            .as_deref()
            .is_some_and(|w| w.contains("set aside by the owner")),
        "{held:?}"
    );
    // The same problem seen again files against it, and starts nothing new.
    let cause = cause_of(&r, &top);
    let later = theseus_protocol::now_unix_ms() + 3_600_000;
    for (kind, data) in causes() {
        let mut row = crate::ledger::LedgerRow::named(kind, None, None, data);
        row.at_unix_ms = later - 1_000;
        r.core.store.append_ledger(&row).unwrap();
    }
    r.core.backlog_night(later).unwrap();
    assert_eq!(items_of(&r, &cause), 1, "no second item for its cause");
    assert_eq!(r.state(&top), TaskState::WaitingHuman);
    // Said once more: a set-aside of one already aside changes nothing.
    assert!(!aside(&r, &top).changed);
    // Bring it back: it is ready, and with a free slot the pipeline starts it.
    let back = r
        .core
        .self_retry(
            &SelfRetryParams {
                item: top.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    assert_eq!(back.item, top);
    assert_eq!(r.state(&top), TaskState::Proposed);
}

/// A drop forbids the cause for good; a set-aside leaves the cause free, which is the whole point
/// of it: the two differ in exactly that.
#[tokio::test]
async fn a_drop_forbids_the_cause_and_a_set_aside_does_not() {
    let r = rig_with(1, 30.0);
    let ready = r.view().ready;
    let (kept, dropped) = (ready[0].item.clone(), ready[1].item.clone());
    aside(&r, &kept);
    r.core
        .self_drop(
            &SelfDropParams {
                item: dropped.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    let later = theseus_protocol::now_unix_ms() + 3_600_000;
    for (kind, data) in causes() {
        let mut row = crate::ledger::LedgerRow::named(kind, None, None, data);
        row.at_unix_ms = later - 1_000;
        r.core.store.append_ledger(&row).unwrap();
    }
    let night = r.core.backlog_night(later).unwrap();
    assert!(
        night.kept_out >= 1,
        "the dropped cause is kept out: {night:?}"
    );
    let (c_kept, c_dropped) = (cause_of(&r, &kept), cause_of(&r, &dropped));
    assert_eq!(
        items_of(&r, &c_kept),
        1,
        "the set-aside cause files against its item"
    );
    assert_eq!(
        items_of(&r, &c_dropped),
        1,
        "the dropped cause is never filed again"
    );
}

/// A set-aside build stops, as a dropped one does, and an item that is over cannot be set aside.
#[tokio::test]
async fn an_item_being_built_stops_and_a_closed_one_is_refused() {
    let mut r = rig_with(1, 30.0);
    r.tick().await;
    let building = r.fake.starts.lock().unwrap()[0].0.clone();
    assert_eq!(r.state(&building), TaskState::InProgress);
    aside(&r, &building);
    assert_eq!(r.state(&building), TaskState::WaitingHuman);
    r.tick().await;
    let stopped = r.rows("self.pipeline.ended");
    assert_eq!(stopped.len(), 1, "its build was stopped: {stopped:?}");
    assert_eq!(stopped[0]["item"], building.as_str());
    // A closed item is refused with words.
    r.grace = std::time::Duration::ZERO;
    let done = r.view().ready[0].item.clone();
    let mut t = crate::task_graph::get(&r.core.store, &done)
        .unwrap()
        .unwrap();
    t.state = TaskState::Done;
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    let refused = r.core.self_drop(
        &SelfDropParams {
            item: done,
            set_aside: true,
            ..Default::default()
        },
        &Answerer::from("the CLI"),
    );
    let words = format!("{:#}", refused.unwrap_err());
    assert!(
        words.contains("only an open item can be set aside"),
        "{words}"
    );
}
