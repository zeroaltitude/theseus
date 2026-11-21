//! The budgeter's rules, one test each, over hand-made pieces.

use std::collections::BTreeSet;

use super::super::gate::Admitted;
use super::super::select::Band;
use super::*;
use crate::view::{Anchor, Audience, Author, Marks, Pos, Present, Reason, Sizes, Space};

fn pov() -> PointOfView {
    PointOfView {
        anchor: Anchor::Place("local".into()),
        person: Some("owner".into()),
        present: Present::OwnerAlone,
        at_hand: None,
        now_ms: 1_000_000,
    }
}

fn piece(band: Band, space: Space, at: u64, form: Form, sizes: (u64, u64, u64)) -> Piece {
    let pos = Pos {
        at_ms: at,
        id: format!("t{at}"),
    };
    Piece {
        range: SourceRange {
            space,
            first: pos.clone(),
            last: pos,
        },
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([Reason::Window]),
        form,
        marks: Marks {
            place: Some("local".into()),
            audience: Audience::Private,
            author: Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole: sizes.0,
            spoken: sizes.1,
            line: sizes.2,
        },
        handle: Handle(format!("h:{at}")),
        unit: None,
    }
}

fn here() -> Space {
    Space::Exchanges {
        place: "local".into(),
    }
}

fn fact(thing: &str) -> Space {
    Space::Facts {
        thing: thing.into(),
    }
}

fn budget(ceiling: u64, here_allowance: u64) -> Budget {
    Budget {
        ceiling,
        here_allowance,
        provider_window: 1_000_000,
        reserve: 0,
    }
}

fn run(pieces: Vec<Piece>, b: &Budget) -> Result<View, Overflow> {
    fit(&pov(), pieces.into_iter().map(Admitted::new).collect(), b)
}

fn forms(v: &View) -> Vec<(Band, u64, Form)> {
    v.shown
        .iter()
        .map(|s| (s.piece.band, s.piece.range.last.at_ms, s.form))
        .collect()
}

#[test]
fn an_exchange_asked_for_twice_is_shown_once_in_slot_g_with_both_reasons() {
    let mut past = piece(Band::Past7, here(), 5, Form::Line, (100, 40, 10));
    past.reasons = BTreeSet::from([Reason::Past]);
    let window = piece(Band::WindowWords, here(), 5, Form::Spoken, (100, 40, 10));
    let v = run(vec![past, window], &budget(60_000, 150_000)).expect("fits");
    assert_eq!(v.shown.len(), 1);
    let s = &v.shown[0];
    assert_eq!(
        (s.piece.slot, s.piece.band, s.form),
        (Slot::Here, Band::WindowWords, Form::Spoken)
    );
    assert_eq!(
        s.piece.reasons,
        BTreeSet::from([Reason::Past, Reason::Window])
    );
    assert_eq!(v.dropped.len(), 1);
    assert_eq!(v.dropped[0].why, DropWhy::Merged);
    assert!(
        v.residue.is_empty(),
        "a merged piece is shown, so it is not counted"
    );
    assert_eq!(v.tokens, 40);
}

#[test]
fn over_the_ceiling_a_piece_steps_down_a_form_before_it_is_dropped() {
    let a = piece(Band::Recall, fact("a"), 2, Form::Whole, (100, 40, 10));
    let b = piece(Band::Recall, fact("b"), 1, Form::Whole, (100, 40, 10));
    let v = run(vec![a.clone(), b.clone()], &budget(140, 150_000)).expect("fits");
    assert_eq!(
        forms(&v),
        [
            (Band::Recall, 2, Form::Whole),
            (Band::Recall, 1, Form::Spoken)
        ],
        "the oldest steps down first, and only as far as needed"
    );
    let v = run(vec![a.clone(), b.clone()], &budget(60, 150_000)).expect("fits");
    assert_eq!(
        forms(&v),
        [
            (Band::Recall, 2, Form::Spoken),
            (Band::Recall, 1, Form::Line)
        ],
        "every piece of a band steps down before any steps again"
    );
    let v = run(vec![a, b], &budget(15, 150_000)).expect("fits");
    assert_eq!(forms(&v), [(Band::Recall, 2, Form::Line)]);
    assert_eq!(v.dropped.len(), 1);
    assert_eq!(
        (v.dropped[0].why, v.dropped[0].tokens),
        (DropWhy::OverBudget, 100)
    );
    assert_eq!(
        v.residue,
        [Residue {
            band: Band::Recall,
            pieces: 1,
            tokens: 100
        }]
    );
}

