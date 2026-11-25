//! The budgeter's rules, one test each, over hand-made pieces. Every shown piece is charged its heading
//! (`H`) and every band that leaves a residue line its line (`R`).

use std::collections::BTreeSet;

use super::super::gate::Admitted;
use super::super::select::Band;
use super::*;
use crate::view::{Anchor, Audience, Author, Marks, Pos, Present, Reason, Sizes, Space};

const H: u64 = HEADING_TOKENS;
const R: u64 = RESIDUE_LINE_TOKENS;

fn pov() -> PointOfView {
    PointOfView {
        anchor: Anchor::Place("local".into()),
        person: Some("owner".into()),
        present: Present::OwnerAlone,
        at_hand: None,
        now_ms: 1_000_000,
    }
}

fn pos(at: u64) -> Pos {
    Pos {
        at_ms: at,
        id: format!("t{at}"),
    }
}

fn span(
    band: Band,
    space: Space,
    first: u64,
    last: u64,
    form: Form,
    sizes: (u64, u64, u64),
) -> Piece {
    Piece {
        range: SourceRange {
            space,
            first: pos(first),
            last: pos(last),
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
        handle: Handle(format!("h:{last}")),
        unit: None,
    }
}

fn piece(band: Band, space: Space, at: u64, form: Form, sizes: (u64, u64, u64)) -> Piece {
    span(band, space, at, at, form, sizes)
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

fn code(part: &str) -> Space {
    Space::Code { part: part.into() }
}

fn budget(ceiling: u64, here_allowance: u64) -> Budget {
    Budget {
        ceiling,
        here_allowance,
        provider_window: 1_000_000,
        reserve: 0,
        floor_only: false,
        bare: false,
        topic: ceiling,
    }
}

fn run(pieces: Vec<Piece>, b: &Budget) -> Result<View, Overflow> {
    fit(
        &pov(),
        pieces.into_iter().map(Admitted::for_tests).collect(),
        b,
    )
}

fn forms(v: &View) -> Vec<(Band, u64, Form)> {
    v.shown
        .iter()
        .map(|s| (s.piece.band, s.piece.range.last.at_ms, s.form))
        .collect()
}

#[test]
fn an_exchange_asked_for_twice_is_shown_once_in_slot_g_with_both_reasons() {
    let mut elsewhere = piece(Band::ElsewhereWords, here(), 5, Form::Line, (100, 40, 10));
    elsewhere.reasons = BTreeSet::from([Reason::Elsewhere]);
    let window = piece(Band::WindowWords, here(), 5, Form::Spoken, (100, 40, 10));
    let v = run(vec![elsewhere, window], &budget(60_000, 150_000)).expect("fits");
    assert_eq!(v.shown.len(), 1);
    let s = &v.shown[0];
    assert_eq!(
        (s.piece.slot, s.piece.band, s.form),
        (Slot::Here, Band::WindowWords, Form::Spoken)
    );
    assert_eq!(
        s.piece.reasons,
        BTreeSet::from([Reason::Elsewhere, Reason::Window])
    );
    assert_eq!(v.dropped.len(), 1);
    assert_eq!(v.dropped[0].why, DropWhy::Merged);
    assert!(
        v.residue.is_empty(),
        "a merged piece is shown, so it is not counted"
    );
    assert_eq!(
        v.tokens,
        40 + H,
        "identical coverage is charged once, at its largest"
    );
}

#[test]
fn a_partial_overlap_is_charged_at_least_its_union() {
    let a = span(
        Band::Named,
        fact("x"),
        1,
        2,
        Form::Line,
        (40_000, 40_000, 40_000),
    );
    let b = span(
        Band::Named,
        fact("x"),
        2,
        3,
        Form::Line,
        (40_000, 40_000, 40_000),
    );
    let v = run(vec![a.clone(), b.clone()], &budget(200_000, 0)).expect("fits");
    assert_eq!(v.shown.len(), 1);
    assert_eq!(
        v.shown[0].piece.sizes.line, 80_000,
        "the parts summed, never their maximum"
    );
    assert_eq!(
        (
            v.shown[0].piece.range.first.at_ms,
            v.shown[0].piece.range.last.at_ms
        ),
        (1, 3)
    );
    let v = run(vec![a, b], &budget(50_000, 0)).expect("fits");
    assert!(v.shown.is_empty(), "60,000 of union does not fit 50,000");
}

#[test]
fn over_the_ceiling_a_piece_steps_down_a_form_before_it_is_dropped() {
    let a = piece(Band::Recall, fact("a"), 2, Form::Whole, (100, 40, 10));
    let b = piece(Band::Recall, fact("b"), 1, Form::Whole, (100, 40, 10));
    let v = run(
        vec![a.clone(), b.clone()],
        &budget(100 + 40 + 2 * H, 150_000),
    )
    .expect("fits");
    assert_eq!(
        forms(&v),
        [
            (Band::Recall, 2, Form::Whole),
            (Band::Recall, 1, Form::Spoken)
        ],
        "the oldest steps down first, and only as far as needed"
    );
    let v = run(vec![a.clone(), b.clone()], &budget(60 + 2 * H, 150_000)).expect("fits");
    assert_eq!(
        forms(&v),
        [
            (Band::Recall, 2, Form::Spoken),
            (Band::Recall, 1, Form::Line)
        ],
        "every piece of a band steps down before any steps again"
    );
    let v = run(vec![a, b], &budget(10 + H + R, 150_000)).expect("fits");
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
    assert_eq!(v.tokens, 10 + H + R, "the residue line is charged");
}

#[test]
fn cuts_come_from_the_end_of_the_admission_order() {
    let words = piece(Band::ElsewhereWords, fact("w"), 1, Form::Line, (0, 0, 30));
    let recall = piece(Band::Recall, fact("r"), 9, Form::Line, (0, 0, 30));
    let v = run(vec![recall, words], &budget(30 + H + R, 150_000)).expect("fits");
    assert_eq!(forms(&v), [(Band::ElsewhereWords, 1, Form::Line)]);
}

#[test]
fn the_floor_is_never_cut_and_tool_results_go_before_the_ladder() {
    let floor = piece(Band::Floor, here(), 9, Form::Spoken, (1_000, 100, 10));
    let tools = piece(Band::WindowTools, here(), 9, Form::Whole, (1_000, 100, 10));
    let v = run(vec![floor, tools, node()], &budget(60_000, 300 + 2 * H)).expect("fits");
    assert_eq!(
        forms(&v),
        [(Band::Floor, 9, Form::Spoken), (Band::Ladder, 3, Form::Whole)],
        "the window's tool traffic is cleared at its own row; the floor's words and the ladder stay"
    );
    let floor = piece(Band::Floor, here(), 9, Form::Spoken, (1_000, 100, 10));
    let v = run(vec![floor], &budget(1_000, 0)).expect("the floor fits the provider's window");
    assert_eq!(
        forms(&v),
        [(Band::Floor, 9, Form::Spoken)],
        "over its own pool, the floor stays"
    );
}

fn node() -> Piece {
    piece(
        Band::Ladder,
        Space::Folds {
            place: "local".into(),
            level: 1,
        },
        3,
        Form::Whole,
        (200, 200, 20),
    )
}

#[test]
fn protected_text_may_exceed_its_own_pool_when_the_total_fits() {
    let b = budget(60_000, 150_000);
    let recall = piece(Band::Recall, fact("r"), 9, Form::Line, (0, 0, 30));
    let frame = piece(
        Band::Frame,
        code("system"),
        1,
        Form::Whole,
        (70_000, 70_000, 70_000),
    );
    let floor = piece(Band::Floor, here(), 9, Form::Spoken, (0, 100, 10));
    let v = run(vec![frame, floor, recall.clone(), node()], &b).expect("the whole fits");
    assert_eq!(
        forms(&v),
        [(Band::Frame, 1, Form::Whole), (Band::Floor, 9, Form::Spoken), (Band::Ladder, 3, Form::Whole)],
        "the frame stands over the ceiling; recall, not protected, is held to that pool though the whole \
         has room, and G's own pool is untouched"
    );
    let frame = piece(Band::Frame, code("system"), 1, Form::Whole, (100, 100, 100));
    let floor = piece(Band::Floor, here(), 9, Form::Whole, (160_000, 100, 10));
    let v = run(vec![frame, floor, recall, node()], &b).expect("the whole fits");
    assert_eq!(
        forms(&v),
        [
            (Band::Frame, 1, Form::Whole),
            (Band::Floor, 9, Form::Whole),
            (Band::Recall, 9, Form::Line)
        ],
        "the floor stands over G's allowance whole; the ladder, not protected, is held to G"
    );
    assert_eq!(v.tokens, 100 + 160_000 + 30 + 3 * H + R);
    assert!(v.tokens <= b.total());
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
        &budget(20 + H + R, 0),
    )
    .expect("fits");
    assert_eq!(
        forms(&v),
        [(Band::Named, 3, Form::Line)],
        "one unit of two cannot be half shown"
    );
    let whys: Vec<DropWhy> = v.dropped.iter().map(|d| d.why).collect();
    assert_eq!(whys, [DropWhy::UnitCut, DropWhy::OverBudget]);
    let v = run(vec![stale, correction, other], &budget(20 + H + R + 59, 0)).expect("fits");
    assert_eq!(
        v.shown.len(),
        1,
        "room for one more piece is not room for half a unit"
    );
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
            tokens: 2 * (70 + H) + R,
            room: 120
        })
    );
}

