//! The view record kept and read back, and what each reader is shown of it.

use std::collections::BTreeSet;

use theseus_protocol::view::lines;

use super::show::{PieceText, Reader};
use super::*;
use crate::view::budget::{DropWhy, Dropped, Residue, Shown, View};
use crate::view::select::Band;
use crate::view::{
    Anchor, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Present, Reason, Sizes,
    Slot, SourceRange, Space,
};

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

fn range(space: Space, id: &str) -> SourceRange {
    let pos = Pos {
        at_ms: 1,
        id: id.into(),
    };
    SourceRange {
        space,
        first: pos.clone(),
        last: pos,
    }
}

fn exchanges(place: &str) -> Space {
    Space::Exchanges {
        place: place.into(),
    }
}

fn piece(place: &str, id: &str, band: Band, reasons: &[Reason]) -> Piece {
    Piece {
        range: range(exchanges(place), id),
        band,
        slot: band.row().slot,
        reasons: reasons.iter().copied().collect::<BTreeSet<_>>(),
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
        handle: Handle(format!("x:{id}")),
        unit: None,
    }
}

fn shown(p: Piece) -> Shown {
    Shown {
        form: p.form,
        tokens: p.tokens(),
        piece: p,
    }
}

/// A view at `local`: two pieces here, one from another place, a frame piece, one drop of each place.
pub(crate) fn view(now_ms: u64) -> View {
    let mut frame = piece("local", "frame", Band::Frame, &[Reason::Fixed]);
    frame.range = range(
        Space::Code {
            part: "frame".into(),
        },
        "frame",
    );
    frame.marks.place = None;
    let shown = vec![
        shown(frame),
        shown(piece("local", "t2", Band::Floor, &[Reason::Floor])),
        shown(piece(
            "local",
            "t1",
            Band::WindowWords,
            &[Reason::Window, Reason::Named],
        )),
        shown(piece(
            "discord:dm:9",
            "far",
            Band::ElsewhereWords,
            &[Reason::Elsewhere],
        )),
    ];
    let drop = |place: &str, id: &str| Dropped {
        range: range(exchanges(place), id),
        slot: Slot::Past,
        band: Band::Past30,
        reasons: BTreeSet::from([Reason::Past]),
        handle: Handle(format!("x:{id}")),
        tokens: 70,
        why: DropWhy::OverBudget,
    };
    View {
        pov: PointOfView {
            anchor: Anchor::Place("local".into()),
            person: Some("owner".into()),
            present: Present::OwnerAlone,
            at_hand: None,
            now_ms,
        },
        tokens: shown.iter().map(|s| s.tokens).sum(),
        shown,
        dropped: vec![drop("local", "old"), drop("discord:dm:9", "far-old")],
        residue: vec![Residue {
            band: Band::PastOlder,
            pieces: 3,
            tokens: 900,
        }],
    }
}

#[test]
fn a_kept_view_is_read_back_by_its_compilation_and_by_its_turn_in_the_order_built() {
    let (_d, store) = store();
    keep(&store, "t1", "c-b", &view(2_000)).unwrap();
    keep(&store, "t1", "c-a", &view(1_000)).unwrap();
    // A turn whose id starts with the other's is its own.
    keep(&store, "t10", "c-z", &view(500)).unwrap();
    let back = read(&store, "c-a").unwrap().expect("kept");
    assert_eq!(back, ViewRecord::new("c-a", view(1_000)));
    let order: Vec<_> = of_turn(&store, "t1")
        .unwrap()
        .into_iter()
        .map(|v| v.compilation)
        .collect();
    assert_eq!(order, ["c-a", "c-b"], "built first, listed first");
    assert_eq!(of_turn(&store, "t10").unwrap().len(), 1);
    assert!(of_turn(&store, "t2").unwrap().is_empty());
    assert!(read(&store, "none").unwrap().is_none());
}

