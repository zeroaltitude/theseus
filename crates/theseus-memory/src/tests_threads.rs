//! The thread distance and the seal through recall's pipeline
//! (theseus-f2g0): a cross-thread hit pays, a hit under the bar is withheld,
//! naming the thread waives both, and a sealed thread's note never comes in,
//! whoever asks, over generated recalls.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use crate::recall::{recall, Asker, Candidate, Pack, Params, Place, Reason};
use crate::science::Baseline;
use crate::threads::{SessionThread, ThreadLens};

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
    assert_eq!(reason_of(&p, "far_low"), Some(Reason::Thread));
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

proptest! {
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
