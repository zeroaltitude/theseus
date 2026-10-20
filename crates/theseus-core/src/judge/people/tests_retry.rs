//! A `people.v1` judgment that fails for a transient reason is asked again
//! once (the Oct 8 backfill lost 12 of its 9,081 to timeouts and an
//! overload, and a failed judgment counts as asked, so none was ever asked
//! again). A fake model and a fake Jev; every name is invented.

use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_protocol::import::ImportPeopleParams;

use super::tests::{import, people_on, rows, script, script_extractions, until_rows, TAG};
use crate::tests_categorize::rig;

/// Jev is down for the first two calls and back before the retry: two calls
/// failed, two retries answered and name the failures they answer, and the
/// extraction's row lists the retries, not the failures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_failed_transiently_is_asked_again_and_the_row_lists_the_retry() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    script_extractions(&r.fake);
    r.core
        .runner
        .judge
        .retry_pause
        .set(Duration::from_millis(1500))
        .unwrap();
    jev.set_mode(FakeMode::Down);

    let core = r.core.clone();
    let run = tokio::spawn(async move {
        core.import_people_propose(&ImportPeopleParams {
            tag: TAG.into(),
            dry_run: false,
            propose: true,
            cap_usd: Some(0.4),
        })
        .await
        .unwrap()
    });
    // Both first calls refused: Jev is back for the retries.
    let t0 = Instant::now();
    while jev.connections() < 2 {
        assert!(t0.elapsed() < Duration::from_secs(20), "waited for 2 calls");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    jev.set_mode(FakeMode::Up);
    let report = run.await.unwrap().propose.unwrap();
    assert_eq!(report.judged, 2, "{report:?}");

    let calls = until_rows(&r.core.store, "judge.call", 4).await;
    assert_eq!(calls.len(), 4, "two failed, two retries");
    let outcome = |c: &crate::ledger::LedgerRow| c.data["outcome"]["outcome"].clone();
    let failed: Vec<_> = calls
        .iter()
        .filter(|c| outcome(c) == json!("failed"))
        .collect();
    let retries: Vec<_> = calls
        .iter()
        .filter(|c| c.data["context"].get("retry_of").is_some())
        .collect();
    assert_eq!(failed.len(), 2, "{calls:?}");
    assert_eq!(retries.len(), 2, "{calls:?}");
    for c in &retries {
        assert_eq!(outcome(c), json!("answered"), "{c:?}");
        assert_eq!(c.data["context"]["retry_of_class"], json!("network"));
        assert!(
            failed
                .iter()
                .any(|f| f.data["id"] == c.data["context"]["retry_of"]),
            "a retry names the failure it answers: {c:?}"
        );
    }
    let extracted = rows(&r.core.store, "people.extracted");
    assert_eq!(extracted.len(), 1);
    let mut listed: Vec<String> =
        serde_json::from_value(extracted[0].data["judgments"].clone()).unwrap();
    let mut ids: Vec<String> = retries
        .iter()
        .map(|c| c.data["id"].as_str().unwrap().to_string())
        .collect();
    listed.sort();
    ids.sort();
    assert_eq!(listed, ids, "the row lists the judgments that stand");
}
