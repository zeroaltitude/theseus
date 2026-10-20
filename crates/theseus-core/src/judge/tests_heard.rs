//! `heard.v1` live (theseus-s3dwj): a voice call's engine asks, the verdict
//! comes back over the call's own wait, and every call is a `judge.call` row
//! scoped `judge:heard`, a late or a below-the-band one too.

use std::time::{Duration, Instant};

use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted as Jev};

use super::heard::{HeardInput, HeardOverlap, PACK};
use crate::config::PackMode;
use crate::tests_judge::{off, rig_with, texts};

fn input(text: &str) -> HeardInput {
    HeardInput {
        transcript: text.into(),
        said: vec!["Theseus here. Go ahead.".into()],
        overlap: HeardOverlap::Join,
        after_start_ms: Some(300),
        after_end_ms: None,
        length_ms: 700,
        confidence: Some(0.62),
        echo_prone: false,
        place: "private".into(),
    }
}

/// The `judge:heard` rows, waited for on the runtime's timer.
async fn rows(store: &crate::store::Store, n: usize) -> Vec<Value> {
    let t0 = Instant::now();
    loop {
        let rows: Vec<Value> = store
            .scope_after("judge:heard", 0)
            .unwrap()
            .into_iter()
            .map(|r| r.decode::<crate::ledger::LedgerRow>().unwrap().data)
            .collect();
        if rows.len() >= n {
            return rows;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "{} of {n} rows",
            rows.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// An echo Jev is sure of comes back acting, with the judgment's id; its row
/// holds the answer, the state's blob (the transcript and the sentence it
/// may echo), and the call's own time.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_echo_jev_is_sure_of_comes_back_acting_and_is_recorded() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "heard",
        Jev::Choice {
            option: "echo".into(),
            confidence: 0.9,
        },
    );
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let svc = r.core.runner.judge.clone();
    let v = svc
        .judge_heard("ses_voice", input("seus here"))
        .await
        .unwrap();
    assert_eq!((v.choice.as_str(), v.acts), ("echo", true));
    assert!(v.p > 0.85, "{v:?}");
    let rows = rows(&r.core.store, 1).await;
    let d = &rows[0];
    assert_eq!(d["id"].as_str(), Some(v.judgment.as_str()));
    assert_eq!(
        (d["pack"].as_str(), d["mode"].as_str()),
        (Some(PACK), Some("live"))
    );
    assert_eq!(d["context"]["purpose"], "voice");
    assert_eq!(d["context"]["heard"]["taken"], true);
    assert!(d["context"]["heard"]["ms"].is_u64());
    let digest = d["state"]["sha256"].as_str().unwrap();
    let state = std::fs::read_to_string(r.core.store.blobs().path(digest)).unwrap();
    assert!(
        state.contains("seus here") && state.contains("Theseus here. Go ahead."),
        "{state}"
    );
}

/// An answer in the confirm band decides nothing, and says so: the engine
/// keeps its rules' verdict.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_in_the_confirm_band_does_not_act() {
    let jev = FakeJev::start().unwrap();
    jev.script(
        "heard",
        Jev::Choice {
            option: "echo".into(),
            confidence: 0.6,
        },
    );
    let r = rig_with(texts(1), Some(&jev), |_| {});
    let svc = r.core.runner.judge.clone();
    let v = svc
        .judge_heard("ses_voice", input("seus here"))
        .await
        .unwrap();
    assert_eq!((v.choice.as_str(), v.acts), ("echo", false), "{v:?}");
    assert_eq!(rows(&r.core.store, 1).await.len(), 1, "still recorded");
}

/// With the pack off, or the judge off, nothing is asked and nothing waits.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_pack_off_nothing_is_asked() {
    let jev = FakeJev::start().unwrap();
    let r = rig_with(texts(1), Some(&jev), |c| {
        c.judge.packs.insert(PACK.into(), off());
    });
    let svc = r.core.runner.judge.clone();
    assert!(svc
        .judge_heard("ses_voice", input("seus here"))
        .await
        .is_none());
    assert!(jev.seen().is_empty());
}

#[test]
fn heard_is_wired_live_with_no_action() {
    assert_eq!(super::wired_mode(PACK), PackMode::Live);
    let p = theseus_judge::pack::by_name(PACK).unwrap();
    assert_eq!(p.action, theseus_judge::pack::Action::None);
    assert_eq!(p.point, theseus_judge::pack::Point::Voice);
    assert_eq!(p.questions.len(), 1);
}
