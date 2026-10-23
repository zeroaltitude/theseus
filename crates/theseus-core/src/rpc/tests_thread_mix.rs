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

use super::tests_own_top::recall_in;
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

/// The five live threads of the threadbench (theseus-stwkb), each with a
/// working set of relative roots as 98bz.5 grows them (homesvc's holding the
/// generic `notes`), each thread's own same-shape note in its session, and
/// the place's own conversation holding msg 113's new cutoff.
const STREAMS: [(&str, &str, &[&str], &str); 5] = [
    (
        "homesvc",
        "homesvc maintenance",
        &["homesvc", "notes"],
        "the homesvc staging copy listens on port 8453",
    ),
    (
        "tarnlib",
        "tarnlib release",
        &["tarnlib"],
        "the tarnlib docs server listens on port 9443",
    ),
    (
        "datatidy",
        "datatidy pipeline",
        &["datatidy"],
        "the datatidy report flags rows above the 0.45 cutoff",
    ),
    (
        "ledgerweb",
        "ledgerweb checkout",
        &["ledgerweb"],
        "the ledgerweb report flags rows above the 0.40 cutoff on port 8443",
    ),
    (
        "quillhn",
        "quillhn reader",
        &["quillhn"],
        "the quillhn reader listens on port 8080",
    ),
];

/// A candidate's owner in the fixture: a thread, or the place's own conversation.
#[derive(Clone, Copy)]
enum Who {
    Thread(&'static str),
    Home,
}

/// What a candidate's chunk shares with the message by entity.
#[derive(Clone, Copy)]
enum Ties {
    None,
    Shared,
    Apart,
}

/// One message: the asking thread, its words, and the index's answer.
struct Message {
    asker: &'static str,
    words: &'static str,
    hits: Vec<(Who, f64, Ties)>,
}

/// A handful of the bench's messages: one with no name, one that says "notes",
/// msg 113's shape (the home note holds the answer), and one naming its own
/// project. The asking thread's own note is the best hit (0.104), in context.
fn messages() -> Vec<Message> {
    use {Ties::*, Who::*};
    vec![
        Message {
            asker: "tarnlib",
            words: "which port does the staging copy listen on?",
            hits: vec![
                (Thread("tarnlib"), 0.104, None),
                (Thread("homesvc"), 0.100, None),
                (Thread("quillhn"), 0.099, None),
                (Thread("ledgerweb"), 0.098, None),
                (Home, 0.060, None),
            ],
        },
        Message {
            asker: "ledgerweb",
            words: "which port did we put in the notes for staging?",
            hits: vec![
                (Thread("ledgerweb"), 0.104, None),
                (Thread("homesvc"), 0.101, None),
                (Thread("tarnlib"), 0.099, None),
                (Thread("quillhn"), 0.096, None),
            ],
        },
        Message {
            asker: "datatidy",
            words: "Regenerate the report with the new cutoff.",
            hits: vec![
                (Thread("datatidy"), 0.104, Shared),
                (Thread("ledgerweb"), 0.100, Shared),
                (Home, 0.097, None),
                (Thread("quillhn"), 0.099, Apart),
                (Thread("tarnlib"), 0.050, None),
            ],
        },
        Message {
            asker: "quillhn",
            words: "what port does the quillhn reader use?",
            hits: vec![
                (Thread("quillhn"), 0.104, None),
                (Thread("homesvc"), 0.099, None),
                (Thread("tarnlib"), 0.098, None),
                (Thread("datatidy"), 0.090, Apart),
            ],
        },
    ]
}

/// The fixture's admissions over every message: from another thread, from
/// the place's own conversation, and the distance's drops by rule.
#[derive(Debug, Default, PartialEq)]
struct Tally {
    other: usize,
    home: usize,
    drops: std::collections::BTreeMap<String, usize>,
}

/// What a variant of the fixture changes: the config, the working sets'
/// roots, a link, and the notes Jev said yes to (by stream, in message 3).
#[derive(Default)]
struct Variant {
    old_own_top: bool,
    no_roots: bool,
    link: Option<(&'static str, &'static str)>,
    jev: &'static [&'static str],
}

