//! Recall's vector query inside its deadline (theseus-zo1y). The vector
//! source embeds the turn's new text alone, cut by the tender at `[memory]
//! recall_vector_tokens` word pieces, while the word sources read the longer
//! query; a recall's two queries (its words' alone, then the whole) go on one
//! of the tender's connections; and once the recall stops waiting (its
//! deadline passed, or its turn ended) that connection closes, so the tender
//! stops the query's embedding and frees its slot. The tender here is a
//! stand-in socket that counts its connections, answers in order on each as
//! the real one does, and notes when each closes.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_protocol::index::{IndexQueryParams, IndexSourceRank};
use theseus_protocol::{Request, Response};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::{Ask, AskFuture, AskTwo, WORDS_ONLY};
use crate::tests_recall::{index_of, recalls, rig_with, session, turn, Rig};

const OTTER: &str = "Remember: the otters den under the alder roots at Fenmoor weir.";

/// What the stand-in tender saw.
#[derive(Default)]
struct Seen {
    /// Connections accepted.
    accepts: AtomicUsize,
    /// When each was accepted, by its number.
    accepted: Mutex<Vec<(usize, Instant)>>,
    /// Each request, by its connection's number.
    asked: Mutex<Vec<(usize, IndexQueryParams)>>,
    /// When each connection closed, by its number.
    closed: Mutex<Vec<(usize, Instant)>>,
}

/// A stand-in tender on a socket in `dir`: each connection's requests
/// answered in order, from `index` (`tests_recall`'s stand-in index), the
/// words' at once and a vector query's after `vector` (each of its hits
/// ranked by the vector source too). While it waits to answer a vector
/// query, a close of the connection is noted at once, as the real tender's
/// embedder notices one between its layers.
fn tender(
    dir: &std::path::Path,
    index: Ask,
    vector: Duration,
    seen: Arc<Seen>,
) -> std::path::PathBuf {
    let path = dir.join("fake-index.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((conn, _)) = listener.accept().await else {
                return;
            };
            let n = seen.accepts.fetch_add(1, Ordering::SeqCst);
            seen.accepted.lock().unwrap().push((n, Instant::now()));
            let (index, seen) = (index.clone(), seen.clone());
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
                    seen.asked.lock().unwrap().push((n, p.clone()));
                    let is_vector = p.sources.iter().any(|s| s == "vector");
                    let mut answer = index(p).await.unwrap();
                    if is_vector {
                        let mut next = String::new();
                        tokio::select! {
                            () = tokio::time::sleep(vector) => {}
                            // The caller has gone (EOF), or sent more.
                            read = r.read_line(&mut next) => {
                                assert_eq!(read.unwrap_or(0), 0, "a third request");
                                break;
                            }
                        }
                        for (i, h) in answer.hits.iter_mut().enumerate() {
                            let rank = IndexSourceRank {
                                rank: i + 1,
                                score: 0.9,
                            };
                            h.sources.insert("vector".into(), rank);
                        }
                    }
                    let mut out = serde_json::to_vec(&Response::ok(req.id, answer)).unwrap();
                    out.push(b'\n');
                    if w.write_all(&out).await.is_err() {
                        break;
                    }
                }
                seen.closed.lock().unwrap().push((n, Instant::now()));
            });
        }
    });
    path
}

/// The core's two queries on one connection, to the stand-in at `path`.
fn two_on(path: std::path::PathBuf) -> AskTwo {
    Arc::new(
        move |a: IndexQueryParams, b: IndexQueryParams| -> (AskFuture, AskFuture) {
            let (x, y) = crate::tender::pair::call_two(&path, &a, &b, Duration::from_secs(10));
            (
                Box::pin(async move { x.await.map_err(|e| e.to_string()) }),
                Box::pin(async move { y.await.map_err(|e| e.to_string()) }),
            )
        },
    )
}

/// A core whose recall runs live on `baseline`, over a past session that
/// said `OTTER`, asking the stand-in tender, whose vector side answers after
/// `vector`, under a deadline of `deadline_ms`; and its session for the
/// turns.
fn rig(vector: Duration, deadline_ms: u64) -> (Rig, String, Arc<Seen>, tempfile::TempDir) {
    let r = rig_with(MemoryMode::Live, |c| {
        c.memory.arm = MemoryArm::Baseline;
        c.memory.canary_fraction = 1.0;
        c.memory.recall_deadline_ms = deadline_ms;
    });
    let c = r.core.clone();
    let past = session(&c, None, &[OTTER]);
    let now = session(&c, None, &[]);
    let seen = Arc::new(Seen::default());
    let dir = tempfile::tempdir().unwrap();
    let index = index_of(&c, vec![past]);
    let path = tender(dir.path(), index.clone(), vector, seen.clone());
    c.runner.memory.set_ask(index);
    c.runner.memory.set_ask_two(two_on(path));
    (r, now, seen, dir)
}

