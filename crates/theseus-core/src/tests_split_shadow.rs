//! `split.v1` in shadow (theseus-z9mze.5): Jev is asked, with the inbound batch, whether the work a
//! message asks for divides by area, and the answer is logged with the turn beside the static
//! fan-out cue's row. It decides nothing: the cue fires on its words whatever Jev says. The rig is
//! `tests_fan_out_cue`'s.

use std::time::Duration;

use theseus_judge::fake::FakeJev;

use crate::ledger::LedgerRow;
use crate::tests_injection_tiers::tiered;
use crate::tests_judge::kinds;
use crate::tests_route::{mode, turn};

/// The `split.v1` judgments, in the order they were recorded.
fn judged(r: &crate::tests_route::Rig) -> Vec<LedgerRow> {
    kinds(&r.core.store, "judge.call")
        .into_iter()
        .filter(|j| j.data["pack"] == "split.v1")
        .collect()
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
