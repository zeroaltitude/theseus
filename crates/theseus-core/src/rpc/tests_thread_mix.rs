//! Streams that do not mix (theseus-wncdv), through the whole core. The
//! threadbench's threads arms answered about another stream's facts on 4 to 7
//! of 12 cross-stream questions (one session: 0): every stream has a port, a
//! ticket, a branch, so a question about this thread's port scores another
//! stream's nearly as high, and the notes of the place's own conversation,
//! where each stream's first messages ran before its thread existed, passed
//! undistanced and unmarked. Each mechanism has a test here: recall's
//! provenance (who owns a note, and what another stream's must beat), when
//! another thread's notes are pulled (a shared entity, not a question's
//! shape), and what a prefix carries into a thread's request
//! (`prefix::tests`).

use std::sync::Arc;

use theseus_protocol::memory::RecallManifest;
use theseus_protocol::threads::ThreadSealParams;
use theseus_store::{kinds, NewRecord};

use super::tests_thread_recall::{
    ambient, cli, new_thread, nodes_of, say, scored, scored_with, thread_session,
};
use super::Core;
use crate::config::MemoryMode;
use crate::moved::{moved_of_key, MovedTo};
use crate::tests_recall::{rig_with, session, Rig};

/// A canary rig, with the three keys at `on` (their default is true).
fn rig(on: bool) -> Rig {
    rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.threads.cross_recall_beats_own = on;
        c.threads.cross_recall_needs_entity = on;
        c.threads.base_distance = on;
    })
}

/// Two streams with a colliding fact each, in their threads, and the place's
/// own conversation holding a third stream's first message.
struct World {
    homesvc: String,
    datatidy: String,
    base: String,
}

fn world(c: &Core) -> World {
    new_thread(c, "homesvc", "homesvc maintenance");
    new_thread(c, "datatidy", "datatidy cleanup");
    let homesvc = thread_session(c, "homesvc");
    say(
        c,
        &homesvc,
        "the staging copy of homesvc listens on port 8453",
    );
    let datatidy = thread_session(c, "datatidy");
    say(c, &datatidy, "the datatidy preview answers on port 8444");
    let base = session(c, None, &["the ledger fix SEC-4412 staging port is 8443"]);
    World {
        homesvc,
        datatidy,
        base,
    }
}

async fn ask_port(c: &Arc<Core>, w: &World) -> RecallManifest {
    let picks = vec![
        (w.homesvc.clone(), 0, 0.040),
        (w.datatidy.clone(), 0, 0.039),
        (w.base.clone(), 0, 0.038),
    ];
    ambient(
        c,
        &w.homesvc,
        "which port is the staging copy for the homesvc work on?",
        scored(c, picks),
    )
    .await
}

/// Mechanism 1 (recall's provenance): a cross-stream question asked in thread A
/// does not get thread B's fact, nor the place's own conversation's. Their hits
/// score within a hair of A's own, so the bar alone admits them; they must
/// outrank what A holds once they have paid. With the keys off, as before, both
/// come in.
#[tokio::test]
async fn a_cross_stream_question_in_a_thread_does_not_get_another_streams_fact() {
    let on = rig(true);
    let w = world(&on.core);
    let m = ask_port(&on.core, &w).await;
    assert_eq!(nodes_of(&m), [w.homesvc.as_str()], "{m:?}");
    assert_eq!(m.drops.get("thread"), Some(&2), "{:?}", m.drops);

    let off = rig(false);
    let w = world(&off.core);
    let m = ask_port(&off.core, &w).await;
    let from = nodes_of(&m);
    assert!(
        from.contains(&w.datatidy.as_str()) && from.contains(&w.base.as_str()),
        "the old behaviour admits both: {m:?}"
    );
}

/// Naming the other stream is the asked way: its fact comes in, the place's own
/// conversation's still does not (a name is no claim on the conversation).
#[tokio::test]
async fn naming_a_stream_brings_its_fact_in_but_not_the_places_conversation() {
    let r = rig(true);
    let c = &r.core;
    let w = world(c);
    let picks = vec![
        (w.homesvc.clone(), 0, 0.030),
        (w.datatidy.clone(), 0, 0.040),
        (w.base.clone(), 0, 0.039),
    ];
    let m = ambient(
        c,
        &w.homesvc,
        "and which port does the datatidy preview use?",
        scored(c, picks),
    )
    .await;
    let from = nodes_of(&m);
    assert!(from.contains(&w.datatidy.as_str()), "{m:?}");
    assert!(!from.contains(&w.base.as_str()), "{m:?}");
    assert_eq!(m.drops.get("thread"), Some(&1), "{:?}", m.drops);
}