#[test]
fn cuts_come_from_the_end_of_the_admission_order() {
    let words = piece(Band::ElsewhereWords, fact("w"), 1, Form::Line, (0, 0, 30));
    let recall = piece(Band::Recall, fact("r"), 9, Form::Line, (0, 0, 30));
    let v = run(vec![recall, words], &budget(40, 150_000)).expect("fits");
    assert_eq!(forms(&v), [(Band::ElsewhereWords, 1, Form::Line)]);
}

#[test]
fn the_floor_is_never_cut_and_tool_results_go_before_the_ladder() {
    let floor = piece(Band::Floor, here(), 9, Form::Spoken, (1_000, 100, 10));
    let tools = piece(Band::WindowTools, here(), 9, Form::Whole, (1_000, 100, 10));
    let node = piece(
        Band::Ladder,
        Space::Folds {
            place: "local".into(),
            level: 1,
        },
        3,
        Form::Whole,
        (200, 200, 20),
    );
    let v = run(vec![floor.clone(), tools, node], &budget(60_000, 400)).expect("fits");
    assert_eq!(
        forms(&v),
        [(Band::Floor, 9, Form::Spoken), (Band::Ladder, 3, Form::Whole)],
        "the window's tool traffic is cleared at its own row; the floor's words and the ladder stay"
    );
    let v = run(vec![floor], &budget(1_000, 0)).expect("the floor fits the provider's window");
    assert_eq!(
        forms(&v),
        [(Band::Floor, 9, Form::Spoken)],
        "over every pool, the floor stays"
    );
}

#[test]
fn a_stale_fact_and_its_correction_go_together() {
    let mut stale = piece(Band::Named, fact("x"), 1, Form::Line, (0, 0, 20));
    let mut correction = piece(Band::Named, fact("y"), 2, Form::Line, (0, 0, 20));
    stale.unit = Some("u".into());
    correction.unit = Some("u".into());
    let other = piece(Band::Named, fact("z"), 3, Form::Line, (0, 0, 20));
    let v = run(
        vec![stale.clone(), correction.clone(), other.clone()],
        &budget(45, 0),
    )
    .expect("fits");
    assert_eq!(forms(&v), [(Band::Named, 3, Form::Line)]);
    let whys: Vec<DropWhy> = v.dropped.iter().map(|d| d.why).collect();
    assert_eq!(whys, [DropWhy::UnitCut, DropWhy::OverBudget]);
    let v = run(vec![stale, correction, other], &budget(40, 0)).expect("fits");
    assert_eq!(v.shown.len(), 1, "one unit of two cannot be half shown");
}

#[test]
fn a_floor_over_the_providers_window_is_the_typed_overflow_and_names_the_newest() {
    let a = piece(Band::Floor, here(), 1, Form::Spoken, (0, 70, 1));
    let b = piece(Band::Floor, here(), 2, Form::Spoken, (0, 70, 1));
    let recall = piece(Band::Recall, fact("r"), 9, Form::Line, (0, 0, 30));
    let b_ = Budget {
        provider_window: 120,
        ..budget(60_000, 150_000)
    };
    assert_eq!(
        run(vec![a, b, recall], &b_),
        Err(Overflow::Floor {
            handle: Handle("h:2".into()),
            tokens: 140,
            room: 120
        })
    );
}