#[test]
fn the_tally_counts_each_piece_once_a_slot_and_once_under_each_reason() {
    let t = Tally::of(&ViewRecord::new("c", view(1)));
    let here = t.slots[&Slot::Here];
    assert_eq!((here.pieces, here.tokens), (2, 200));
    assert_eq!(t.reasons[&Reason::Window].pieces, 1);
    assert_eq!(t.reasons[&Reason::Named].pieces, 1);
    assert_eq!(t.reasons[&Reason::Floor].pieces, 1);
    assert_eq!(
        t.dropped["over_budget"],
        Count {
            pieces: 2,
            tokens: 140
        }
    );
    assert_eq!(t.dropped_reasons[&Reason::Past].pieces, 2);
    assert_eq!(t.present, Present::OwnerAlone);
    // Ids and counts only: no text and no person in the row.
    let row = serde_json::to_string(&t).unwrap();
    assert!(!row.contains("owner"), "{row}");
}

#[test]
fn by_default_a_reader_is_shown_ids_kinds_forms_tokens_and_reasons_and_no_text() {
    let v = Reader::default().show(&ViewRecord::new("c", view(1)));
    assert_eq!(v.pov.present, "owner_alone");
    assert!(v.pov.person, "a person was named");
    let slots: Vec<_> = v.slots.iter().map(|s| s.slot.as_str()).collect();
    assert_eq!(slots, ["frame", "elsewhere", "here"], "in the slots' order");
    let here = &v.slots[2];
    assert_eq!(here.tokens, 200);
    let t1 = &here.pieces[1];
    assert_eq!((t1.kind.as_str(), t1.first.as_str()), ("exchanges", "t1"));
    assert_eq!(t1.reasons, ["window", "named"]);
    assert_eq!((t1.form.as_str(), t1.label.as_str()), ("whole", "here"));
    assert!(v
        .slots
        .iter()
        .flat_map(|s| &s.pieces)
        .all(|p| p.text.is_none()));
    assert_eq!(v.dropped.len(), 2);
    assert_eq!(v.dropped[0].why, "over_budget");
    assert_eq!(v.residue.len(), 1);
    assert_eq!((v.totals.shown, v.totals.held_out), (4, 0));
    let said = lines(&v);
    assert!(said.contains("x:t1 exchanges t1 [window_words] whole 100 tokens, here: window, named"));
    assert!(!said.contains("owner\n") && !said.contains(" | "), "{said}");
}

#[test]
fn a_reader_in_company_is_shown_that_places_pieces_and_a_count_of_the_rest() {
    let v = Reader {
        place: Some("local"),
        text: None,
    }
    .show(&ViewRecord::new("c", view(1)));
    let ids: Vec<_> = v
        .slots
        .iter()
        .flat_map(|s| &s.pieces)
        .map(|p| p.first.as_str())
        .collect();
    assert_eq!(
        ids,
        ["frame", "t2", "t1"],
        "code's own text, and the place's"
    );
    let drops: Vec<_> = v.dropped.iter().map(|d| d.handle.as_str()).collect();
    assert_eq!(drops, ["x:old"]);
    assert!(v.residue.is_empty(), "a residue names no place");
    assert_eq!(
        v.totals.held_out, 2,
        "one piece and one drop of another place"
    );
    let said = lines(&v);
    assert!(!said.contains("far") && !said.contains("discord"), "{said}");
}

struct Fixed;

impl PieceText for Fixed {
    fn text(&self, piece: &Piece) -> Option<String> {
        (piece.range.first.id == "t2").then(|| "his words".to_string())
    }
}

#[test]
fn text_comes_only_from_the_reader_given_and_a_source_without_text_says_none() {
    let v = Reader {
        place: None,
        text: Some(&Fixed),
    }
    .show(&ViewRecord::new("c", view(1)));
    let texts: Vec<_> = v
        .slots
        .iter()
        .flat_map(|s| &s.pieces)
        .map(|p| p.text.as_deref())
        .collect();
    assert_eq!(texts, [None, None, Some("his words"), None]);
    assert!(lines(&v).contains("      | his words"));
    // The store's reader: an exchange with no record, and code's own text, give none.
    let (_d, store) = store();
    let p = piece("local", "t9", Band::Floor, &[Reason::Floor]);
    assert_eq!(StoreText(&store).text(&p), None);
}