#[test]
fn a_view_that_fits_still_fits_with_its_residue_line() {
    let frame = piece(Band::Frame, code("system"), 1, Form::Whole, (100, 100, 100));
    let recall = piece(Band::Recall, fact("r"), 9, Form::Line, (0, 0, 10));
    let room = |provider_window| Budget {
        provider_window,
        ..budget(60_000, 150_000)
    };
    let v = run(vec![frame.clone(), recall.clone()], &room(100 + H + R)).expect("fits");
    assert_eq!(forms(&v), [(Band::Frame, 1, Form::Whole)]);
    assert_eq!(v.tokens, 100 + H + R);
    assert!(
        run(vec![frame, recall], &room(100 + H + R - 1)).is_err(),
        "the residue line a cut leaves is counted"
    );
}

#[test]
fn an_impossible_size_is_the_typed_overflow_not_a_wrap() {
    let a = piece(
        Band::Frame,
        code("a"),
        1,
        Form::Whole,
        (u64::MAX, u64::MAX, u64::MAX),
    );
    let b = piece(Band::Frame, code("b"), 2, Form::Whole, (1, 1, 1));
    let b_ = Budget {
        provider_window: u64::MAX,
        ..budget(u64::MAX / 4, u64::MAX / 4)
    };
    match run(vec![a, b], &b_) {
        Err(Overflow::Floor { tokens, .. }) => assert_eq!(tokens, u64::MAX),
        other => panic!("expected the overflow, got {other:?}"),
    }
}

