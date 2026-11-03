//! `split.v1` (theseus-z9mze.5; promoted by theseus-h66v4.6): Jev is asked whether the work a message
//! asks for divides by area. Under route.v10 it is asked in the route lane and its answer is the
//! fan-out cue (before route.v10 it was a shadow pack in the inbound batch and decided nothing).

use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use crate::ledger::LedgerRow;
use crate::tests_injection_tiers::tiered;
use crate::tests_judge::kinds;
use crate::tests_route::{mode, turn};

const OPEN: &str = "Audit every mooring on the outer wall and say which were checked.";

fn say(jev: &FakeJev, option: &str, confidence: f64) {
    jev.script(
        "split",
        Jev::Choice {
            option: option.into(),
            confidence,
        },
    );
}

/// The `split.v1` judgments, in the order they were recorded.
fn judged(r: &crate::tests_route::Rig) -> Vec<LedgerRow> {
    kinds(&r.core.store, "judge.call")
        .into_iter()
        .filter(|j| j.data["pack"] == "split.v1")
        .collect()
}

async fn until_judged(r: &crate::tests_route::Rig, n: usize) {
    let t0 = Instant::now();
    while judged(r).len() < n {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{n} split judgments"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn choice(j: &LedgerRow) -> Value {
    j.data["answers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["question"] == "split")
        .unwrap_or_else(|| panic!("no split answer in {}", j.data))["answer"]["choice"]
        .clone()
}

/// Under route.v10 (theseus-h66v4.6) the split is promoted: asked in the route lane with its own
/// state beside the route pack, in the turn's mode and not shadow, and the turn waits for it, so
/// its answer is on the verdict the cue reads.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn under_route_v10_the_split_is_asked_with_the_route_pack_and_not_in_shadow() {
    let jev = FakeJev::start().unwrap();
    crate::tests_route_open::says(&jev, "sonnet", "high", 0.95);
    say(&jev, "splits_by_area", 0.95);
    let o = crate::tests_route_open::open(&jev, 1, |_| {});
    let res = turn(&o.r.core, None, OPEN, None).await;
    until_judged(&o.r, 1).await;
    let got = judged(&o.r);
    assert_eq!(got.len(), 1);
    assert_ne!(
        got[0].data["mode"], "shadow",
        "promoted: it is no shadow ask"
    );
    assert_eq!(got[0].turn_id.as_deref(), Some(res.turn_id.as_str()));
    assert_eq!(choice(&got[0]), "splits_by_area");
    // Not in classify's request any more: a state of its own, asked beside the route pack's.
    let seen = jev.seen();
    let split_req: Vec<_> = seen
        .iter()
        .filter(|s| s.body["questions"].get("split.v1/split").is_some())
        .collect();
    assert!(!split_req.is_empty());
    assert!(split_req
        .iter()
        .all(|s| s.body["questions"].get("classify.v4/kind").is_none()));
    // The effort and the task type are asked too, each with its own state.
    for q in ["effort.v1/effort", "work.v1/work"] {
        assert!(
            seen.iter().any(|s| s.body["questions"].get(q).is_some()),
            "{q} was asked"
        );
    }
    let cue = kinds(&o.r.core.store, "fan_out.cue");
    assert_eq!(cue.len(), 1);
    assert_eq!(cue[0].data["split"], "splits_by_area");
}

/// A slash command is no ask to divide: it is not judged, so nothing is asked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slash_command_is_not_asked_whether_it_splits() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = tiered(&jev, 1, true, |_| {});
    let _ = turn(&r.core, None, "/status", None).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(judged(&r).is_empty());
}
