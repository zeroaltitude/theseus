//! A recall after an idle stretch has the meaning search (issue 21 of 1.0.7),
//! against `tests_recall_standin`'s tender. `Memory::warm_on_message` sends the warm at a person's message, and
//! recall's deadline extends to `LOAD_BOUND` only while the tender says it
//! is loading.

use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::loading::LOAD_BOUND;
use crate::recall::WORDS_ONLY;
use crate::tests_recall::turn;
use crate::tests_recall_standin::{recalled, rig, rig_arm, two_on, Model, State};

/// The owner's case: the model was unloaded, so a recall used to answer by
/// words alone. A person's message loads it at its arrival, and recall's
/// wait covers the load, so the recall has vectors, and the turn waited no
/// longer than the load and the bound. On the paused clock, so the bound on
/// the turn's time is its own virtual time, which a busy machine cannot stretch.
#[tokio::test(start_paused = true)]
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
/// tender does not answer the question. On the paused clock, as above.
#[tokio::test(start_paused = true)]
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
/// Run on tokio's paused clock: the recall's deadline and the stand-in's hold
/// are both the bound, so on a real clock a busy machine decided which of the
/// two answered first, and the row said "the model is loading" instead of the
/// bound. Here the clock moves only when every task waits, so `took` is the
/// turn's own virtual time and load cannot change it.
#[tokio::test(start_paused = true)]
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
/// and a tender that is down is no wait at all. On the paused clock, as above.
#[tokio::test(start_paused = true)]
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

/// A person's message asks the tender to warm at its arrival, once, and a
/// recall whose answer comes in time asks it nothing more (the question at
/// the deadline is for a recall that has not been answered).
#[tokio::test]
async fn a_persons_message_asks_the_tender_to_warm_and_a_recall_in_time_asks_no_more() {
    let model = Model::new(State::Loaded, Duration::from_millis(700));
    let (r, now, _seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    let (m, _) = recalled(&r, &now).await;
    assert_eq!(m.outcome, "ran", "{:?}", m.skipped);
    let until = std::time::Instant::now() + Duration::from_secs(5);
    while model.warms.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        model.warms.load(Ordering::SeqCst),
        1,
        "the arrival's warm, and no question at a deadline that never passed"
    );
}

/// An arm of words alone has no vector source to load a model for, so a
/// message on it warms nothing.
#[tokio::test]
async fn a_message_on_an_arm_with_no_vector_source_warms_nothing() {
    let model = Model::new(State::Unloaded, Duration::from_millis(700));
    let (r, now, _seen, _dir) = rig_arm(&model, true, 250, MemoryMode::Live, MemoryArm::Bm25);
    turn(&r.core, &now, "Where do the otters den?").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(model.warms.load(Ordering::SeqCst), 0);
    assert_eq!(model.loads.load(Ordering::SeqCst), 0);
}

/// A turn's own recall asks the index for the hot set and the nearest groups (`fast`), and a
/// search somebody asks for by name does not: it is exact, hot and cold (issue 21 of 1.0.7).
#[tokio::test]
async fn a_turns_recall_is_fast_and_a_search_by_name_is_exact() {
    use std::sync::{Arc, Mutex};
    use theseus_protocol::index::IndexQueryParams;

    let model = Model::new(State::Loaded, Duration::from_millis(5));
    let (r, now, seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    recalled(&r, &now).await;
    let asked = seen.lock().unwrap().clone();
    assert!(!asked.is_empty());
    assert!(
        asked.iter().all(|p| p.fast),
        "a turn's recall is fast: {asked:?}"
    );
    let named: Arc<Mutex<Vec<IndexQueryParams>>> = Arc::default();
    let into = named.clone();
    r.core
        .runner
        .memory
        .set_ask(Arc::new(move |p: IndexQueryParams| {
            into.lock().unwrap().push(p);
            Box::pin(async { Err("a stand-in that only listens".to_string()) })
        }));
    let begun = r.core.runner.memory.begin(
        "Where do the otters den?".to_string(),
        None,
        5,
        MemoryArm::Baseline,
        Duration::from_millis(300),
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(begun);
    let asked = named.lock().unwrap().clone();
    assert!(!asked.is_empty(), "the search asked the index");
    assert!(
        asked.iter().all(|p| !p.fast),
        "a search by name is exact: {asked:?}"
    );
}