/// The inner operations for `n` chained overlapping pieces and `n` separate ones in three units, cut hard.
fn ops_for(n: u64) -> u64 {
    let mut pieces = Vec::new();
    for i in 0..n {
        pieces.push(span(
            Band::Named,
            fact("chain"),
            i,
            i + 1,
            Form::Line,
            (0, 0, 1),
        ));
        let mut p = piece(Band::Recall, fact("s"), 2 * i, Form::Whole, (30, 20, 10));
        p.unit = Some(format!("u{}", i % 3));
        pieces.push(p);
    }
    ops::take();
    let v = run(pieces, &budget(n, 0)).expect("fits");
    assert!(!v.residue.is_empty(), "the cut ran");
    ops::take()
}

#[test]
fn the_budgeter_is_linear_in_its_pieces() {
    let small = ops_for(500);
    let large = ops_for(5_000);
    assert!(
        large <= 12 * small,
        "{large} operations at 5,000 against {small} at 500"
    );
    assert!(large <= 20 * 10_000, "{large} operations for 10,000 pieces");
}

/// The topic block (slot H: the task's card, the things named and in play, recall) is held to its own cap
/// inside the ceiling, and cut from its own end: recall first, then what is in play. Nothing outside it is
/// cut for it, however much room the ceiling has.
#[test]
fn the_topic_block_is_held_to_its_cap_and_recall_is_cut_first() {
    let pieces = || {
        let card = |band, thing, at| piece(band, fact(thing), at, Form::Whole, (300, 100, 10));
        vec![
            card(Band::TaskCard, "task", 4),
            card(Band::Named, "named", 3),
            card(Band::InPlay, "played", 2),
            card(Band::Recall, "recalled", 9),
            card(Band::Waiting, "waiting", 1),
        ]
    };
    let capped = |topic| Budget {
        topic,
        ..budget(60_000, 150_000)
    };
    let card = 300 + H;
    let whole = |band, at| (band, at, Form::Whole);
    let uncut = run(pieces(), &budget(60_000, 150_000)).expect("fits");
    assert_eq!(
        forms(&uncut).len(),
        5,
        "under the ceiling alone nothing is cut"
    );

    let v = run(pieces(), &capped(3 * card + 100 + H)).expect("fits");
    assert_eq!(
        forms(&v),
        [
            whole(Band::Waiting, 1),
            whole(Band::TaskCard, 4),
            whole(Band::Named, 3),
            whole(Band::InPlay, 2),
            (Band::Recall, 9, Form::Spoken),
        ],
        "recall steps down first"
    );

    let v = run(pieces(), &capped(3 * card + R)).expect("fits");
    assert_eq!(
        forms(&v),
        [
            whole(Band::Waiting, 1),
            whole(Band::TaskCard, 4),
            whole(Band::Named, 3),
            whole(Band::InPlay, 2),
        ],
        "recall is dropped before any card steps down"
    );
    assert_eq!(
        v.residue,
        [Residue {
            band: Band::Recall,
            pieces: 1,
            tokens: 300
        }],
        "the residue counts the card's own tokens"
    );

    let v = run(pieces(), &capped(2 * card + 2 * R)).expect("fits");
    assert_eq!(
        forms(&v),
        [
            whole(Band::Waiting, 1),
            whole(Band::TaskCard, 4),
            whole(Band::Named, 3),
        ],
        "then what is in play; the task's card and the named things stand"
    );
    let block: u64 = (v.shown.iter())
        .filter(|s| s.piece.slot == Slot::Called)
        .map(|s| s.tokens)
        .sum::<u64>()
        + 2 * R;
    assert!(
        block <= 2 * card + 2 * R,
        "the block within its cap: {block}"
    );
    assert_eq!(
        v.tokens,
        3 * card + 2 * R,
        "the waiting card is not cut for it"
    );
}
