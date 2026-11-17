//! A call that failed because its own session's working copy had been closed is no fault of the tool
//! (issue 20): the night does not file it under the tool's cause. On 2026-11-17 the loop built a fix
//! twice, and spent two reviews, for `proc.run` "failing", when the rows were calls made in a
//! working copy the pipeline had removed under a live session.
//!
//! One planted row of each kind, through the whole night: the marked one files nothing; the same
//! producer's row with no mark (a real fault of the tool) and a spawn failure are filed as before.

use super::*;

const CAUSE: &str = "action.failed:harness:proc.run";

/// A failed `proc.run` of the harness, with the ledger's `failure` word when it has one.
fn row(c: &Core, failure: Option<&str>, n: u64) {
    for i in 0..n {
        let mut d = json!({"producer": "harness:proc.run", "duration_ms": 0, "outcome": "failed",
                           "correlation_id": format!("act_tree_{i}"),
                           "detail": {"error": "working directory /state/self/trees/x does not exist"}});
        if let Some(f) = failure {
            d["failure"] = json!(f);
        }
        seed(c, "action.failed", d, tick());
    }
}

fn filed(c: &Core) -> Vec<String> {
    listed(c).into_iter().map(|i| i.cause).collect()
}

/// KEY: the marked failure files nothing, however often it happens in a night.
#[test]
fn a_call_that_failed_because_its_tree_was_gone_files_nothing() {
    let c = acting(0, 20);
    row(&c, Some(theseus_kernel::TREE_GONE), 5);
    let night = c.backlog_night(step_now()).unwrap();
    assert!(night.filed.is_empty(), "{:?} {:?}", night.filed, filed(&c));
    assert!(!filed(&c).iter().any(|k| k == CAUSE), "{:?}", filed(&c));
}

/// The same producer's unmarked failures are still a fault of the tool (a failure that names no
/// class keeps its old cause), and so is a spawn failure.
#[test]
fn the_same_producers_unmarked_failure_and_a_spawn_failure_are_still_filed() {
    let c = acting(0, 20);
    row(&c, None, 3);
    c.backlog_night(step_now()).unwrap();
    assert!(filed(&c).iter().any(|k| k == CAUSE), "{:?}", filed(&c));

    let c = acting(0, 20);
    row(&c, Some("spawn"), 3);
    c.backlog_night(step_now()).unwrap();
    assert!(
        filed(&c)
            .iter()
            .any(|k| k == "action.failed:harness:proc.run:spawn"),
        "{:?}",
        filed(&c)
    );
}

/// A marked row and a plain one in one night: only the plain one is a cause.
#[test]
fn a_night_with_both_files_only_the_real_fault() {
    let c = acting(0, 20);
    row(&c, Some(theseus_kernel::TREE_GONE), 4);
    row(&c, None, 2);
    c.backlog_night(step_now()).unwrap();
    let got = listed(&c);
    let ours: Vec<_> = got.iter().filter(|i| i.cause == CAUSE).collect();
    assert_eq!(ours.len(), 1, "{:?}", filed(&c));
    // Its evidence is the two plain rows, not the four marked ones.
    let rows = ours[0]
        .evidence
        .iter()
        .filter(|e| e.starts_with("ledger:"))
        .count();
    assert_eq!(rows, 2, "{:?}", ours[0].evidence);
}

fn step_now() -> u64 {
    std::thread::sleep(std::time::Duration::from_millis(3));
    tick()
}