async fn tally(v: Variant) -> Tally {
    use theseus_protocol::threads::{ThreadEditParams, ThreadLinkParams, WorkingSet};
    let r = rig_with(MemoryMode::Canary, |c| {
        c.memory.canary_fraction = 1.0;
        c.threads.own_top_admissible = !v.old_own_top;
    });
    let c = &r.core;
    let mut of = std::collections::BTreeMap::new();
    for (slug, title, roots, note) in STREAMS {
        new_thread(c, slug, title);
        let roots = if v.no_roots {
            vec![]
        } else {
            roots.iter().map(|r| r.to_string()).collect()
        };
        let working_set = Some(WorkingSet {
            roots,
            ..Default::default()
        });
        let edit = ThreadEditParams {
            slug: slug.into(),
            working_set,
            ..Default::default()
        };
        c.thread_edit(&edit, cli()).unwrap();
        let sid = thread_session(c, slug);
        let node = say(c, &sid, note);
        of.insert(slug, (sid, node));
    }
    if let Some((a, b)) = v.link {
        let p = ThreadLinkParams {
            slug: a.into(),
            other: b.into(),
            ..Default::default()
        };
        c.thread_link(&p, cli()).unwrap();
    }
    let home = session(
        c,
        None,
        &["The cutoff for flagging changed: use 0.55 from now on."],
    );
    let mut t = Tally::default();
    for (i, m) in messages().into_iter().enumerate() {
        let sid = |w: Who| match w {
            Who::Thread(s) => of[s].0.clone(),
            Who::Home => home.clone(),
        };
        let picks = m.hits.iter().map(|(w, f, _)| (sid(*w), 0, *f)).collect();
        let ties = m
            .hits
            .iter()
            .map(|(_, _, tie)| match tie {
                Ties::None => (vec![], vec![]),
                Ties::Shared => (vec!["file:report.md".into()], vec!["file:report.md".into()]),
                Ties::Apart => (vec!["file:viewer.py".into()], vec![]),
            })
            .collect();
        let jev: Vec<String> = if i == 2 {
            v.jev.iter().map(|s| of[s].1.clone()).collect()
        } else {
            vec![]
        };
        let held = std::collections::BTreeSet::from([of[m.asker].1.clone()]);
        let ask = scored_with(c, picks, ties);
        let got = recall_in(c, &of[m.asker].0, m.words, held, ask, &jev).await;
        for a in &got.admitted {
            if a.session_id == home {
                t.home += 1;
            } else if a.session_id != of[m.asker].0 {
                t.other += 1;
            }
        }
        for d in got
            .dropped
            .iter()
            .filter(|d| d.reason.starts_with("thread"))
        {
            *t.drops.entry(d.reason.clone()).or_default() += 1;
        }
    }
    eprintln!("tally: {t:?}");
    t
}

fn tallied(other: usize, home: usize, drops: &[(&str, usize)]) -> Tally {
    Tally {
        other,
        home,
        drops: drops.iter().map(|(k, n)| (k.to_string(), *n)).collect(),
    }
}

/// The fixture admits near T12 (theseus-stwkb). T12-like here: what the lens
/// admitted before 98bz.8 (`the_fixture_with_the_key_off_is_the_old_lens`),
/// with msg 113's home note in. On the base (98bz.8's `own_top` for every
/// provenance) the asking thread's own notes are all in context, so nothing
/// held another stream's note back: 9 came in, and no drop said `thread_own`,
/// as on the threadbench's install #74. Now only the one the message waives by
/// naming homesvc's `notes` root comes in, beside the home note with the new
/// cutoff. A change that lets other streams back in turns this red.
#[tokio::test]
async fn the_fixture_admits_near_t12() {
    let t = tally(Variant::default()).await;
    let drops = [
        ("thread_bar", 1),
        ("thread_base", 1),
        ("thread_entity", 2),
        ("thread_own", 8),
    ];
    assert_eq!(t, tallied(1, 1, &drops));
}

/// `[threads] own_top_admissible = false`: the verdicts before 98bz.8, the
/// home note included (it drops `thread_base`).
#[tokio::test]
async fn the_fixture_with_the_key_off_is_the_old_lens() {
    let t = tally(Variant {
        old_own_top: true,
        ..Default::default()
    })
    .await;
    let drops = [
        ("thread_bar", 1),
        ("thread_base", 2),
        ("thread_entity", 2),
        ("thread_own", 8),
    ];
    assert_eq!(t, tallied(1, 0, &drops));
}

/// The bisect's other switches, each alone (theseus-stwkb): without the
/// working sets' roots the `notes` waiver is gone; a link waives its other
/// end; Jev's yes admits a note `own_top` held back. On the base each left
/// 9 in, since nothing was held back to move.
#[tokio::test]
async fn the_fixtures_other_switches_each_move_one_note() {
    let roots = tally(Variant {
        no_roots: true,
        ..Default::default()
    })
    .await;
    let linked = tally(Variant {
        link: Some(("tarnlib", "homesvc")),
        ..Default::default()
    })
    .await;
    let jev = tally(Variant {
        jev: &["ledgerweb"],
        ..Default::default()
    })
    .await;
    let drops = |own| {
        [
            ("thread_bar", 1),
            ("thread_base", 1),
            ("thread_entity", 2),
            ("thread_own", own),
        ]
    };
    assert_eq!(
        (roots, linked, jev),
        (
            tallied(0, 1, &drops(9)),
            tallied(2, 1, &drops(7)),
            tallied(2, 1, &drops(7)),
        )
    );
}
