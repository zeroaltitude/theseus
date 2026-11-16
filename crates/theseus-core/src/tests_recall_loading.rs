//! A recall after an idle stretch has the meaning search (issue 21 of 1.0.7).
//! The tender here is a stand-in socket with the real one's model: unloaded
//! until something asks, loading for `load`, then loaded. A query that names
//! a `wait_ms` waits for the load, one that does not is answered at once
//! without vectors, and `index.warm` starts the load and answers its state.
//! `Memory::warm_on_message` sends the warm at a person's message, and
//! recall's deadline extends to `LOAD_BOUND` only while the tender says it
//! is loading.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_protocol::index::{IndexQueryParams, IndexSourceRank};
use theseus_protocol::{Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::loading::{Warm, WarmFuture, LOAD_BOUND};
use crate::recall::{Ask, AskFuture, AskTwo, WORDS_ONLY};
use crate::tests_recall::{index_of, recalls, rig_with, session, turn, Rig};

const OTTER: &str = "Remember: the otters den under the alder roots at Fenmoor weir.";

#[derive(Clone, Copy)]
enum State {
    Unloaded,
    Loading(Instant),
    Loaded,
    Refused,
}

/// The stand-in's model, and what was asked of it.
struct Model {
    load: Duration,
    state: Mutex<State>,
    /// Whether an `index.warm` starts a load (the real one does; a test that
    /// loses the message's warm turns it off).
    warm_starts: bool,
    /// The vector search's embedding, after the model is loaded.
    embed: Duration,
    warms: AtomicUsize,
    loads: AtomicUsize,
}

impl Model {
    fn new(state: State, load: Duration) -> Arc<Self> {
        Self::with(state, load, true, Duration::ZERO)
    }

    fn with(state: State, load: Duration, warm_starts: bool, embed: Duration) -> Arc<Self> {
        Arc::new(Self {
            load,
            state: Mutex::new(state),
            warm_starts,
            embed,
            warms: AtomicUsize::new(0),
            loads: AtomicUsize::new(0),
        })
    }

    fn name(&self) -> &'static str {
        let mut s = self.state.lock().unwrap();
        if let State::Loading(until) = *s {
            if Instant::now() >= until {
                *s = State::Loaded;
            }
        }
        match *s {
            State::Unloaded => "unloaded",
            State::Loading(_) => "loading",
            State::Loaded => "loaded",
            State::Refused => "refused",
        }
    }

    fn start(&self) {
        let mut s = self.state.lock().unwrap();
        if matches!(*s, State::Unloaded) {
            *s = State::Loading(Instant::now() + self.load);
            self.loads.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// `index.warm`.
    fn warm(&self) -> &'static str {
        self.warms.fetch_add(1, Ordering::SeqCst);
        if self.warm_starts {
            self.start();
        }
        match self.name() {
            "unloaded" if self.warm_starts => "loading",
            n => n,
        }
    }

    /// A vector search's model, waiting up to `wait` for a load: whether it
    /// is loaded.
    async fn ready(&self, wait: Duration) -> bool {
        self.start();
        let until = Instant::now() + wait;
        loop {
            match self.name() {
                "loaded" => return true,
                "refused" => return false,
                _ if Instant::now() >= until => return false,
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    }
}

/// A stand-in tender on a socket in `dir`, each connection's requests
/// answered in order, from `index`, the vector side as `model` has it.
fn tender(
    dir: &std::path::Path,
    index: Ask,
    model: Arc<Model>,
    seen: Arc<Mutex<Vec<IndexQueryParams>>>,
) -> std::path::PathBuf {
    let path = dir.join("fake-index.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((conn, _)) = listener.accept().await else {
                return;
            };
            let (index, model, seen) = (index.clone(), model.clone(), seen.clone());
            tokio::spawn(async move {
                let (r, mut w) = conn.into_split();
                let mut r = BufReader::new(r);
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let req: Request = serde_json::from_str(&line).unwrap();
                    let p: IndexQueryParams = serde_json::from_value(req.params).unwrap();
                    seen.lock().unwrap().push(p.clone());
                    let is_vector = p.sources.iter().any(|s| s == "vector");
                    let wait = Duration::from_millis(p.wait_ms);
                    let mut answer = index(p).await.unwrap();
                    if is_vector {
                        if model.ready(wait).await {
                            tokio::time::sleep(model.embed).await;
                            for (i, h) in answer.hits.iter_mut().enumerate() {
                                let rank = IndexSourceRank {
                                    rank: i + 1,
                                    score: 0.9,
                                };
                                h.sources.insert("vector".into(), rank);
                            }
                        } else {
                            let why = if model.name() == "refused" {
                                "bm25_only: the weights would not load"
                            } else {
                                "the model is loading"
                            };
                            answer.skipped.insert("vector".into(), why.into());
                        }
                    }
                    let mut out = serde_json::to_vec(&Response::ok(req.id, answer)).unwrap();
                    out.push(b'\n');
                    if w.write_all(&out).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    path
}

fn two_on(path: std::path::PathBuf) -> AskTwo {
    Arc::new(
        move |a: IndexQueryParams, b: IndexQueryParams| -> (AskFuture, AskFuture) {
            let (x, y) = crate::tender::pair::call_two(&path, &a, &b, Duration::from_secs(30));
            (
                Box::pin(async move { x.await.map_err(|e| e.to_string()) }),
                Box::pin(async move { y.await.map_err(|e| e.to_string()) }),
            )
        },
    )
}

/// What `index.warm` says of `model`, or nothing when it is down.
fn warm_of(model: Option<Arc<Model>>) -> Warm {
    Arc::new(move || -> WarmFuture {
        let model = model.clone();
        Box::pin(async move { model.map(|m| m.warm().to_string()) })
    })
}

/// A core whose recall runs live on `baseline` over a past session that said
/// `OTTER`, asking the stand-in tender, with `deadline_ms`; and the session
/// for its turns. `warm` answers `index.warm`.
fn rig(
    model: &Arc<Model>,
    warm: Warm,
    deadline_ms: u64,
    mode: MemoryMode,
) -> (
    Rig,
    String,
    Arc<Mutex<Vec<IndexQueryParams>>>,
    tempfile::TempDir,
) {
    let r = rig_with(mode, |c| {
        c.memory.arm = MemoryArm::Baseline;
        c.memory.canary_fraction = 1.0;
        c.memory.recall_deadline_ms = deadline_ms;
    });
    let c = r.core.clone();
    let past = session(&c, None, &[OTTER]);
    let now = session(&c, None, &[]);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let dir = tempfile::tempdir().unwrap();
    let index = index_of(&c, vec![past]);
    let path = tender(dir.path(), index.clone(), model.clone(), seen.clone());
    c.runner.memory.set_ask(index);
    c.runner.memory.set_ask_two(two_on(path));
    c.runner.memory.set_warm(warm);
    (r, now, seen, dir)
}

/// The turn's only recall, and how long the turn took.
async fn recalled(r: &Rig, now: &str) -> (theseus_protocol::memory::RecallManifest, Duration) {
    let t0 = Instant::now();
    turn(&r.core, now, "Where do the otters den?").await;
    let took = t0.elapsed();
    let mut rows = recalls(&r.core, now);
    assert_eq!(rows.len(), 1);
    (rows.remove(0), took)
}

/// The owner's case: the model was unloaded, so a recall used to answer by
/// words alone. A person's message loads it at its arrival, and recall's
/// wait covers the load, so the recall has vectors, and the turn waited no
/// longer than the load and the bound.
#[tokio::test]
async fn a_recall_after_an_idle_stretch_has_the_meaning_search() {
    let model = Model::new(State::Unloaded, Duration::from_millis(700));
    let (r, now, seen, _dir) = rig(&model, warm_of(Some(model.clone())), 250, MemoryMode::Live);
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
    let (r, now, _seen, _dir) = rig(&model, warm_of(Some(model.clone())), 250, MemoryMode::Live);
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
        let warm = warm_of(warm_up.then(|| model.clone()));
        let (r, now, _seen, _dir) = rig(&model, warm, 250, MemoryMode::Live);
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
    let (r, now, _seen, _dir) = rig(&model, warm_of(Some(model.clone())), 250, MemoryMode::Live);
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
    let (r, now, _seen, _dir) = rig(&model, warm_of(Some(model.clone())), 250, MemoryMode::Live);
    let (m, took) = recalled(&r, &now).await;
    assert!(
        m.skipped["vector"].starts_with("bm25_only"),
        "{:?}",
        m.skipped
    );
    assert!(took < Duration::from_millis(1_800), "{took:?}");

    let model = Model::new(State::Unloaded, Duration::from_millis(700));
    let (r, now, _seen, _dir) = rig(&model, warm_of(None), 250, MemoryMode::Live);
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
    let (r, now, _seen, _dir) = rig(&model, warm_of(Some(model.clone())), 250, MemoryMode::Off);
    turn(&r.core, &now, "Where do the otters den?").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(model.warms.load(Ordering::SeqCst), 0);
    assert_eq!(model.loads.load(Ordering::SeqCst), 0);
}
