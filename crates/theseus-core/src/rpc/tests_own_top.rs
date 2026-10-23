//! `own_top` counts only what can still be admitted (theseus-98bz.8), through
//! the whole core. The threadbench's msg 113: "The cutoff for flagging
//! changed: use 0.55 from now on." ran at home; the next message, in the
//! pipeline thread, asked for the report with the new cutoff, and the home
//! note holding "0.55" (fused 0.097, its share 0.93 of the best) was dropped
//! as another stream's because, once it had paid, it did not beat the
//! thread's own best hit, 0.104: a note already in the turn's context.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::memory::RecallManifest;

use super::tests_thread_recall::{new_thread, say, scored, thread_session};
use super::Core;
use crate::config::memory::MemoryArm;
use crate::config::MemoryMode;
use crate::recall::Scene;
use crate::tests_recall::{rig_with, session, Rig};

const ASK: &str = "Regenerate the report with the new cutoff.";

fn rig(admissible: bool) -> Rig {
    rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.threads.own_top_admissible = admissible;
    })
}

/// The pipeline thread's own note (in the turn's context), a second own
/// note, and the home note that holds the new cutoff.
struct World {
    pipe: String,
    in_context: String,
    home: String,
    cutoff: String,
}

fn world(c: &Core) -> World {
    new_thread(c, "pipeline", "datatidy pipeline");
    let pipe = thread_session(c, "pipeline");
    let in_context = say(
        c,
        &pipe,
        "the report groups rows by region and flags outliers",
    );
    say(
        c,
        &pipe,
        "the report is written to report.md under datatidy",
    );
    let home = session(c, None, &[]);
    let cutoff = say(
        c,
        &home,
        "The cutoff for flagging changed: use 0.55 from now on.",
    );
    World {
        pipe,
        in_context,
        home,
        cutoff,
    }
}

/// The turn's recall for `asker`, with `in_context` already in its request.
async fn recall(
    c: &Arc<Core>,
    asker: &str,
    in_context: &str,
    picks: Vec<(String, usize, f64)>,
) -> RecallManifest {
    let memory = &c.runner.memory;
    memory.set_ask(scored(c, picks));
    let mut begun = memory.begin(
        ASK.to_string(),
        None,
        40,
        MemoryArm::Baseline,
        Duration::from_millis(250),
    );
    let answer = begun.answer().await;
    let scene = Scene {
        mode: "live",
        session_id: Some(asker),
        turn_id: None,
        place: c.runner.place_of(asker),
        in_context: BTreeSet::from([in_context.to_string()]),
        labeled: Default::default(),
        budget_tokens: None,
        max_items: None,
        science: memory.science_for(MemoryArm::Baseline),
        activation: None,
        threads: c.runner.thread_scene(Some(asker), ASK, true),
        persona: None,
        category: None,
    };
    memory.manifest(
        &scene,
        &begun,
        answer,
        |s| c.runner.place_of(s),
        |_| Vec::new(),
        true,
    )
}

fn reason_of<'a>(m: &'a RecallManifest, node: &str) -> Option<&'a str> {
    m.dropped
        .iter()
        .find(|d| d.node_id == node)
        .map(|d| d.reason.as_str())
}

fn admitted(m: &RecallManifest, node: &str) -> bool {
    m.admitted.iter().any(|a| a.node_id == node)
}

/// msg 113's numbers: the cross candidate at 0.097 against the thread's own
/// note at 0.104, in context, the best of the query, and no other own
/// candidate: it pays and is admitted; the own note drops `in_context`.
#[tokio::test]
async fn a_note_in_context_holds_no_cross_note_back() {
    let r = rig(true);
    let c = &r.core;
    let w = world(c);
    let m = recall(
        c,
        &w.pipe,
        &w.in_context,
        vec![(w.pipe.clone(), 0, 0.104), (w.home.clone(), 0, 0.097)],
    )
    .await;
    assert!(admitted(&m, &w.cutoff), "{:?}", m.dropped);
    assert_eq!(reason_of(&m, &w.in_context), Some("in_context"));
}

/// An own candidate not in context above the cross note's paid score
/// (0.097 × 0.65 = 0.063) still keeps it out.
#[tokio::test]
async fn an_own_note_that_can_be_admitted_still_outranks_it() {
    let r = rig(true);
    let c = &r.core;
    let w = world(c);
    let picks = vec![
        (w.pipe.clone(), 0, 0.104),
        (w.home.clone(), 0, 0.097),
        (w.pipe.clone(), 1, 0.08),
    ];
    let m = recall(c, &w.pipe, &w.in_context, picks).await;
    assert_eq!(
        reason_of(&m, &w.cutoff),
        Some("thread_base"),
        "{:?}",
        m.admitted
    );
}

/// `own_top_admissible = false`: the note in context counts again, and the
/// cross note drops as it did.
#[tokio::test]
async fn with_the_key_off_the_old_verdict_is_back() {
    let r = rig(false);
    let c = &r.core;
    let w = world(c);
    let m = recall(
        c,
        &w.pipe,
        &w.in_context,
        vec![(w.pipe.clone(), 0, 0.104), (w.home.clone(), 0, 0.097)],
    )
    .await;
    assert_eq!(reason_of(&m, &w.cutoff), Some("thread_base"));
    assert!(!admitted(&m, &w.cutoff));
}
