//! A judgment of a candidate that fails for a reason that may pass (Jev
//! unreachable, a timeout, a 429, a 5xx) is asked once more. The backfill
//! marks a session done after one pass and a failed judgment counts as
//! asked, so without the retry the name was never asked again. A refusal
//! (a bad request, a bad key) is not retried. Every name here is invented.

use theseus_judge::fake::{FakeJev, FakeMode};

use super::tests::{import, people_on, propose, rows, script, script_extractions, until_rows};
use crate::tests_categorize::rig;

/// The judgments of `people.v1` in the ledger, by outcome.
fn by_outcome(store: &crate::store::Store, outcome: &str) -> Vec<serde_json::Value> {
    rows(store, "judge.call")
        .into_iter()
        .map(|r| r.data)
        .filter(|d| d["outcome"]["outcome"] == outcome)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_failed_transiently_is_asked_again_and_names_its_failure() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    jev.set_mode(FakeMode::Down);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    script_extractions(&r.fake);

    // Jev comes back once both first calls have been refused.
    let back = jev.clone();
    let comes_back = tokio::spawn(async move {
        while back.connections() < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        back.set_mode(FakeMode::Up);
    });
    let first = propose(&r.core, false, 0.4).await;
    comes_back.await.unwrap();
    assert_eq!(first.read, 1, "{first:?}");
    assert_eq!(first.judged, 2, "{first:?}");

    let all = until_rows(&r.core.store, "judge.call", 4).await;
    assert_eq!(all.len(), 4, "two failures and their two retries");
    let failed = by_outcome(&r.core.store, "failed");
    let answered = by_outcome(&r.core.store, "answered");
    assert_eq!((failed.len(), answered.len()), (2, 2), "{all:?}");
    let ids = |v: &[serde_json::Value]| -> Vec<String> {
        let mut ids: Vec<String> = v
            .iter()
            .map(|d| d["id"].as_str().unwrap().to_string())
            .collect();
        ids.sort();
        ids
    };
    let mut answers: Vec<String> = answered
        .iter()
        .map(|d| d["context"]["retry_of"].as_str().unwrap().to_string())
        .collect();
    answers.sort();
    assert_eq!(
        answers,
        ids(&failed),
        "each retry names the failure it answers"
    );
    for d in &answered {
        let again = d["context"]["candidate"]["name"].as_str().unwrap();
        let was = failed
            .iter()
            .find(|f| f["id"] == d["context"]["retry_of"])
            .unwrap();
        assert_eq!(was["context"]["candidate"]["name"], again, "the same name");
        assert_ne!(was["id"], d["id"], "a new id");
    }

    // The extraction lists the judgments that stand: the retries.
    let extracted = rows(&r.core.store, "people.extracted");
    assert_eq!(extracted.len(), 1);
    let mut listed: Vec<String> = extracted[0].data["judgments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j.as_str().unwrap().to_string())
        .collect();
    listed.sort();
    assert_eq!(listed, ids(&answered));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_judgment_is_not_asked_again() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    jev.set_mode(FakeMode::Status(400));
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    script_extractions(&r.fake);

    let first = propose(&r.core, false, 0.4).await;
    assert_eq!(first.judged, 2, "{first:?}");
    let all = until_rows(&r.core.store, "judge.call", 2).await;
    assert_eq!(all.len(), 2);
    assert_eq!(jev.connections(), 2, "one call each, none again");
    assert!(all.iter().all(|r| r.data["context"]["retry_of"].is_null()));
}
