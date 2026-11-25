//! The contract's shells, driven by fakes: a view built end to end, and the admission table iterated.

use std::collections::BTreeSet;

use super::budget::{fit, Budget, Overflow, HEADING_TOKENS};
use super::fold::fold_due;
use super::gate::{Admitted, Gate};
use super::record::{view_record, ViewRecord, VIEW_PREFIX};
use super::select::{Band, Selector, ADMISSION};
use super::*;

struct FakeTimeline;

impl Timeline for FakeTimeline {
    fn newest(
        &self,
        _: &TimelineRoot,
        before: Option<&Cursor>,
        n: usize,
    ) -> anyhow::Result<Vec<ExchangeRef>> {
        let all = (1..=5u64).rev().map(|i| ExchangeRef {
            turn_id: format!("t{i}"),
            session_id: "s".into(),
            at_ms: i * 1000,
        });
        Ok(all
            .filter(|e| before.is_none_or(|c| e.cursor() < *c))
            .take(n)
            .collect())
    }
}

struct FakeSnapshot(FakeTimeline);

impl Snapshot for FakeSnapshot {
    fn now_ms(&self) -> u64 {
        9_000
    }
    fn timeline(&self) -> &dyn Timeline {
        &self.0
    }
    fn at_hand(&self, _: &Anchor) -> Option<AtHand> {
        None
    }
}

/// The floor: the newest three exchanges of the place, whole.
struct FakeFloor;

impl Selector for FakeFloor {
    fn band(&self) -> Band {
        Band::Floor
    }
    fn select(&self, pov: &PointOfView, snap: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let Anchor::Place(place) = &pov.anchor else {
            return Ok(Vec::new());
        };
        let root = TimelineRoot::Place(place.clone());
        Ok(snap
            .timeline()
            .newest(&root, None, 3)?
            .into_iter()
            .map(|e| piece(place, &e, self.band()))
            .collect())
    }
}

/// Recall: one piece from elsewhere.
struct FakeRecall;

impl Selector for FakeRecall {
    fn band(&self) -> Band {
        Band::Recall
    }
    fn select(&self, _: &PointOfView, _: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        let e = ExchangeRef {
            turn_id: "far".into(),
            session_id: "s2".into(),
            at_ms: 10,
        };
        let mut p = piece("discord:9", &e, self.band());
        p.marks.author = Author::Theseus;
        Ok(vec![p])
    }
}

/// Admits what carries a place, denies the rest.
struct FakeGate;

impl Gate for FakeGate {
    fn admit(&self, _: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        piece
            .marks
            .place
            .is_some()
            .then(|| Admitted::for_tests(piece))
    }
}

fn piece(place: &str, e: &ExchangeRef, band: Band) -> Piece {
    let pos = Pos {
        at_ms: e.at_ms,
        id: e.turn_id.clone(),
    };
    Piece {
        range: SourceRange {
            space: Space::Exchanges {
                place: place.into(),
            },
            first: pos.clone(),
            last: pos,
        },
        band,
        slot: band.row().slot,
        reasons: BTreeSet::from([if band == Band::Floor {
            Reason::Floor
        } else {
            Reason::Recalled
        }]),
        form: Form::Whole,
        marks: Marks {
            place: Some(place.into()),
            audience: Audience::Private,
            author: Author::His {
                may_authorise: true,
            },
            outside: false,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole: 100,
            spoken: 40,
            line: 10,
        },
        handle: Handle(format!("x:{}", e.turn_id)),
        unit: None,
    }
}

/// The owner's point of view at the terminal, as the turn's view step builds one.
fn pov(present: Present, snap: &dyn Snapshot) -> PointOfView {
    let anchor = Anchor::Place("local".into());
    PointOfView {
        at_hand: snap.at_hand(&anchor),
        anchor,
        person: Some("owner".into()),
        present,
        now_ms: snap.now_ms(),
    }
}

