//! The thread distance and the seal through recall's pipeline
//! (theseus-f2g0): a cross-thread hit pays, a hit under the bar is withheld,
//! naming the thread waives both, and a sealed thread's note never comes in,
//! whoever asks, over generated recalls.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use crate::recall::{recall, Asker, Candidate, Pack, Params, Place, Reason};
use crate::science::Baseline;
use crate::threads::{Cause, SessionThread, ThreadLens};

fn cand(node: &str, session: &str, fused: f64) -> Candidate {
    Candidate {
        node_id: node.into(),
        chunk: 0,
        session_id: session.into(),
        position: 1,
        kind: "user_message".into(),
        origin: "operator".into(),
        external: false,
        text: format!("the text of {node}"),
        fused,
        index_rank: 1,
        place: Place::Private,
        time_ms: 0,
        entities: Vec::new(),
        matched: Vec::new(),
    }
}

fn lens(here: Option<&str>, waived: &[&str], held: &[&str]) -> ThreadLens {
    let mut of_session = BTreeMap::new();
    for (s, slug) in [("ses_pr", "pr-round"), ("ses_fix", "security-fix")] {
        of_session.insert(
            s.to_string(),
            SessionThread {
                slug: slug.into(),
                held: held.contains(&slug),
            },
        );
    }
    ThreadLens {
        here: here.map(str::to_string),
        distance: here.is_some(),
        of_session,
        waived: waived.iter().map(|s| s.to_string()).collect(),
        top_fused: 0.04,
        penalty: 0.35,
        bar: 0.75,
        ..ThreadLens::default()
    }
}

fn run(lens: &ThreadLens, cands: Vec<Candidate>) -> Pack {
    let none = BTreeSet::new();
    let asker = Asker {
        session_id: "ses_here",
        place: &Place::Private,
        in_context: &none,
        labeled: &none,
        links: &[],
        now_ms: 0,
        retention: &BTreeMap::new(),
        threads: Some(lens),
    };
    recall(&Baseline::default(), &asker, cands, &Params::default())
}

fn admitted(p: &Pack) -> Vec<&str> {
    p.admitted
        .iter()
        .map(|a| a.candidate.node_id.as_str())
        .collect()
}

fn reason_of(p: &Pack, node: &str) -> Option<Reason> {
    p.dropped
        .iter()
        .find(|d| d.candidate.node_id == node)
        .map(|d| d.reason)
}

/// A note Jev read as about the message's subject (`admitted`,
/// theseus-98bz.10) is admitted under the bar, and pays the penalty: it ranks
/// behind the asking thread's own note it would have outranked.
#[test]
fn a_note_jev_admitted_pays_and_passes_under_the_bar() {
    let mut l = lens(Some("here"), &[], &[]);
    let cands = || {
        vec![
            cand("own", "ses_unthreaded", 0.020),
            cand("far_low", "ses_pr", 0.025),
        ]
    };
    assert_eq!(
        reason_of(&run(&l, cands()), "far_low"),
        Some(Reason::Thread)
    );
    l.admitted.insert("far_low".into());
    // 0.025 * 0.65 = 0.016 < 0.020: in, behind the own note.
    assert_eq!(admitted(&run(&l, cands())), ["own", "far_low"]);
}

/// Above the bar another thread's note is admitted but ranks behind what its
/// score alone would put it; under the bar it is withheld, as `thread`.
#[test]
fn another_threads_hit_pays_and_is_withheld_under_the_bar() {
    let l = lens(Some("here"), &[], &[]);
    let p = run(
        &l,
        vec![
            cand("far_top", "ses_pr", 0.040),
            cand("own", "ses_unthreaded", 0.036),
            cand("far_mid", "ses_pr", 0.033),
            cand("far_low", "ses_pr", 0.020),
        ],
    );
    // 0.040 * 0.65 = 0.026 < 0.036: the unthreaded note now leads.
    assert_eq!(admitted(&p), ["own", "far_top", "far_mid"]);
    assert_eq!(reason_of(&p, "far_low"), Some(Reason::Thread(Cause::Bar)));
}

