//! The share of recalls that get vectors, and a turn's time to first word,
//! after idle stretches and in a warm session (issue 21 of 1.0.7), against
//! `tests_recall_standin`'s tender with a model that loads in 800 ms. Turns
//! run as the owner's did: after ten idle minutes (the model unloaded, no
//! keystroke before the message), after a first keystroke that warmed it a
//! typing's time earlier, and in a warm session. The stand-in's provider
//! answers at once, so a turn's duration is the time before its first word.
//! The table prints with `--no-capture`; its "before" is this file and the
//! stand-in at `main` before the issue, the lines marked `new:` cut.

use std::time::{Duration, Instant};

use crate::config::MemoryMode;
use crate::tests_recall::{recalls, turn};
use crate::tests_recall_standin::{rig, Model, State};

const LOAD: Duration = Duration::from_millis(800);
/// A first keystroke to its message: a person typing a line.
const TYPING: Duration = Duration::from_millis(1_200);

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Cold,
    Typed,
    Warm,
}

/// The middle of `ms`, and its greatest.
fn median_max(ms: &[u64]) -> (u64, u64) {
    let mut v = ms.to_vec();
    v.sort_unstable();
    (v[v.len() / 2], v[v.len() - 1])
}

#[tokio::test]
async fn the_share_of_recalls_with_vectors_after_idle_stretches() {
    let model = Model::new(State::Unloaded, LOAD);
    let (r, now, _seen, _dir) = rig(&model, true, 250, MemoryMode::Live);
    // The owner's month: 56% of recalls met an unloaded model.
    use Kind::{Cold, Typed, Warm};
    let plan = [
        Cold, Warm, Cold, Warm, Typed, Warm, Cold, Warm, Typed, Cold, Warm, Typed, Cold, Cold,
    ];
    let mut took: Vec<(Kind, u64)> = Vec::new();
    for kind in plan {
        match kind {
            Cold => model.idle(),
            Typed => {
                model.idle();
                model.warm();
                tokio::time::sleep(TYPING).await;
            }
            Warm => model.settled().await,
        }
        let t0 = Instant::now();
        turn(&r.core, &now, "Where do the otters den?").await;
        took.push((kind, t0.elapsed().as_millis() as u64));
    }
    let rows = recalls(&r.core, &now);
    assert_eq!(rows.len(), plan.len());
    let with_vectors: Vec<bool> = rows
        .iter()
        .map(|m| m.sources.contains_key("vector") && m.skipped.is_empty())
        .collect();
    eprintln!("recall measure: model load {LOAD:?}, recall deadline 250 ms");
    for (name, k) in [
        ("after ten idle minutes", Cold),
        ("after a first keystroke", Typed),
        ("in a warm session", Warm),
    ] {
        let of: Vec<usize> = (0..plan.len()).filter(|&i| plan[i] == k).collect();
        let ms: Vec<u64> = of.iter().map(|&i| took[i].1).collect();
        let (mid, max) = median_max(&ms);
        let got = of.iter().filter(|&&i| with_vectors[i]).count();
        eprintln!(
            "  {name:<26} {got}/{} with vectors; a turn's time to first word: median {mid} ms, greatest {max} ms",
            of.len()
        );
    }
    let got = with_vectors.iter().filter(|v| **v).count();
    eprintln!(
        "  all: {got}/{} recalls with vectors ({}%)",
        plan.len(),
        100 * got / plan.len()
    );
    assert_eq!(got, plan.len(), "every recall had the meaning search"); // new:
}
