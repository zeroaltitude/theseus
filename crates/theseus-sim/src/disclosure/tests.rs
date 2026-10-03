//! The simulator's own tests: its world generator, its oracle, and that a
//! seed reproduces.

use std::collections::BTreeSet;

use super::atoms::{covers, fits_any, scan, Atoms, Aud, Origin, Views, R};
use super::check::paired;
use super::world::{Shared, OWNER};
use super::{run, Params};

#[test]
fn a_seed_makes_the_same_world_and_another_seed_another() {
    let a = Shared::generate(7, 0.15);
    let b = Shared::generate(7, 0.15);
    let summary = |s: &Shared| {
        format!(
            "{:?} {:?} {:?}",
            s.people,
            s.channels
                .iter()
                .map(|c| (c.id, c.truth.clone(), c.readable))
                .collect::<Vec<_>>(),
            s.files.iter().map(|f| f.text.clone()).collect::<Vec<_>>()
        )
    };
    assert_eq!(summary(&a), summary(&b));
    let others: BTreeSet<String> = (1..20)
        .map(|s| summary(&Shared::generate(s, 0.15)))
        .collect();
    assert!(
        others.len() > 10,
        "seeds make different worlds: {}",
        others.len()
    );
}

/// The world has what every invariant needs: an owner and others, each
/// channel viewed by the owner, files of both trees and context files of
/// both kinds, each with an atom of the readers its tree says.
#[test]
fn the_world_has_an_owner_others_channels_and_both_trees() {
    for seed in 0..50 {
        let w = Shared::generate(seed, 0.15);
        assert_eq!(w.people[0], OWNER);
        assert!((5..=7).contains(&w.people.len()), "{seed}: {:?}", w.people);
        assert!((2..=4).contains(&w.channels.len()));
        for c in &w.channels {
            assert!(
                c.truth.contains(&OWNER),
                "{seed}: the owner views #{}",
                c.name
            );
            assert!(
                c.pushed.is_none(),
                "nothing pushed before the binding connects"
            );
        }
        for f in w.files.iter().chain(&w.context) {
            let ids = scan(&f.text);
            assert_eq!(ids.len(), 1, "{}", f.text);
            let a = w.atoms.get(ids[0]).unwrap();
            assert_eq!(
                a.readers,
                if f.public { R::Public } else { R::Owner },
                "{}",
                f.path
            );
        }
        assert_eq!(w.files.iter().filter(|f| f.public).count(), 3);
        assert_eq!(w.context.len(), 2);
    }
}

#[test]
fn a_marker_is_found_whole_once_and_never_inside_another_word() {
    let mut atoms = Atoms::default();
    let (a, ma) = atoms.mint(R::Owner, Origin::Cli);
    let (b, mb) = atoms.mint(R::Public, Origin::Page);
    let text = format!("x{ma} and {mb}{ma} zq12 qz zqqz zq9x9qz");
    assert_eq!(scan(&text), vec![a, b]);
    // A marker cut across two deltas is found once both have streamed.
    let (left, right) = mb.split_at(3);
    assert!(scan(left).is_empty());
    assert_eq!(scan(&format!("{left}{right}")), vec![b]);
}

fn set(ids: &[u64]) -> BTreeSet<u64> {
    ids.iter().copied().collect()
}

/// The oracle's `covers`, case by case (§2.5): the owner alone is covered by
/// anything; a DM by its person; a channel by readers naming every viewer
/// but the owner, or by its own words; a channel nobody can read only by
/// public text and its own words.
#[test]
fn the_oracle_covers_an_audience_only_when_every_member_may_read() {
    let (ana, ben, lab, den) = (11, 12, 21, 22);
    let views: Views = [(lab, Some(set(&[OWNER, ana]))), (den, None)].into();
    let place = |members: Option<BTreeSet<u64>>| Aud::Place {
        channel: lab,
        members,
    };
    assert!(covers(&R::Owner, &Aud::Owner, OWNER, &views));
    assert!(!covers(&R::Owner, &Aud::Person(ana), OWNER, &views));
    assert!(covers(&R::Owner, &Aud::Person(OWNER), OWNER, &views));
    assert!(covers(
        &R::People(set(&[ana])),
        &Aud::Person(ana),
        OWNER,
        &views
    ));
    assert!(
        covers(&R::Place(lab), &Aud::Person(ana), OWNER, &views),
        "ana views #lab"
    );
    assert!(!covers(&R::Place(lab), &Aud::Person(ben), OWNER, &views));
    assert!(
        !covers(&R::Place(den), &Aud::Person(ana), OWNER, &views),
        "#den unread"
    );
    let two = place(Some(set(&[OWNER, ana])));
    assert!(!covers(&R::Owner, &two, OWNER, &views));
    assert!(covers(&R::People(set(&[ana])), &two, OWNER, &views));
    assert!(!covers(&R::People(set(&[ben])), &two, OWNER, &views));
    assert!(
        covers(&R::Place(lab), &place(None), OWNER, &views),
        "its own words"
    );
    assert!(!covers(
        &R::People(set(&[ana])),
        &place(None),
        OWNER,
        &views
    ));
    assert!(covers(&R::Public, &place(None), OWNER, &views));
    assert!(fits_any(&R::Place(lab), Some(lab)) && fits_any(&R::Public, Some(lab)));
    assert!(!fits_any(&R::Place(den), Some(lab)) && !fits_any(&R::Owner, Some(lab)));
}