/// Naming the thread waives the penalty and the bar both.
#[test]
fn naming_the_thread_waives_the_distance() {
    let l = lens(Some("here"), &["pr-round"], &[]);
    let p = run(
        &l,
        vec![
            cand("far_top", "ses_pr", 0.040),
            cand("far_low", "ses_pr", 0.002),
        ],
    );
    assert_eq!(admitted(&p), ["far_top", "far_low"]);
}

/// An unthreaded session recalls as it did: no distance for a thread's note.
#[test]
fn an_unthreaded_session_recalls_as_before() {
    let l = lens(None, &[], &[]);
    let p = run(&l, vec![cand("far_low", "ses_pr", 0.002)]);
    assert_eq!(admitted(&p), ["far_low"]);
}

/// A sealed thread's note is withheld from every other thread and from an
/// unthreaded session, named or not, and a held thread reads itself.
#[test]
fn a_sealed_threads_note_is_withheld_but_for_its_own_thread() {
    let sealed = ["security-fix"];
    for here in [Some("pr-round"), None] {
        let p = run(
            &lens(here, &["security-fix"], &sealed),
            vec![cand("n", "ses_fix", 0.04)],
        );
        assert!(admitted(&p).is_empty());
        assert_eq!(reason_of(&p, "n"), Some(Reason::Sealed));
    }
    let own = run(
        &lens(Some("security-fix"), &[], &sealed),
        vec![cand("n", "ses_fix", 0.04)],
    );
    assert_eq!(admitted(&own), ["n"]);
}

/// A candidate whose chunk holds the entities `has` and shares `shares` with the message.
fn with_entities(mut c: Candidate, has: &[&str], shares: &[&str]) -> Candidate {
    c.entities = has.iter().map(|e| e.to_string()).collect();
    c.matched = shares.iter().map(|e| e.to_string()).collect();
    c
}

/// Mechanism 3, when another stream's notes are pulled (theseus-wncdv): the
/// shape of a question (a port, a ticket) is no reason to pull another
/// stream's note. A candidate of another thread whose chunk holds entities
/// that the message names none of is dropped as `thread`, however high it
/// scores; one that shares an entity, or holds none, is judged by the bar.
#[test]
fn a_question_shape_alone_pulls_no_other_streams_note() {
    let mut l = lens(Some("here"), &[], &[]);
    l.needs_entity = true;
    let p = run(
        &l,
        vec![
            with_entities(cand("apart", "ses_fix", 0.040), &["beads:SEC-4412"], &[]),
            with_entities(
                cand("shared", "ses_fix", 0.039),
                &["beads:SEC-4412", "file:app.py"],
                &["file:app.py"],
            ),
            cand("plain", "ses_pr", 0.038),
        ],
    );
    assert_eq!(admitted(&p), ["shared", "plain"]);
    assert_eq!(reason_of(&p, "apart"), Some(Reason::Thread(Cause::Entity)));
    // Off, the bar alone decides, as before.
    l.needs_entity = false;
    let p = run(
        &l,
        vec![with_entities(
            cand("apart", "ses_fix", 0.040),
            &["beads:SEC-4412"],
            &[],
        )],
    );
    assert_eq!(admitted(&p), ["apart"]);
}

/// Naming the thread waives the entity rule too; the thread's own notes and
/// an unthreaded asker's recall never meet it.
#[test]
fn the_entity_rule_is_waived_by_a_name_and_never_meets_a_threads_own_notes() {
    let apart = |node: &str, session: &str| {
        with_entities(cand(node, session, 0.040), &["beads:SEC-4412"], &[])
    };
    let mut named = lens(Some("here"), &["security-fix"], &[]);
    named.needs_entity = true;
    assert_eq!(admitted(&run(&named, vec![apart("n", "ses_fix")])), ["n"]);
    let mut own = lens(Some("security-fix"), &[], &[]);
    own.needs_entity = true;
    assert_eq!(admitted(&run(&own, vec![apart("n", "ses_fix")])), ["n"]);
    let mut none = lens(None, &[], &[]);
    none.needs_entity = true;
    assert_eq!(admitted(&run(&none, vec![apart("n", "ses_fix")])), ["n"]);
}