#[test]
fn a_view_is_built_from_fakes_through_the_shells() {
    let snap = FakeSnapshot(FakeTimeline);
    let pov = pov(Present::OwnerAlone, &snap);
    let selectors: Vec<Box<dyn Selector>> = vec![Box::new(FakeRecall), Box::new(FakeFloor)];
    let mut denied = piece(
        "x",
        &ExchangeRef {
            turn_id: "d".into(),
            session_id: "s".into(),
            at_ms: 1,
        },
        Band::Named,
    );
    denied.marks.place = None;
    let admitted: Vec<Admitted<Piece>> = selectors
        .iter()
        .flat_map(|s| s.select(&pov, &snap).expect("selects"))
        .chain([denied])
        .filter_map(|p| FakeGate.admit(&pov, p))
        .collect();
    let view = fit(&pov, admitted, &Budget::new(200_000, 8_000)).expect("fits");
    let order: Vec<(Band, &str)> = view
        .shown
        .iter()
        .map(|s| (s.piece.band, s.piece.range.last.id.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            (Band::Floor, "t5"),
            (Band::Floor, "t4"),
            (Band::Floor, "t3"),
            (Band::Recall, "far")
        ],
        "table order, then newest first; the denied piece is not mentioned"
    );
    assert_eq!(
        view.tokens,
        4 * (100 + HEADING_TOKENS),
        "each shown piece is charged its heading"
    );
    assert_eq!(Label::of(&view.shown[0].piece, &pov), Label::Here);
    assert_eq!(Label::of(&view.shown[3].piece, &pov), Label::Testimony);
    let record = ViewRecord::new("c1", view);
    assert_eq!(record.key(), format!("{VIEW_PREFIX}c1"));
    view_record(&record).expect("encodes");
    let back: ViewRecord =
        serde_json::from_value(serde_json::to_value(&record).expect("json")).expect("decodes");
    assert_eq!(back, record);
}

#[test]
fn a_floor_that_cannot_fit_is_a_typed_overflow() {
    let snap = FakeSnapshot(FakeTimeline);
    let pov = pov(Present::OwnerAlone, &snap);
    let admitted: Vec<_> = FakeFloor
        .select(&pov, &snap)
        .expect("selects")
        .into_iter()
        .map(Admitted::for_tests)
        .collect();
    let err = fit(&pov, admitted, &Budget::new(1_000, 900)).expect_err("does not fit");
    assert_eq!(
        err,
        Overflow::Floor {
            handle: Handle("x:t5".into()),
            tokens: 3 * (100 + HEADING_TOKENS),
            room: 100
        }
    );
}

#[test]
fn the_admission_table_has_each_band_once_and_its_floors_are_the_floor_and_now() {
    let bands: BTreeSet<Band> = ADMISSION.iter().map(|r| r.band).collect();
    assert_eq!(bands.len(), ADMISSION.len(), "each band has one row");
    for (i, row) in ADMISSION.iter().enumerate() {
        assert_eq!(row.band.priority(), i);
        assert_eq!(
            row.floor,
            matches!(row.band, Band::Floor | Band::Now),
            "{:?}",
            row.band
        );
    }
    assert_eq!(ADMISSION[0].band, Band::Frame, "the frame is counted first");
}

#[test]
fn a_stored_piece_needs_the_gate_and_folds_are_by_count() {
    let stored = piece(
        "local",
        &ExchangeRef {
            turn_id: "t1".into(),
            session_id: "s".into(),
            at_ms: 1,
        },
        Band::Now,
    );
    assert!(
        Admitted::code(stored.clone()).is_none(),
        "a stored piece needs the gate"
    );
    let relabelled = Piece {
        range: SourceRange {
            space: Space::Code {
                part: "clock".into(),
            },
            ..stored.range.clone()
        },
        ..stored
    };
    assert!(
        Admitted::code(relabelled.clone()).is_none(),
        "a stored piece relabelled as code still needs the gate"
    );
    let clock = Piece {
        marks: Marks {
            place: None,
            ..relabelled.marks.clone()
        },
        ..relabelled
    };
    assert!(
        Admitted::code(clock).is_some(),
        "code's own text has nothing to permit"
    );
    assert!(!fold_due(49) && fold_due(50) && !fold_due(0));
    let page = FakeTimeline
        .newest(
            &TimelineRoot::Place("local".into()),
            Some(&Cursor {
                at_ms: 3000,
                turn_id: "t3".into(),
            }),
            10,
        )
        .expect("reads");
    assert_eq!(
        page.iter().map(|e| e.turn_id.as_str()).collect::<Vec<_>>(),
        ["t2", "t1"]
    );
}
