//! Recall answers in time (theseus-w9qv): a query that asks for vectors goes
//! out with its word sources alone beside it, so a vector search that misses
//! the deadline (its query's embedding behind a busy embedder) still leaves
//! the word hits, which rank and pack as usual under `outcome = words_only`;
//! a vector search in time gives the whole answer as before; and health's
//! memory block counts the last turns' recalls by outcome. The index is
//! `tests_recall`'s stand-in, its vector side made slow, failed, or stalled
//! on purpose.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use theseus_protocol::index::{IndexQueryParams, IndexSourceRank};

use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::{Ask, AskFuture, WORDS_ONLY};
use crate::tests_recall::{admitted, index_of, recalls, rig_with, session, turn};

const OTTER: &str = "Remember: the otters den under the alder roots at Fenmoor weir.";

/// How the stand-in's vector side behaves.
#[derive(Clone, Copy)]
enum Vectors {
    /// Answers at once, each hit ranked by the vector source too.
    Fast,
    /// Answers after this long: an embedding behind a busy embedder.
    Slow(Duration),
    /// Refuses the query.
    Failed,
    /// Never answers, and nor do the words.
    Stalled,
}

/// `tests_recall`'s stand-in index over `sessions`, with its vector side as
/// `v` says, keeping the sources of every query it is asked.
fn index(
    core: &Arc<crate::Core>,
    sessions: Vec<String>,
    v: Vectors,
    asked: Arc<Mutex<Vec<Vec<String>>>>,
) -> Ask {
    let inner = index_of(core, sessions);
    Arc::new(move |p: IndexQueryParams| -> AskFuture {
        asked.lock().unwrap().push(p.sources.clone());
        let vector = p.sources.iter().any(|s| s == "vector");
        let answer = inner(p);
        Box::pin(async move {
            if matches!(v, Vectors::Stalled) {
                std::future::pending::<()>().await;
            }
            if !vector {
                return answer.await;
            }
            match v {
                Vectors::Fast | Vectors::Stalled => {}
                Vectors::Slow(d) => tokio::time::sleep(d).await,
                Vectors::Failed => return Err("the query would not embed".into()),
            }
            let mut r = answer.await?;
            for (i, h) in r.hits.iter_mut().enumerate() {
                let rank = IndexSourceRank {
                    rank: i + 1,
                    score: 0.9,
                };
                h.sources.insert("vector".into(), rank);
            }
            Ok(r)
        })
    })
}