/// Mechanism 1, recall's provenance (theseus-wncdv): streams share the shape
/// of their facts, so another stream's hit scores nearly as high as the
/// thread's own. Another stream's hit pays its penalty and must still outrank
/// the asking thread's own best hit; one that does not is withheld.
#[test]
fn another_stream_must_outrank_what_the_thread_already_holds() {
    let cands = || {
        vec![
            cand("own", "ses_pr", 0.030),
            cand("far_hi", "ses_fix", 0.050),
            cand("far_mid", "ses_fix", 0.045),
            cand("far_lo", "ses_fix", 0.034),
        ]
    };
    let mut l = lens(Some("pr-round"), &[], &[]);
    l.top_fused = 0.05;
    l.own_top = 0.030;
    // Without the rule the bar alone decides: the other's two above it come in beside the thread's own.
    let p = run(&l, cands());
    assert_eq!(admitted(&p).len(), 3, "{:?}", admitted(&p));
    l.beats_own = true;
    // 0.050 * 0.65 = 0.0325 > 0.030; 0.045 * 0.65 = 0.02925 does not.
    let p = run(&l, cands());
    assert_eq!(admitted(&p), ["far_hi", "own"]);
    assert_eq!(reason_of(&p, "far_mid"), Some(Reason::Thread(Cause::Own)));
    assert_eq!(reason_of(&p, "far_lo"), Some(Reason::Thread(Cause::Bar)));
    // Naming the other thread waives it; with nothing of its own the thread
    // admits what is above the bar.
    let mut named = l.clone();
    named.waived.insert("security-fix".into());
    assert_eq!(admitted(&run(&named, cands())).len(), 4);
    let mut bare = l;
    bare.own_top = 0.0;
    assert_eq!(
        admitted(&run(&bare, vec![cand("far_mid", "ses_fix", 0.045)])),
        ["far_mid"]
    );
}

/// The place's own conversation, where each stream's first messages run before
/// its thread exists, is another stream's to a threaded asker: it pays the
/// distance and a name never waives it; an unthreaded asker recalls it as before.
#[test]
fn the_places_own_conversation_is_another_stream_to_a_thread() {
    let mut l = lens(Some("pr-round"), &["security-fix"], &[]);
    l.base = true;
    let p = run(
        &l,
        vec![
            cand("base_hi", "ses_unthreaded", 0.040),
            cand("base_lo", "ses_unthreaded", 0.020),
        ],
    );
    assert_eq!(admitted(&p), ["base_hi"]);
    assert_eq!(reason_of(&p, "base_lo"), Some(Reason::Thread(Cause::Base)));
    // 0.040 * 0.65 is below 0.030 of its own: withheld.
    l.beats_own = true;
    l.own_top = 0.030;
    let p = run(&l, vec![cand("base_hi", "ses_unthreaded", 0.040)]);
    assert_eq!(reason_of(&p, "base_hi"), Some(Reason::Thread(Cause::Base)));
    l.base = false;
    let p = run(&l, vec![cand("base_lo", "ses_unthreaded", 0.020)]);
    assert_eq!(admitted(&p), ["base_lo"], "off, it recalls as before");
    let mut none = lens(None, &[], &[]);
    none.base = true;
    let p = run(&none, vec![cand("base_lo", "ses_unthreaded", 0.020)]);
    assert_eq!(
        admitted(&p),
        ["base_lo"],
        "an unthreaded asker is as before"
    );
}

/// A node a split moved belongs to the thread it moved to, whichever session
/// it is still in: another thread's moved notes pay the distance, the thread's
/// own do not, and a sealed thread's never come in.
#[test]
fn a_moved_node_belongs_to_its_thread_not_its_session() {
    let moved = |held: bool| {
        BTreeMap::from([(
            "moved".to_string(),
            SessionThread {
                slug: "security-fix".into(),
                held,
            },
        )])
    };
    let c = || vec![cand("moved", "ses_unthreaded", 0.002)];
    let mut l = lens(Some("pr-round"), &[], &[]);
    l.of_node = moved(false);
    assert_eq!(
        reason_of(&run(&l, c()), "moved"),
        Some(Reason::Thread(Cause::Bar)),
        "another thread's"
    );
    let mut own = lens(Some("security-fix"), &[], &[]);
    own.of_node = moved(false);
    assert_eq!(admitted(&run(&own, c())), ["moved"], "its own thread's");
    for here in [Some("pr-round"), None] {
        let mut sealed = lens(here, &["security-fix"], &[]);
        sealed.of_node = moved(true);
        assert_eq!(reason_of(&run(&sealed, c()), "moved"), Some(Reason::Sealed));
    }
}

