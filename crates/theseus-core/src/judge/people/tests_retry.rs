//! A `people.v1` judgment that fails for a transient reason is asked once
//! more (the Oct 8-9 backfill's 12 failures: timeouts and an overload, each
//! name left unjudged because the failed call counted as asked and the
//! session was marked done). Every name here is invented.

use std::time::Duration;

use theseus_judge::fake::{FakeJev, FakeMode};

use super::tests::{
    answer, billed, import, people_on, propose, rows, script, script_extractions, until_rows,
};
use crate::tests_categorize::rig;

/// Jev is down for the first call of each of the first session's two
/// candidates and back before the second: both are asked again, answered,
/// and the extraction's row lists the answers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_failed_transiently_is_asked_again_and_the_retry_stands() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    script_extractions(&r.fake);
    jev.set_mode(FakeMode::Down);
    let back = jev.clone();
    let revive = tokio::spawn(async move {
        while back.connections() < 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        back.set_mode(FakeMode::Up);
    });

    // A cap that holds one session: Wren and Kestrel are judged.
    let first = propose(&r.core, false, 0.4).await;
    revive.await.unwrap();
    assert_eq!((first.read, first.judged), (1, 2), "{first:?}");

    let calls = until_rows(&r.core.store, "judge.call", 4).await;
    let failed: Vec<_> = calls
        .iter()
        .filter(|c| c.data["outcome"]["outcome"] == "failed")
        .collect();
    let answered: Vec<_> = calls
        .iter()
        .filter(|c| c.data["outcome"]["outcome"] == "answered")
        .collect();
    assert_eq!(
        (failed.len(), answered.len()),
        (2, 2),
        "two refused, two retries answered"
    );
    for a in &answered {
        let of = a.data["context"]["retry_of"].as_str().unwrap();
        assert!(
            failed.iter().any(|f| f.data["id"] == of),
            "a retry names the failure it answers: {of}"
        );
        assert_eq!(a.data["context"]["retry_after_failure"], "network");
        assert_ne!(a.data["id"], of, "the retry is a judgment of its own");
    }
    let x = rows(&r.core.store, "people.extracted");
    let stands: Vec<&str> = x[0].data["judgments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let mut want: Vec<&str> = answered
        .iter()
        .map(|a| a.data["id"].as_str().unwrap())
        .collect();
    let mut got = stands.clone();
    want.sort();
    got.sort();
    assert_eq!(
        got, want,
        "the extraction lists the retries, not the failures"
    );
}

/// A refusal that asking again cannot change is not asked again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_failed_for_good_is_not_asked_again() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(billed(answer(serde_json::json!([
            {"name": "Wren Halloway", "handles": [], "role_line": "", "evidence": ["L2"]}
        ]))));
    jev.set_mode(FakeMode::Status(400));
    let first = propose(&r.core, false, 0.4).await;
    assert_eq!(first.read, 1, "{first:?}");
    let calls = until_rows(&r.core.store, "judge.call", 1).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        rows(&r.core.store, "judge.call").len(),
        1,
        "no second call after a 400"
    );
    assert_eq!(jev.connections(), 1);
}
