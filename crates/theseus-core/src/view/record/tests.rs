//! The view record kept and read back, and what each reader is shown of it.

use std::collections::BTreeSet;

use theseus_protocol::view::lines;

use super::show::{PieceText, Reader, Source, RESTRICTED};
use super::*;
use crate::view::budget::{DropWhy, Dropped, Residue, Shown, View};
use crate::view::select::Band;
use crate::view::{
    Anchor, AtHand, Audience, Author, Form, Handle, Marks, Piece, PointOfView, Pos, Present,
    Reason, Sizes, Slot, SourceRange, Space,
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
    keep(&store, "t1", "c-a", &view(1_000)).unwrap();
    keep(&store, "t1", "c-b", &view(2_000)).unwrap();
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
    assert_eq!(newest(&store, "t1").unwrap().unwrap().compilation, "c-b");
    assert_eq!(of_turn(&store, "t10").unwrap().len(), 1);
    assert!(of_turn(&store, "t2").unwrap().is_empty());
    assert!(newest(&store, "t2").unwrap().is_none());
    assert!(read(&store, "none").unwrap().is_none());
    // The counts are a record of their own, beside the view: no ledger row.
    assert_eq!(tally(&store, "c-b").unwrap().unwrap().compilation, "c-b");
}

#[test]
fn an_id_holding_the_separator_is_refused_and_nothing_is_kept() {
    let (_d, store) = store();
    assert!(keep(&store, "t1\u{1}x", "c", &view(1)).is_err());
    assert!(keep(&store, "t1", "c\u{1}x", &view(1)).is_err());
    assert!(of_turn(&store, "t1").unwrap().is_empty());
    assert!(read(&store, "c").unwrap().is_none() && tally(&store, "c").unwrap().is_none());
}

#[test]
fn within_one_millisecond_the_order_is_by_compilation_id() {
    let (_d, store) = store();
    keep(&store, "t1", "c-z", &view(7)).unwrap();
    keep(&store, "t1", "c-a", &view(7)).unwrap();
    let order: Vec<_> = of_turn(&store, "t1")
        .unwrap()
        .into_iter()
        .map(|v| v.compilation)
        .collect();
    // Newest first, the smaller id first: listed in the order built, the larger first.
    assert_eq!(order, ["c-z", "c-a"]);
    assert_eq!(newest(&store, "t1").unwrap().unwrap().compilation, "c-a");
}

#[test]
fn a_turn_with_a_hundred_compilations_reads_its_newest_and_a_bounded_few() {
    let (_d, store) = store();
    for i in 0..100u64 {
        keep(&store, "t1", &format!("c{i:03}"), &view(1_000 + i)).unwrap();
    }
    assert_eq!(newest(&store, "t1").unwrap().unwrap().compilation, "c099");
    let all = of_turn(&store, "t1").unwrap();
    assert_eq!(all.len(), VIEWS_READ);
    assert_eq!(all.last().unwrap().compilation, "c099");
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
    // Ids and counts only: no person and no handle in it.
    let row = serde_json::to_string(&t).unwrap();
    assert!(!row.contains("person") && !row.contains("x:"), "{row}");
}

/// Every source may be shown.
struct All;

impl Source for All {
    fn may_show(&self, _: &SourceRange, _: Option<&Marks>) -> bool {
        true
    }
    fn at_hand(&self, _: &AtHand) -> bool {
        true
    }
}

/// The sources of `local` alone (and code's own text); no at-hand reference.
struct LocalOnly;

impl Source for LocalOnly {
    fn may_show(&self, range: &SourceRange, _: Option<&Marks>) -> bool {
        match &range.space {
            Space::Code { .. } => true,
            Space::Exchanges { place } => place == "local",
            _ => false,
        }
    }
    fn at_hand(&self, _: &AtHand) -> bool {
        false
    }
}

fn owner(source: &dyn Source) -> Reader<'_> {
    Reader {
        source,
        text: None,
        restricted: false,
    }
}

#[test]
fn by_default_a_reader_is_shown_ids_kinds_forms_tokens_and_reasons_and_no_text() {
    let v = owner(&All).show(&ViewRecord::new("c", view(1)));
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
    assert_eq!(v.totals.shown, 4);
    assert!(v.restricted.is_none());
    let said = lines(&v);
    assert!(said.contains("x:t1 exchanges t1 [window_words] whole 100 tokens, here: window, named"));
    assert!(!said.contains(" | "), "{said}");
}

/// A record whose only difference from `view(1)` is in pieces `LocalOnly` may not see: another place's
/// pieces, shown and dropped, more of them and of other sizes; and an at-hand reference.
fn with_more_hidden() -> ViewRecord {
    let mut v = view(1);
    for (id, tokens) in [("far2", 900u64), ("far3", 5)] {
        let mut p = piece(
            "discord:dm:7",
            id,
            Band::ElsewhereWords,
            &[Reason::Elsewhere],
        );
        p.sizes.whole = tokens;
        v.shown.push(shown(p));
    }
    v.dropped.push(Dropped {
        range: range(exchanges("discord:dm:7"), "far4"),
        slot: Slot::Elsewhere,
        band: Band::ElsewhereWords,
        reasons: BTreeSet::from([Reason::Elsewhere]),
        handle: Handle("x:far4".into()),
        tokens: 333,
        why: DropWhy::OverBudget,
    });
    v.residue.push(Residue {
        band: Band::ElsewhereWords,
        pieces: 9,
        tokens: 9_000,
    });
    v.pov.at_hand = Some(AtHand::LineOfWork("kelp".into()));
    let mut r = ViewRecord::new("c", v);
    r.tokens += 1_238;
    r
}

#[test]
fn a_restricted_reader_reads_the_same_bytes_whatever_it_may_not_see() {
    let reader = Reader {
        source: &LocalOnly,
        text: None,
        restricted: true,
    };
    let fewer = reader.show(&ViewRecord::new("c", view(1)));
    let more = reader.show(&with_more_hidden());
    assert_eq!(lines(&fewer), lines(&more), "nothing of the hidden is told");
    assert_eq!(
        serde_json::to_string(&fewer).unwrap(),
        serde_json::to_string(&more).unwrap()
    );
    let ids: Vec<_> = fewer
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
    assert!(fewer.residue.is_empty() && fewer.pov.at_hand.is_none());
    assert_eq!(fewer.restricted.as_deref(), Some(RESTRICTED));
    let said = lines(&fewer);
    assert!(
        !said.contains("far") && !said.contains("discord") && !said.contains("kelp"),
        "{said}"
    );
}

#[test]
fn a_source_that_may_not_be_shown_today_leaves_its_piece_out_for_the_owner_too() {
    let v = owner(&LocalOnly).show(&with_more_hidden());
    let said = lines(&v);
    assert!(!said.contains("far") && !said.contains("kelp"), "{said}");
    assert!(v.restricted.is_none());
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
        source: &All,
        text: Some(&Fixed),
        restricted: false,
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
