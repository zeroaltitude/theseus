//! The stand-in tender of the tests of recall's meeting with an unloaded
//! embedding model (issue 21 of 1.0.7), and of the measure of the share of
//! recalls that get vectors. It is a socket with the real tender's model:
//! unloaded until something asks, loading for `load`, then loaded. A query
//! that names a `wait_ms` waits for the load, one that does not is answered
//! at once without vectors, and `index.warm` starts the load and answers its
//! state. Lines marked `new:` use what issue 21 added: the measure's "before"
//! is this file and the measure at `main` before the issue, with them cut.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_protocol::index::{IndexQueryParams, IndexSourceRank};
use theseus_protocol::{Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::{Ask, AskFuture, AskTwo};
// new:begin
use crate::recall::loading::{Warm, WarmFuture};
// new:end
use crate::tests_recall::{index_of, recalls, rig_with, session, turn, Rig};

pub(crate) const OTTER: &str = "Remember: the otters den under the alder roots at Fenmoor weir.";

#[derive(Clone, Copy)]
pub(crate) enum State {
    Unloaded,
    Loading(Instant),
    Loaded,
    Refused,
}

/// The stand-in's model, and what was asked of it.
pub(crate) struct Model {
    pub(crate) load: Duration,
    pub(crate) state: Mutex<State>,
    /// Whether an `index.warm` starts a load (the real one does; a test that
    /// loses the message's warm turns it off).
    pub(crate) warm_starts: bool,
    /// The vector search's embedding, after the model is loaded.
    pub(crate) embed: Duration,
    pub(crate) warms: AtomicUsize,
    pub(crate) loads: AtomicUsize,
}

impl Model {
    pub(crate) fn new(state: State, load: Duration) -> Arc<Self> {
        Self::with(state, load, true, Duration::ZERO)
    }

    pub(crate) fn with(
        state: State,
        load: Duration,
        warm_starts: bool,
        embed: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            load,
            state: Mutex::new(state),
            warm_starts,
            embed,
            warms: AtomicUsize::new(0),
            loads: AtomicUsize::new(0),
        })
    }

    pub(crate) fn name(&self) -> &'static str {
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

    pub(crate) fn start(&self) {
        let mut s = self.state.lock().unwrap();
        if matches!(*s, State::Unloaded) {
            *s = State::Loading(Instant::now() + self.load);
            self.loads.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// `index.warm`.
    pub(crate) fn warm(&self) -> &'static str {
        self.warms.fetch_add(1, Ordering::SeqCst);
        if self.warm_starts {
            self.start();
        }
        match self.name() {
            "unloaded" if self.warm_starts => "loading",
            n => n,
        }
    }

    /// Ten idle minutes later: the model is unloaded.
    pub(crate) fn idle(&self) {
        *self.state.lock().unwrap() = State::Unloaded;
    }

    /// Until the model is loaded (a load some earlier query began).
    pub(crate) async fn settled(&self) {
        while self.name() != "loaded" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    /// A vector search's model, waiting up to `wait` for a load: whether it
    /// is loaded.
    pub(crate) async fn ready(&self, wait: Duration) -> bool {
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
pub(crate) fn tender(
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

pub(crate) fn two_on(path: std::path::PathBuf) -> AskTwo {
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

// new:begin
/// What `index.warm` says of `model`, or nothing when it is down.
pub(crate) fn warm_of(model: Option<Arc<Model>>) -> Warm {
    Arc::new(move || -> WarmFuture {
        let model = model.clone();
        Box::pin(async move { model.map(|m| m.warm().to_string()) })
    })
}
// new:end

/// A core whose recall runs live on `baseline` over a past session that said
/// `OTTER`, asking the stand-in tender, with `deadline_ms`; and the session
/// for its turns. `warm_up` says whether `index.warm` is answered (new: a tender that
/// is down answers nothing).
pub(crate) fn rig(
    model: &Arc<Model>,
    warm_up: bool,
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
    c.runner
        .memory
        .set_warm(warm_of(warm_up.then(|| model.clone()))); // new:
    (r, now, seen, dir)
}

/// The turn's only recall, and how long the turn took.
pub(crate) async fn recalled(
    r: &Rig,
    now: &str,
) -> (theseus_protocol::memory::RecallManifest, Duration) {
    let t0 = Instant::now();
    turn(&r.core, now, "Where do the otters den?").await;
    let took = t0.elapsed();
    let mut rows = recalls(&r.core, now);
    assert_eq!(rows.len(), 1);
    (rows.remove(0), took)
}
