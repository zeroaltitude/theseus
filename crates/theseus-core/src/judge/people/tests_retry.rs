//! A `people.v1` judgment that fails for a transient reason is asked once
//! more (judge.failed:people.v1): the backfill marks a session done after
//! one pass and a failed judgment counts as asked, so before, a name whose
//! call timed out or hit a dead network was never judged. A failure that a
//! second ask cannot mend is not repeated. Every name here is invented.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, FakeMode};

use super::tests::{answer, import, people_on, propose, rows, script, until_rows};
use crate::tests_categorize::rig;

/// The first session's extraction (Wren and Kestrel get past the
/// exclusions: two judgments), then the second session's, with no one.
fn script_extractions(fake: &crate::provider::FakeProvider) {
    let mut s = fake.script.lock().unwrap();
    s.push_back(answer(json!([
        {"name": "Wren Halloway", "handles": [], "role_line": "Takes the north gauge readings.",
         "evidence": ["L2"]},
        {"name": "Kestrel", "handles": [], "role_line": "", "evidence": ["L3"]}
    ])));
    s.push_back(answer(json!([])));
}

fn ids(rows: &[crate::ledger::LedgerRow], outcome: &str) -> BTreeSet<String> {
    rows.iter()
        .filter(|r| r.data["outcome"]["outcome"] == outcome)
        .map(|r| r.data["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_failed_in_transit_is_asked_again_and_the_retry_stands() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    script_extractions(&r.fake);

    // Jev is down for the first calls, and back before the retries.
    jev.set_mode(FakeMode::Down);
    let core = r.core.clone();
    let run = tokio::spawn(async move { propose(&core, false, 5.0).await });
    let t0 = Instant::now();
    while jev.connections() < 2 {
        assert!(t0.elapsed() < Duration::from_secs(20), "no first calls");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    jev.set_mode(FakeMode::Up);
    let report = run.await.unwrap();
    assert_eq!(report.judged, 2, "{report:?}");

    let calls = until_rows(&r.core.store, "judge.call", 4).await;
    let failed = ids(&calls, "failed");
    let answered = ids(&calls, "answered");
    assert_eq!((failed.len(), answered.len()), (2, 2), "{calls:#?}");
    // Each retry names the failure it answers, and is its own judgment.
    let answers: Vec<_> = calls
        .iter()
        .filter(|r| r.data["outcome"]["outcome"] == "answered")
        .collect();
    for a in &answers {
        let of = a.data["context"]["retry_of"].as_str().expect("retry_of");
        assert!(failed.contains(of), "{of} is a failed judgment");
    }
    let named: BTreeSet<String> = answers
        .iter()
        .map(|a| a.data["context"]["retry_of"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(named, failed, "each failure is answered once");
    // The extraction's row lists the judgments that stand: the retries'.
    let listed: BTreeSet<String> = rows(&r.core.store, "people.extracted")
        .iter()
        .flat_map(|x| x.data["judgments"].as_array().cloned().unwrap_or_default())
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert_eq!(listed, answered);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_a_second_ask_cannot_mend_is_not_repeated() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    script_extractions(&r.fake);

    jev.set_mode(FakeMode::Status(400));
    let report = propose(&r.core, false, 5.0).await;
    assert_eq!(report.judged, 2, "{report:?}");
    let calls = until_rows(&r.core.store, "judge.call", 2).await;
    // Past the pause a retry would have waited.
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    let calls_after = rows(&r.core.store, "judge.call");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls_after.len(), 2, "a refused request is not asked again");
    assert_eq!(ids(&calls_after, "failed").len(), 2);
}
