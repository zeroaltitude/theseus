//! `kind_of_thing`'s `other` answer is recorded (theseus-81zx7): at the act
//! band it is a step of the plan and a row, never a write or a card; below it,
//! nothing.

use serde_json::json;
use theseus_judge::fake::FakeJev;

use crate::tests_judge::kinds;
use crate::tests_onto::{bound, cards, rig, said};

const SAID: &str = "let's plan the Lantern Ledger migration";

/// Only the `other` answer is scripted: the plan holds one step, and its row
/// is the whole record. No category, no list, no card.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plan_with_only_other_writes_its_row() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("kind_of_thing", "other", 0.9);
    let r = rig(jev);
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    let rows = kinds(&r.core.store, "onto.filed");
    let [row] = rows.as_slice() else {
        panic!("one onto.filed row: {rows:?}")
    };
    let [step] = row.data["steps"].as_array().unwrap().as_slice() else {
        panic!("one step: {row:?}")
    };
    assert_eq!(
        (&step["kind"], &step["outcome"], &step["confidence"]),
        (&json!("kind_of_thing"), &json!("other"), &json!(0.9))
    );
    let names: Vec<&str> = step["names"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|n| n.as_str())
        .collect();
    assert!(names.contains(&"Lantern Ledger"), "{names:?}");
    assert!(names.len() <= 10);
    assert!(cards(&r.core).is_empty(), "no card");
    let o = r.core.runner.ontology.snapshot(&r.core.store).unwrap();
    assert!(o.member_list(&sid, "topic").is_none(), "nothing is filed");
    assert!(kinds(&r.core.store, "ontology.category").is_empty());
}

/// The confirm band is not the act band: nothing is recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_below_the_act_band_is_not_recorded() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("kind_of_thing", "other", 0.7);
    let r = rig(jev);
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    assert!(kinds(&r.core.store, "onto.filed").is_empty());
}

/// A listed kind's answer is not `other`: nothing is recorded either.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_listed_kind_is_not_recorded_as_other() {
    let jev = FakeJev::start().unwrap();
    jev.script_onto("kind_of_thing", "topic", 0.95);
    let r = rig(jev);
    let sid = bound(&r.core);
    said(&r, &sid, SAID).await;
    assert!(kinds(&r.core.store, "onto.filed").is_empty());
}