/// Mechanism 3 (when another thread's notes are pulled): a thread with nothing
/// of its own to compare is still not given a note whose chunk holds entities the
/// message names none of; one that shares an entity comes in. Off, the bar decides.
#[tokio::test]
async fn another_threads_note_is_pulled_on_a_shared_entity_not_a_shape() {
    let r = rig(true);
    let c = &r.core;
    let w = world(c);
    let held = vec!["beads:SEC-4412".to_string(), "file:app.py".to_string()];
    let ask = |shared: Vec<String>| {
        scored_with(
            c,
            vec![(w.datatidy.clone(), 0, 0.040)],
            vec![(held.clone(), shared)],
        )
    };
    let message = "which port should the new replica use?";
    let m = ambient(c, &w.homesvc, message, ask(vec![])).await;
    assert!(m.admitted.is_empty(), "{m:?}");
    assert_eq!(m.drops.get("thread"), Some(&1), "{:?}", m.drops);
    let m = ambient(c, &w.homesvc, message, ask(vec!["file:app.py".into()])).await;
    assert_eq!(nodes_of(&m), [w.datatidy.as_str()], "{m:?}");

    let off = rig(false);
    let w = world(&off.core);
    let m = ambient(
        &off.core,
        &w.homesvc,
        message,
        scored_with(
            &off.core,
            vec![(w.datatidy.clone(), 0, 0.040)],
            vec![(held.clone(), vec![])],
        ),
    )
    .await;
    assert_eq!(nodes_of(&m), [w.datatidy.as_str()], "off, as before: {m:?}");
}

/// A note of the place's own conversation that a split moved to thread B is B's,
/// not the conversation's: another stream's to A (here with the base rule off, so
/// only its owner decides), A's own to B, and a sealed B's never ambient.
#[tokio::test]
async fn a_node_a_split_moved_is_its_threads_in_recall() {
    let r = rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.threads.base_distance = false;
    });
    let c = &r.core;
    let w = world(c);
    let (_, note) = c.store.session_nodes(&w.base).unwrap().remove(0);
    let picks = || {
        vec![
            (w.homesvc.clone(), 0, 0.040),
            (w.datatidy.clone(), 0, 0.040),
            (w.base.clone(), 0, 0.030),
        ]
    };
    let words = "tell me about the staging port";
    let m = ambient(c, &w.homesvc, words, scored(c, picks())).await;
    assert!(
        nodes_of(&m).contains(&w.base.as_str()),
        "unmoved, a conversation note passes with the base rule off: {m:?}"
    );

    let moved = MovedTo {
        thread: "datatidy".into(),
        split: "spl_test".into(),
    };
    c.store
        .append(&[
            NewRecord::json(kinds::META, Some(&moved_of_key(&note.id)), &Some(&moved)).unwrap(),
        ])
        .unwrap();
    let m = ambient(c, &w.homesvc, words, scored(c, picks())).await;
    assert!(
        !nodes_of(&m).contains(&w.base.as_str()),
        "moved to datatidy, it is another stream's to homesvc: {m:?}"
    );
    let m = ambient(c, &w.datatidy, words, scored(c, picks())).await;
    assert!(
        nodes_of(&m).contains(&w.base.as_str()),
        "datatidy's own: {m:?}"
    );

    c.thread_seal(
        &ThreadSealParams {
            slug: "datatidy".into(),
            reason: Some("embargoed".into()),
            ..Default::default()
        },
        cli(),
    )
    .unwrap();
    let m = ambient(c, &w.homesvc, words, scored(c, picks())).await;
    assert!(!nodes_of(&m).contains(&w.base.as_str()), "{m:?}");
    assert!(m.drops.get("sealed") >= Some(&1), "{:?}", m.drops);
}