/// One turn under `mode` with the vector side as `v`: its recall row, the
/// sources of the queries asked, and how long the turn took.
async fn one_turn(
    mode: MemoryMode,
    v: Vectors,
) -> (
    theseus_protocol::memory::RecallManifest,
    Vec<Vec<String>>,
    Duration,
    Arc<crate::Core>,
    crate::tests_recall::Rig,
) {
    let r = rig_with(mode, |c| {
        c.memory.arm = MemoryArm::Baseline;
        c.memory.canary_fraction = 1.0;
    });
    let c = r.core.clone();
    let past = session(&c, None, &[OTTER]);
    let now = session(&c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    c.runner
        .memory
        .set_ask(index(&c, vec![past], v, asked.clone()));
    let t0 = Instant::now();
    turn(&c, &now, "Where do the otters den?").await;
    let took = t0.elapsed();
    let rows = recalls(&c, &now);
    assert_eq!(rows.len(), 1, "one recall on the first loop");
    let asked = asked.lock().unwrap().clone();
    (rows[0].clone(), asked, took, c, r)
}

/// The vector search behind a busy embedder: the word hits arrive in a few
/// ms, the deadline passes, and they rank and pack as usual, in shadow and in
/// front of the model, marked `words_only` with the vector source skipped
/// and why. The recall waits no longer than its deadline for them.
#[tokio::test]
async fn a_slow_vector_search_still_yields_the_word_hits_within_the_deadline() {
    for mode in [MemoryMode::Shadow, MemoryMode::Live] {
        let (m, asked, _, c, _r) = one_turn(mode, Vectors::Slow(Duration::from_secs(3))).await;
        assert_eq!(m.outcome, WORDS_ONLY, "{mode:?}");
        assert_eq!(
            asked,
            [vec!["bm25", "entity", "vector"], vec!["bm25", "entity"]]
        );
        assert_eq!(
            m.admitted.len(),
            1,
            "{mode:?}: the otters' note ranked by its words"
        );
        assert!(admitted(&m)[0].starts_with("ses_"));
        assert!(m.sources.contains_key("bm25") && !m.sources.contains_key("vector"));
        let why = "the vector search had not answered within 250 ms; the words alone ranked";
        assert_eq!(m.why.as_deref(), Some(why));
        assert_eq!(m.skipped.get("vector").map(String::as_str), Some(why));
        // The deadline's purpose kept: it waited the deadline, never the
        // vector's 3 s. (A few ms past it, unloaded: the measure below. On a
        // starved test runtime the recall's task runs late, so the bound here
        // is the vector's, not the few ms.)
        assert_eq!(m.timings.deadline_ms, 250);
        assert!(
            (250.0..3_000.0).contains(&m.timings.index_ms),
            "{mode:?}: the index's wait was {} ms",
            m.timings.index_ms
        );
        let h = c.memory_health().recalls.unwrap();
        assert_eq!(
            (h.turns, h.words_only, h.ok, h.last_full_ms),
            (1, 1, 0, None)
        );
    }
}

/// A vector search in time: the whole answer, fused as today, and the word
/// sources' answer beside it unused.
#[tokio::test]
async fn a_fast_vector_search_yields_the_hybrid_answer() {
    for mode in [MemoryMode::Shadow, MemoryMode::Live] {
        let (m, asked, took, c, _r) = one_turn(mode, Vectors::Fast).await;
        assert_eq!(asked.len(), 2);
        assert_eq!(m.outcome, "ran", "{mode:?}");
        assert_eq!(m.why, None);
        assert!(m.skipped.is_empty(), "{:?}", m.skipped);
        assert_eq!(m.sources.get("vector"), Some(&1), "{:?}", m.sources);
        assert_eq!(m.admitted[0].sources.len(), 2, "bm25 and vector ranks");
        assert!(took < Duration::from_secs(2), "{took:?}");
        let h = c.memory_health().recalls.unwrap();
        assert_eq!((h.turns, h.ok, h.words_only), (1, 1, 0));
        assert!(h.last_full_ms.is_some());
    }
}

/// A vector search that fails at once leaves the word hits too, without
/// waiting for the deadline; with nothing at all in time, `deadline`.
#[tokio::test]
async fn a_failed_vector_search_leaves_the_words_and_a_stalled_index_the_deadline() {
    let (m, _, _, _c, _r) = one_turn(MemoryMode::Live, Vectors::Failed).await;
    assert_eq!(m.outcome, WORDS_ONLY);
    assert_eq!(m.admitted.len(), 1);
    assert_eq!(
        m.why.as_deref(),
        Some("the vector search failed (the query would not embed); the words alone ranked")
    );

    let (m, _, took, c, _r) = one_turn(MemoryMode::Live, Vectors::Stalled).await;
    assert_eq!(m.outcome, "deadline");
    assert!(m.admitted.is_empty());
    assert!(took < Duration::from_secs(2), "{took:?}");
    let h = c.memory_health().recalls.unwrap();
    assert_eq!((h.turns, h.deadline), (1, 1));
}

/// Health's counts over several turns of one daemon: each recall by its
/// outcome, and the last whole answer's time.
#[tokio::test]
async fn health_counts_the_last_turns_recalls_by_outcome() {
    let r = rig_with(MemoryMode::Shadow, |_| {});
    let c = &r.core;
    assert_eq!(
        c.memory_health().recalls,
        None,
        "none before the first recall"
    );
    let past = session(c, None, &[OTTER]);
    let now = session(c, None, &[]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let set = |v| {
        c.runner
            .memory
            .set_ask(index(c, vec![past.clone()], v, asked.clone()));
    };
    set(Vectors::Fast);
    turn(c, &now, "Where do the otters den?").await;
    let full = c.memory_health().recalls.unwrap().last_full_ms.unwrap();
    set(Vectors::Slow(Duration::from_secs(3)));
    turn(c, &now, "And the otters' cubs?").await;
    turn(c, &now, "Which weir was it?").await;
    set(Vectors::Stalled);
    turn(c, &now, "Is the alder still there?").await;
    let h = c.memory_health().recalls.unwrap();
    assert_eq!(
        (h.window, h.turns, h.ok, h.words_only, h.deadline, h.error),
        (50, 4, 1, 2, 1, 0)
    );
    assert_eq!(h.last_full_ms, Some(full), "the last whole answer's time");
    let outcomes: Vec<String> = recalls(c, &now).into_iter().map(|m| m.outcome).collect();
    assert_eq!(outcomes, ["ran", WORDS_ONLY, WORDS_ONLY, "deadline"]);
}

/// A measure, not a check (`--ignored --nocapture`): the recall's time on a
/// live turn's path, and the turn's, with the vector search 3 s late (a
/// loaded embedder) and in time, over ten turns each.
#[tokio::test]
#[ignore = "a measure: run with --ignored --nocapture"]
async fn measure_the_recalls_time_on_the_turns_path() {
    for (label, v) in [
        ("vectors in time", Vectors::Fast),
        ("vectors 3 s late", Vectors::Slow(Duration::from_secs(3))),
    ] {
        let mut index_ms = Vec::new();
        let mut total_ms = Vec::new();
        let mut turn_ms = Vec::new();
        let mut outcomes = std::collections::BTreeMap::<String, u32>::new();
        for _ in 0..10 {
            let (m, _, took, _c, _r) = one_turn(MemoryMode::Live, v).await;
            index_ms.push(m.timings.index_ms);
            total_ms.push(m.timings.total_ms);
            turn_ms.push(took.as_secs_f64() * 1000.0);
            *outcomes.entry(m.outcome).or_default() += 1;
        }
        let p = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            format!("p50 {:.1} ms, max {:.1} ms", v[5], v[9])
        };
        println!(
            "{label}: outcomes {outcomes:?}; the index's wait {}; recall on the path {}; the turn {}",
            p(index_ms),
            p(total_ms),
            p(turn_ms)
        );
    }
}

/// The race on tokio's paused clock, where time moves only when every task
/// waits, so the bounds are exact even on a loaded machine: the words in
/// hand and the vector search late, the recall answers at its deadline and
/// not a tick past it; a vector answer in time wins before the deadline; a
/// failed vector search leaves the words as soon as they are in; failed
/// words and a late vector leave `deadline`; both failing, `unavailable`.
#[tokio::test(start_paused = true)]
async fn the_race_keeps_its_deadline_on_a_paused_clock() {
    use crate::recall::{Answer, Memory};
    use theseus_protocol::index::{IndexLag, IndexQueryResult, IndexTimings};
    type Side = Result<Duration, Duration>;
    // Each side answers after its wait: `Ok` with no hits, `Err` refused.
    let memory = |words: Side, vector: Side| {
        let m = Memory::new(crate::config::memory::MemoryConfig::default(), None);
        m.set_ask(Arc::new(move |p: IndexQueryParams| -> AskFuture {
            let side = if p.sources.iter().any(|s| s == "vector") {
                vector
            } else {
                words
            };
            Box::pin(async move {
                let (wait, ok) = match side {
                    Ok(d) => (d, true),
                    Err(d) => (d, false),
                };
                tokio::time::sleep(wait).await;
                if !ok {
                    return Err(format!("refused after {} ms", wait.as_millis()));
                }
                Ok(IndexQueryResult {
                    hits: Vec::new(),
                    indexed_through: 0,
                    lag: IndexLag::default(),
                    timings: IndexTimings::default(),
                    skipped: Default::default(),
                    weights: Default::default(),
                })
            })
        }));
        m
    };
    let ms = Duration::from_millis;
    let deadline = ms(250);
    let run = |words: Side, vector: Side| async move {
        let m = memory(words, vector);
        let t0 = tokio::time::Instant::now();
        let mut begun = m.begin("otters".to_string(), None, 5, MemoryArm::Baseline, deadline);
        let (answer, _) = begun.answer().await;
        let kind = match answer {
            Answer::Hits(r) => {
                format!("hits, vector skipped: {}", r.skipped.contains_key("vector"))
            }
            Answer::Deadline => "deadline".into(),
            Answer::Unavailable(why) => format!("unavailable: {why}"),
        };
        (kind, begun.words_only.is_some(), t0.elapsed())
    };
    assert_eq!(
        run(Ok(ms(3)), Ok(ms(3_000))).await,
        ("hits, vector skipped: true".into(), true, deadline),
        "the words at the deadline, never later"
    );
    assert_eq!(
        run(Ok(ms(3)), Ok(ms(120))).await,
        ("hits, vector skipped: false".into(), false, ms(120)),
        "the whole answer in time"
    );
    assert_eq!(
        run(Ok(ms(40)), Err(ms(10))).await,
        ("hits, vector skipped: true".into(), true, ms(40)),
        "a failed vector search waits for the words, not the deadline"
    );
    assert_eq!(
        run(Err(ms(3)), Ok(ms(3_000))).await,
        ("deadline".into(), false, deadline)
    );
    assert_eq!(
        run(Err(ms(3)), Err(ms(10))).await,
        ("unavailable: refused after 10 ms".into(), false, ms(10))
    );
}
