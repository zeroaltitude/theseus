//! `split.v1` in shadow (theseus-z9mze.5): Jev is asked, with the inbound batch, whether the work a
//! message asks for divides by area, and the answer is logged with the turn beside the static
//! fan-out cue's row. It decides nothing: the cue fires on its words whatever Jev says. The rig is
//! `tests_fan_out_cue`'s.

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

/// The key test. Two turns with the same open-ended ask in a class that may fan out, Jev answering
/// `one_area` to the first and `splits_by_area` to the second: each is a `split.v1` judgment in
/// shadow, on the turn's own id, beside that turn's `fan_out.cue` row, and the cue fired on both,
/// the same rows but for the turn. The answer moved nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_split_answer_is_logged_beside_the_cue_and_moves_nothing() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "deep_coding", 0.95);
    let r = tiered(&jev, 2, true, |_| {});
    say(&jev, "one_area", 0.95);
    let first = turn(&r.core, None, OPEN, None).await;
    // The batch is asked off the turn's path: the first answer is in before the next is scripted.
    until_judged(&r, 1).await;
    say(&jev, "splits_by_area", 0.95);
    let second = turn(&r.core, Some(&first.session_id), OPEN, None).await;
    until_judged(&r, 2).await;
    let got = judged(&r);
    assert_eq!(got.len(), 2, "one a message");
    for (j, turn_id, want) in [
        (&got[0], &first.turn_id, "one_area"),
        (&got[1], &second.turn_id, "splits_by_area"),
    ] {
        assert_eq!(j.data["mode"], "shadow", "it acts on nothing");
        assert_eq!(j.data["point"], "inbound");
        assert_eq!(j.data["outcome"]["outcome"], "answered", "{}", j.data);
        assert_eq!(
            j.turn_id.as_deref(),
            Some(turn_id.as_str()),
            "the turn's own"
        );
        assert_eq!(choice(j), want);
    }

    let cues = kinds(&r.core.store, "fan_out.cue");
    assert_eq!(cues.len(), 2);
    for (c, turn_id) in [(&cues[0], &first.turn_id), (&cues[1], &second.turn_id)] {
        assert_eq!(c.turn_id.as_deref(), Some(turn_id.as_str()), "the join key");
        assert_eq!(
            c.data["fired"], true,
            "the words decide, not Jev: {}",
            c.data
        );
    }
    assert_eq!(cues[0].data["words"], cues[1].data["words"]);
    // It rides the batch: the same request as classify's questions, none of its own.
    let seen = jev.seen();
    let with_split: Vec<_> = seen
        .iter()
        .filter(|s| s.body["questions"].get("split.v1/split").is_some())
        .collect();
    assert!(!with_split.is_empty(), "asked");
    assert!(
        with_split
            .iter()
            .all(|s| s.body["questions"].get("classify.v4/kind").is_some()),
        "in classify's request"
    );
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
