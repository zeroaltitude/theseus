//! A recall after an idle stretch has the meaning search (issue 21 of 1.0.7),
//! against `tests_recall_standin`'s tender. `Memory::warm_on_message` sends the warm at a person's message, and
//! recall's deadline extends to `LOAD_BOUND` only while the tender says it
//! is loading.

use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config::MemoryMode;
use crate::recall::loading::LOAD_BOUND;
use crate::recall::WORDS_ONLY;
use crate::tests_recall::turn;
use crate::tests_recall_standin::{recalled, rig, two_on, Model, State};

/// The owner's case: the model was unloaded, so a recall used to answer by
/// words alone. A person's message loads it at its arrival, and recall's
/// wait covers the load, so the recall has vectors, and the turn waited no
/// longer than the load and the bound.
#[tokio::test]
async fn a_recall_after_an_idle_stretch_has_the_meaning_search() {
    let model = Model::new(State::Unloaded, Duration::from_millis(700));
    let (r, now, seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    let (m, took) = recalled(&r, &now).await;
    assert_eq!(m.outcome, "ran", "{:?}", m.skipped);
    assert!(m.skipped.is_empty(), "{:?}", m.skipped);
    assert!(m.sources.contains_key("vector"), "{:?}", m.sources);
    assert_eq!(model.loads.load(Ordering::SeqCst), 1, "one load");
    assert!(
        model.warms.load(Ordering::SeqCst) >= 1,
        "the message's arrival asked the tender to load"
    );
    assert!(took < LOAD_BOUND + Duration::from_secs(2), "{took:?}");
    let asked = seen.lock().unwrap().clone();
    let whole: Vec<u64> = asked
        .iter()
        .filter(|p| p.sources.iter().any(|s| s == "vector"))
        .map(|p| p.wait_ms)
        .collect();
    assert_eq!(
        whole,
        [LOAD_BOUND.as_millis() as u64],
        "the whole query may wait"
    );
    assert!(
        asked
            .iter()
            .filter(|p| !p.sources.iter().any(|s| s == "vector"))
            .all(|p| p.wait_ms == 0),
        "the words' query never waits"
    );
}

/// The message's warm lost (or the tender asked only by the recall's own
/// query): the recall's own query starts the load, its deadline passes, the
/// tender says it is loading, and the wait goes on to the bound.
#[tokio::test]
async fn a_recall_whose_load_began_with_its_own_query_waits_for_it_too() {
    let model = Model::with(
        State::Unloaded,
        Duration::from_millis(700),
        false,
        Duration::ZERO,
    );
    let (r, now, _seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    let (m, _) = recalled(&r, &now).await;
    assert_eq!(m.outcome, "ran", "{:?}", m.skipped);
    assert!(m.sources.contains_key("vector"), "{:?}", m.sources);
}

/// A model that is loaded and slow to embed keeps the ordinary deadline: the
/// extension is for a load, not for a slow embedding. The same holds when the
/// tender does not answer the question.
#[tokio::test]
async fn a_slow_embedding_or_a_tender_that_does_not_say_keeps_the_ordinary_deadline() {
    for warm_up in [true, false] {
        let model = Model::with(
            State::Loaded,
            Duration::from_millis(700),
            true,
            Duration::from_secs(30),
        );
        let (r, now, _seen, _dir) = rig(&model, warm_up, 250, MemoryMode::Live);
        let (m, took) = recalled(&r, &now).await;
        assert_eq!(m.outcome, WORDS_ONLY, "{:?}", m.skipped);
        let why = "the vector search had not answered within 250 ms; the words alone ranked";
        assert_eq!(m.skipped.get("vector").map(String::as_str), Some(why));
        assert!(
            took < Duration::from_millis(1_800),
            "warm_up {warm_up}: held {took:?} past the deadline's 250 ms"
        );
    }
}

/// A load that never finishes costs the turn the bound and no more: the
/// words alone answer, and the row says the bound, not the ordinary deadline.
#[tokio::test]
async fn a_load_that_never_finishes_costs_the_turn_the_bound_and_no_more() {
    let model = Model::new(State::Unloaded, Duration::from_secs(3_600));
    let (r, now, _seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    let (m, took) = recalled(&r, &now).await;
    assert_eq!(m.outcome, WORDS_ONLY, "{:?}", m.skipped);
    let why = "the vector search had not answered within 2000 ms; the words alone ranked";
    assert_eq!(m.skipped.get("vector").map(String::as_str), Some(why));
    assert_eq!(m.admitted.len(), 1, "the otters' note ranked by its words");
    assert!(took >= LOAD_BOUND - Duration::from_millis(100), "{took:?}");
    assert!(took < LOAD_BOUND + Duration::from_secs(3), "{took:?}");
}

/// A model that will not load answers at once with its words, saying why,
/// and a tender that is down is no wait at all.
#[tokio::test]
async fn a_load_that_fails_or_a_tender_that_is_down_never_holds_the_turn() {
    let model = Model::new(State::Refused, Duration::ZERO);
    let (r, now, _seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    let (m, took) = recalled(&r, &now).await;
    assert!(
        m.skipped["vector"].starts_with("bm25_only"),
        "{:?}",
        m.skipped
    );
    assert!(took < Duration::from_millis(1_800), "{took:?}");

    let model = Model::new(State::Unloaded, Duration::from_millis(700));
    let (r, now, _seen, _dir) = rig(&model, false, 250, MemoryMode::Live);
    let gone = tempfile::tempdir().unwrap();
    r.core
        .runner
        .memory
        .set_ask_two(two_on(gone.path().join("no-such.sock")));
    let (m, took) = recalled(&r, &now).await;
    assert_eq!(m.outcome, "unavailable", "{:?}", m.skipped);
    assert!(took < Duration::from_millis(1_800), "{took:?}");
}

/// With recall off, a message asks the tender nothing.
#[tokio::test]
async fn a_message_with_recall_off_warms_nothing() {
    let model = Model::new(State::Unloaded, Duration::from_millis(700));
    let (r, now, _seen, _dir) = rig(&model, true, 250, MemoryMode::Off);
    turn(&r.core, &now, "Where do the otters den?").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(model.warms.load(Ordering::SeqCst), 0);
    assert_eq!(model.loads.load(Ordering::SeqCst), 0);
}