proptest! {
    /// Over generated recalls (100 seeded cases), with the entity rule on,
    /// no candidate of another thread or of the place's own conversation
    /// that holds entities the message names none of is admitted unless the
    /// message names its thread, whatever it scores.
    #[test]
    fn an_apart_note_of_another_stream_never_comes_in(
        here in prop::sample::select(vec![Some("pr-round"), Some("other")]),
        names in prop::collection::vec(prop::sample::select(vec!["pr-round", "security-fix"]), 0..3),
        scores in prop::collection::vec(0.001f64..0.05, 1..12),
        sessions in prop::collection::vec(prop::sample::select(vec!["ses_fix", "ses_pr", "ses_x"]), 12),
    ) {
        let cands: Vec<Candidate> = scores
            .iter()
            .enumerate()
            .map(|(i, f)| with_entities(cand(&format!("n{i}"), sessions[i], *f), &["beads:T-1"], &[]))
            .collect();
        let mut l = lens(here, &names, &[]);
        l.needs_entity = true;
        l.base = true;
        let p = run(&l, cands);
        for a in &p.admitted {
            let s = a.candidate.session_id.as_str();
            let slug = match s {
                "ses_fix" => Some("security-fix"),
                "ses_pr" => Some("pr-round"),
                _ => None,
            };
            let own = slug.is_some() && slug == here;
            let named = slug.is_some_and(|g| names.contains(&g));
            prop_assert!(own || named, "{:?}", a.candidate);
        }
    }


    /// Over generated recalls (100 seeded cases), a sealed thread's note is
    /// never admitted into another thread's or an unthreaded session's pack,
    /// however high its score or whatever the message names.
    #[test]
    fn a_sealed_note_never_comes_in(
        here in prop::sample::select(vec![None, Some("pr-round"), Some("other")]),
        names in prop::collection::vec(prop::sample::select(vec!["pr-round", "security-fix"]), 0..3),
        scores in prop::collection::vec(0.001f64..0.05, 1..12),
        sessions in prop::collection::vec(prop::sample::select(vec!["ses_fix", "ses_pr", "ses_x"]), 12),
    ) {
        let cands: Vec<Candidate> = scores
            .iter()
            .enumerate()
            .map(|(i, f)| cand(&format!("n{i}"), sessions[i], *f))
            .collect();
        let l = lens(here, &names, &["security-fix"]);
        let p = run(&l, cands);
        for a in &p.admitted {
            prop_assert!(a.candidate.session_id != "ses_fix", "{:?}", a.candidate);
        }
    }
}

/// The threadbench's msg-113 shape (theseus-98bz.2): the thread's own note,
/// already in context, is the index's best at 0.104; another stream's note at
/// share 0.93 pays down to 0.063 and does not outrank it. The drop says
/// `thread_own` with the three numbers; the same note in the place's own
/// conversation says `thread_base`, which wins over the other rules.
#[test]
fn a_drop_says_which_rule_and_the_numbers_it_weighed() {
    let mut l = lens(Some("pr-round"), &[], &[]);
    (l.top_fused, l.own_top, l.beats_own, l.base) = (0.104, 0.104, true, true);
    let fused = 0.93 * 0.104;
    let drop_of = |session: &str| {
        let p = run(&l, vec![cand("near", session, fused)]);
        let d = p
            .dropped
            .iter()
            .find(|d| d.candidate.node_id == "near")
            .cloned();
        d.map(|d| (d.reason.as_str(), d.distance.unwrap()))
    };
    let (reason, d) = drop_of("ses_fix").unwrap();
    assert_eq!(reason, "thread_own");
    assert!((d.own_top - 0.104).abs() < 1e-9, "{d:?}");
    assert!((d.share - 0.93).abs() < 1e-9, "{d:?}");
    assert!((d.paid - 0.063).abs() < 5e-4, "{d:?}");
    let (reason, base) = drop_of("ses_home").unwrap();
    assert_eq!(
        (reason, base.share, base.paid),
        ("thread_base", d.share, d.paid)
    );
    assert_eq!(Reason::Thread(Cause::Own).family(), "thread");
}
