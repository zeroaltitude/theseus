//! A cap on attempts per item (issue 6, condition 4): the try that reaches it moves the item
//! to the failed list with every try's reason, once, and its cause is not filed again by the
//! night until the owner's retry.

use std::time::Duration;

use theseus_protocol::tasks::{TaskEvidence, TaskRecord, TaskState};

use super::tests::{rig_with, Rig};
use crate::rsi::backlog::attempts::{CAPPED, RETRIED};

fn task(r: &Rig, item: &str) -> TaskRecord {
    crate::task_graph::get(&r.core.store, item)
        .unwrap()
        .unwrap()
}

/// Give `item` evidence as its build and gates leave it.
fn give(r: &Rig, item: &str, evidence: &[(&str, &str)]) {
    let mut t = task(r, item);
    for (identity, note) in evidence {
        t.evidence.push(TaskEvidence {
            node: None,
            identity: (*identity).into(),
            note: Some((*note).into()),
            by: "test".into(),
            at_ms: theseus_protocol::now_unix_ms(),
        });
    }
    t.version += 1;
    r.core
        .store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
}

fn cause_of(t: &TaskRecord) -> String {
    t.evidence
        .iter()
        .find_map(|e| e.identity.strip_prefix("cause:"))
        .expect("a filed item names its cause")
        .to_string()
}

/// Items of the cause `key`, whatever their state.
fn items_of(r: &Rig, key: &str) -> Vec<TaskRecord> {
    crate::task_graph::all(&r.core.store)
        .unwrap()
        .into_iter()
        .filter(|t| {
            t.evidence
                .iter()
                .any(|e| e.identity == format!("cause:{key}"))
        })
        .collect()
}

fn markers(t: &TaskRecord) -> usize {
    t.evidence.iter().filter(|e| e.identity == CAPPED).count()
}

/// KEY: three red tries cap an item with every reason in the marker, once; two do not, and
/// the night files the two-try cause again but leaves the capped one for the owner.
#[tokio::test]
async fn an_item_at_its_cap_fails_once_and_its_cause_waits_for_the_owner() {
    let mut r = rig_with(2, 30.0);
    r.tick().await;
    let (a, b) = {
        let s = r.fake.starts.lock().unwrap();
        (s[0].0.clone(), s[1].0.clone())
    };
    give(&r, &a, &[("gate:red", "its gate ended red in suite")]);
    give(&r, &a, &[("gate:red", "its gate ended red in clippy")]);
    for why in ["in suite", "in clippy", "in the door"] {
        give(
            &r,
            &b,
            &[("gate:red", &format!("its gate ended red {why}"))],
        );
    }
    r.grace = Duration::ZERO;
    r.tick().await;
    assert_eq!(
        (r.state(&a), r.state(&b)),
        (TaskState::Failed, TaskState::Failed)
    );
    let (ta, tb) = (task(&r, &a), task(&r, &b));
    assert_eq!(markers(&ta), 0, "two tries are inside the cap of three");
    assert_eq!(markers(&tb), 1);
    let marker = tb.evidence.iter().find(|e| e.identity == CAPPED).unwrap();
    let note = marker.note.as_deref().unwrap();
    assert!(note.starts_with("failed after 3 tries"), "{note}");
    for why in ["in suite", "in clippy", "in the door"] {
        assert!(note.contains(why), "every try's reason: {note}");
    }
    // Another tick says nothing more.
    r.tick().await;
    assert_eq!(markers(&task(&r, &b)), 1, "once");
    // The night: the two-try cause files again, as it always did; the capped one does not.
    let (cause_a, cause_b) = (cause_of(&ta), cause_of(&tb));
    r.core
        .backlog_night(theseus_protocol::now_unix_ms())
        .unwrap();
    assert_eq!(items_of(&r, &cause_a).len(), 2, "the control: filed again");
    assert_eq!(
        items_of(&r, &cause_b).len(),
        1,
        "capped: left for the owner"
    );
    // The owner's retry of the capped item is a fresh start: its tries no longer count.
    give(&r, &b, &[(RETRIED, "retried by the owner")]);
    assert!(!crate::rsi::backlog::attempts::capped(&task(&r, &b)));
}

/// A build that ended with no green branch and ran no gate is a try; one that ran a gate is
/// counted by its gate, not twice.
#[tokio::test]
async fn a_build_that_died_before_its_gate_is_a_try_and_one_that_ran_a_gate_is_not_counted_twice() {
    let mut r = rig_with(2, 30.0);
    r.tick().await;
    let (a, b) = {
        let s = r.fake.starts.lock().unwrap();
        (s[0].0.clone(), s[1].0.clone())
    };
    let died = (
        "ended:no-branch",
        "its build's turn ended with no green branch",
    );
    // Both had one build die with no gate, then began another.
    give(&r, &a, &[died, ("session:s2", "")]);
    give(&r, &b, &[died, ("session:s2", "")]);
    // `a`'s second build ran a gate that went red, so its end below is no extra try (2 in all);
    // `b`'s ran none, so its end is the third.
    give(&r, &a, &[("gate:red", "its gate ended red")]);
    r.grace = Duration::ZERO;
    r.tick().await;
    assert_eq!(
        (r.state(&a), r.state(&b)),
        (TaskState::Failed, TaskState::Failed)
    );
    assert_eq!(
        markers(&task(&r, &a)),
        0,
        "a: one dead build and one red gate"
    );
    assert_eq!(
        markers(&task(&r, &b)),
        1,
        "b: three builds that died with no gate"
    );
}

/// A gate that could not run says nothing about the item: not a try.
#[tokio::test]
async fn a_gate_that_could_not_run_is_not_a_try() {
    let mut r = rig_with(1, 30.0);
    r.tick().await;
    let a = r.fake.starts.lock().unwrap()[0].0.clone();
    give(
        &r,
        &a,
        &[
            ("gate:unrunnable", "the machine had no room"),
            ("gate:unrunnable", "the machine had no room"),
            ("gate:unrunnable", "the machine had no room"),
        ],
    );
    r.grace = Duration::ZERO;
    r.tick().await;
    assert_eq!(markers(&task(&r, &a)), 0);
}