/// A graduation widens its atoms: an owner-only atom graduated to a channel
/// may then reach that channel, and still not another.
#[test]
fn a_graduated_atom_may_reach_its_new_readers() {
    let mut atoms = Atoms::default();
    let (f, _) = atoms.mint(R::Owner, Origin::File("private/a".into()));
    let views: Views = [(21, Some(set(&[OWNER, 11]))), (22, Some(set(&[OWNER, 12])))].into();
    let lab = Aud::Place {
        channel: 21,
        members: Some(set(&[OWNER, 11])),
    };
    let den = Aud::Place {
        channel: 22,
        members: Some(set(&[OWNER, 12])),
    };
    assert!(!atoms.allowed(f, &lab, OWNER, &views));
    atoms.grant(f, R::Place(21));
    assert!(atoms.allowed(f, &lab, OWNER, &views));
    assert!(!atoms.allowed(f, &den, OWNER, &views));
    assert!(atoms.fits_any(f, 21) && !atoms.fits_any(f, 22));
}

/// The pairing check finds a call with no result after it, and a result
/// that answers no call before it.
#[test]
fn the_pairing_check_finds_a_lost_result_and_a_stray_one() {
    use serde_json::json;
    let call = json!({"role": "assistant", "content": [{"type": "tool_use", "id": "c1", "name": "fs_read", "input": {}}]});
    let result = json!({"role": "user", "content": [{"type": "tool_result", "tool_use_id": "c1", "content": "x"}]});
    let other = json!({"role": "user", "content": [{"type": "text", "text": "hi"}]});
    assert!(paired(&[other.clone(), call.clone(), result.clone()]).is_ok());
    assert!(paired(&[other.clone(), call.clone(), other.clone()]).is_err());
    assert!(paired(std::slice::from_ref(&call)).is_err());
    assert!(paired(&[other, result]).is_err());
}

/// A seed reproduces: two runs of it make the same decisions, the same
/// counts, and the same trace; another seed differs.
#[test]
fn a_seed_reproduces_its_run_exactly() {
    let a = run(&Params::new(3, 25)).unwrap();
    let b = run(&Params::new(3, 25)).unwrap();
    let c = run(&Params::new(4, 25)).unwrap();
    let same = |r: &super::Report| super::Report {
        wall_ms: 0,
        ..r.clone()
    };
    assert_eq!(same(&a), same(&b));
    assert_ne!(a.trace, c.trace);
    assert!(a.compiled > 0 && a.turns > 0, "{a:?}");
}

/// A known gap excuses only the atoms it let out, each named first by its
/// gap's rule; none is open since 19d. So an owner-only context file's atom
/// (what theseus-42ub let out), like any other, is excused by none, and a
/// run fails on it as a strict run does.
#[test]
fn a_known_gap_excuses_only_what_it_let_out() {
    let mut atoms = Atoms::default();
    let (ctx, _) = atoms.mint(R::Owner, Origin::Context("private/context.md".into()));
    let (file, _) = atoms.mint(R::Owner, Origin::File("private/notes-0.txt".into()));
    assert_eq!(atoms.gap_for(ctx), None);
    assert_eq!(atoms.gap_for(file), None);
    atoms.let_out(file, "theseus-0000");
    assert_eq!(
        atoms.gap_for(file),
        Some("theseus-0000"),
        "let out for good"
    );
    assert_eq!(atoms.gap_for(ctx), None);
}
