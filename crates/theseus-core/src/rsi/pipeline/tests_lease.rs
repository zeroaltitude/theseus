//! The pipeline's tick leaves a tree alone while a gate runs in it (issue 6, condition 7),
//! though the record says its item is over, and closes it on the tick after the gate ends.

use theseus_protocol::rsi::SelfDropParams;

use super::tests::rig_with;
use crate::approval::Answerer;
use crate::rsi::tree::{self, Tree};
use crate::rsi::tree_lease::Lease;

/// KEY: a dropped item's tree and build tree are kept while a gate holds them, and cleared
/// (one `cleared` row) once it lets go.
#[tokio::test]
async fn a_dropped_items_tree_waits_for_its_running_gate() {
    let r = rig_with(1, 30.0);
    r.tick().await;
    let item = r.fake.starts.lock().unwrap()[0].0.clone();
    let place = &r.f.place;
    let t: Tree = tree::find(place, &item).expect("the start opened its tree");
    std::fs::create_dir_all(t.target.join("debug")).unwrap();
    // The owner drops the item while its gate runs.
    let lease = Lease::take(&[&t.dir, &t.target]);
    r.core
        .self_drop(
            &SelfDropParams {
                item: item.clone(),
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    r.tick().await;
    assert!(
        t.dir.exists() && t.target.exists(),
        "the gate still has its trees"
    );
    assert!(r.rows("self.pipeline.cleared").is_empty());
    // The gate ends; the next tick closes them.
    drop(lease);
    r.tick().await;
    assert!(!t.dir.exists() && !t.target.exists(), "now they go");
    assert_eq!(r.rows("self.pipeline.cleared").len(), 1);
}
