//! A transient failure of Jev's (a dropped connection, a timeout, an
//! overload) does not lose the candidate: the backfill's pass asks again
//! once, after a pause, where it used to leave the failed judgment as the
//! candidate's only word (the mark moved on and the name counted as asked).
//! Names here are invented.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde_json::json;
use theseus_judge::fake::{FakeJev, FakeMode};

use super::tests::{answer, import, people_on, rows, script, until_rows};
use crate::tests_categorize::rig;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_judgment_that_fails_in_transit_is_asked_again_once() {
    let jev = FakeJev::start().unwrap();
    script(&jev);
    let r = rig(Some(&jev), 0, people_on);
    import(&r.core);
    // The first session's extraction: Wren and Kestrel reach Jev.
    r.fake.script.lock().unwrap().push_back(answer(json!([
        {"name": "Wren Halloway", "handles": [], "role_line": "", "evidence": ["L2"]},
        {"name": "Kestrel", "handles": [], "role_line": "", "evidence": ["L3"]}
    ])));
    // The second session names no one.
    r.fake
        .script
        .lock()
        .unwrap()
        .push_back(answer(json!([])));
    jev.set_mode(FakeMode::Down);

    let run = r.core.import_people_propose(&theseus_protocol::import::ImportPeopleParams {
        tag: "tern-2026-05".into(),
        dry_run: false,
        propose: true,
        cap_usd: Some(5.0),
    });
    // Jev comes back once both first calls have been refused.
    let back = async {
        let t0 = Instant::now();
        while jev.connections() < 2 {
            assert!(t0.elapsed() < Duration::from_secs(20), "no call reached Jev");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        jev.set_mode(FakeMode::Up);
    };
    let (done, ()) = tokio::join!(run, back);
    let report = done.unwrap().propose.unwrap();
    assert!(report.stopped.is_none(), "{report:?}");

    let calls = until_rows(&r.core.store, "judge.call", 4).await;
    let by = |outcome: &str| -> Vec<&crate::ledger::LedgerRow> {
        calls
            .iter()
            .filter(|c| c.data["outcome"]["outcome"] == outcome)
            .collect()
    };
    assert_eq!(by("failed").len(), 2, "the two refused calls: {calls:#?}");
    let answered = by("answered");
    assert_eq!(answered.len(), 2, "each asked once more, and answered");
    let firsts: HashSet<&str> = by("failed")
        .iter()
        .map(|c| c.data["id"].as_str().unwrap())
        .collect();
    let retried: HashSet<&str> = answered
        .iter()
        .map(|c| c.data["context"]["retry_of"].as_str().unwrap())
        .collect();
    assert_eq!(retried, firsts, "a retry names the failure it answers");

    // The extraction's row lists the judgments that stand: the retries'.
    let x = &rows(&r.core.store, "people.extracted")[0];
    let listed: HashSet<&str> = x.data["judgments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let standing: HashSet<&str> = answered
        .iter()
        .map(|c| c.data["id"].as_str().unwrap())
        .collect();
    assert_eq!(listed, standing, "{x:?}");
}
