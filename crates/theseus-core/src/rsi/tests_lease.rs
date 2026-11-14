//! A tree stays while a gate runs in it (issue 6, condition 7): the gate itself takes the
//! lease, and a close waits for it.

use super::tests_tree::fixture_with;
use super::tree::{self, Verdict};
use super::tree_lease::{held, Lease};

/// A gate that runs until `<state>/release` exists, as a half-hour gate runs.
const WAITING_GATE: &str = r#"#!/bin/bash
cd "${THESEUS_GATE_ROOT}"
while [ ! -f "$THESEUS_GATE_ROOT/../../../release" ]; do sleep 0.05; done
echo "gate: ok"
"#;

/// KEY: while the gate runs, closing its tree is refused and the tree is there; once the gate
/// ends the same close works. Without the lease the first close removes the tree under the gate.
#[tokio::test]
async fn a_tree_is_not_closed_under_a_running_gate() {
    let f = fixture_with(WAITING_GATE);
    let t = tree::open(&f.place, "mast", 1_792_000_000_000).unwrap();
    let (place, tree_for_gate) = (f.place.clone(), t.clone());
    let gate = tokio::spawn(async move { tree::gate(&place, &tree_for_gate).await });
    let started = std::time::Instant::now();
    while held(&[&t.dir]).is_none() {
        assert!(
            started.elapsed().as_secs() < 20,
            "the gate never took its lease"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let refused = tree::close(&f.place, &t).expect_err("a gate is running in it");
    assert!(
        format!("{refused:#}").contains("a gate is still running"),
        "{refused:#}"
    );
    assert!(t.dir.exists(), "the tree is still there for the gate");
    std::fs::write(f.place.state.join("release"), "").unwrap();
    let run = gate.await.unwrap().unwrap().ran().unwrap();
    assert_eq!(run.verdict, Verdict::Green, "{}", run.tail);
    assert!(
        held(&[&t.dir, &t.target]).is_none(),
        "the lease ends with the gate"
    );
    tree::close(&f.place, &t).unwrap();
    assert!(!t.dir.exists());
}

/// Leases count: two holders of one path, and the path is free only when both are gone.
#[test]
fn a_lease_is_released_by_its_own_drop_only() {
    let p = std::path::PathBuf::from("/nowhere/lease-test-tree");
    let a = Lease::take(&[&p]);
    let b = Lease::take(&[&p]);
    drop(a);
    assert!(held(&[&p]).is_some(), "the second holder still holds it");
    drop(b);
    assert!(held(&[&p]).is_none());
}