/// A recall asks on one connection, its words' query first and then the
/// whole one, and the whole one's vector text is the turn's new text alone:
/// "yes" after a reply embeds "yes", cut at the config's 32 word pieces,
/// while the words' query carries the reply's start too. The whole answer,
/// in time, wins.
#[tokio::test]
async fn a_recall_asks_on_one_connection_and_embeds_the_new_text_alone() {
    // The deadline at its most, so a starved runtime still answers whole.
    let (r, now, seen, _dir) = rig(Duration::ZERO, 5_000);
    let c = &r.core;
    turn(c, &now, "Where do the otters den?").await;
    turn(c, &now, "yes").await;
    assert_eq!(
        seen.accepts.load(Ordering::SeqCst),
        2,
        "one connection a recall"
    );
    let asked = seen.asked.lock().unwrap().clone();
    let by_conn: Vec<(usize, Vec<String>, Option<String>)> = asked
        .iter()
        .map(|(n, p)| (*n, p.sources.clone(), p.vector_text.clone()))
        .collect();
    let words = || vec!["bm25".to_string(), "entity".to_string()];
    let whole = || {
        vec![
            "bm25".to_string(),
            "entity".to_string(),
            "vector".to_string(),
        ]
    };
    assert_eq!(
        by_conn,
        [
            (0, words(), None),
            (0, whole(), Some("Where do the otters den?".into())),
            (1, words(), None),
            (1, whole(), Some("yes".into())),
        ],
        "each recall: its words' query, then its whole one, on one connection"
    );
    let second = &asked[3].1;
    assert_eq!(second.vector_tokens, 32);
    assert!(
        second.text.starts_with("yes\n") && second.text.len() > "yes\n".len(),
        "the words read the reply's start too: {:?}",
        second.text
    );
    for m in recalls(c, &now) {
        assert_eq!(m.outcome, "ran", "the whole answer in time wins");
        assert!(m.sources.contains_key("vector"), "{:?}", m.sources);
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
    }
}

/// The vector side late: the words' hits rank alone (`words_only`, the
/// vector named in `skipped` and why), and the recall's connection closes as
/// its deadline passes, never held for the vector's 5 s: the tender stops
/// its embedding and frees the slot.
#[tokio::test]
async fn a_late_vector_side_leaves_the_words_and_closes_its_connection() {
    let (r, now, seen, _dir) = rig(Duration::from_secs(5), 250);
    let c = &r.core;
    turn(c, &now, "Where do the otters den?").await;
    let rows = recalls(c, &now);
    assert_eq!(rows.len(), 1);
    let m = &rows[0];
    assert_eq!(m.outcome, WORDS_ONLY);
    let why = "the vector search had not answered within 250 ms; the words alone ranked";
    assert_eq!(m.skipped.get("vector").map(String::as_str), Some(why));
    assert_eq!(m.admitted.len(), 1, "the otters' note ranked by its words");
    let deadline = Instant::now() + Duration::from_secs(5);
    let closed = loop {
        if let Some(&(_, at)) = seen.closed.lock().unwrap().first() {
            break at;
        }
        assert!(
            Instant::now() < deadline,
            "the recall's connection never closed"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    let accepted = seen.accepted.lock().unwrap()[0].1;
    let held = closed.duration_since(accepted);
    // The deadline's 250 ms, and room for a starved test runtime.
    assert!(
        held < Duration::from_millis(2_500),
        "the connection was held {held:?}, past the deadline's 250 ms and toward the vector's 5 s"
    );
    assert_eq!(seen.accepts.load(Ordering::SeqCst), 1);
}

/// A turn's recall cuts its vector text at `[memory] recall_vector_tokens`
/// (under its 250 ms deadline); a search's own words, under its 2 s one, go
/// uncut (review of theseus-zo1y: a long operator search kept its tail on
/// the vector side, as before the cut).
#[tokio::test]
async fn a_search_embeds_its_whole_query_and_a_turn_its_cut() {
    let (r, now, seen, _dir) = rig(Duration::ZERO, 5_000);
    let c = &r.core;
    turn(c, &now, "Where do the otters den?").await;
    let search = theseus_protocol::memory::MemorySearchParams {
        query: "Where do the otters den under the alder roots at Fenmoor weir?".into(),
        ..Default::default()
    };
    c.memory_search(search).await.unwrap();
    let asked = seen.asked.lock().unwrap().clone();
    let whole: Vec<(Option<String>, u64)> = asked
        .iter()
        .filter(|(_, p)| p.sources.iter().any(|s| s == "vector"))
        .map(|(_, p)| (p.vector_text.clone(), p.vector_tokens))
        .collect();
    assert_eq!(
        whole,
        [(Some("Where do the otters den?".into()), 32), (None, 0)],
        "the turn's vector text cut at 32; the search's query whole"
    );
}

/// A recall dropped unread (its turn ended first) closes its connection at
/// once, not at its deadline: `Begun`'s drop aborts its task (review of
/// theseus-zo1y).
#[tokio::test]
async fn a_recall_dropped_unread_closes_its_connection_at_once() {
    let (r, _now, seen, _dir) = rig(Duration::from_secs(30), 5_000);
    let begun = r.core.runner.memory.begin(
        "otters".to_string(),
        None,
        5,
        MemoryArm::Baseline,
        Duration::from_secs(20),
    );
    let t0 = Instant::now();
    while seen.asked.lock().unwrap().len() < 2 {
        assert!(t0.elapsed() < Duration::from_secs(10), "never asked");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    drop(begun);
    let dropped = Instant::now();
    while seen.closed.lock().unwrap().is_empty() {
        assert!(
            dropped.elapsed() < Duration::from_secs(5),
            "the connection outlived its dropped recall"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
