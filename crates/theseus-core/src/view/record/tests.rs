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
        slot: Slot::Called,
        band: Band::Recall,
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
            band: Band::WindowTools,
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

/// The owner's look at a record that differs only in a drop it may not see: the whole result is the same,
/// residue included (the held drop's count and size are taken out of its band's residue).
#[test]
fn the_owners_look_tells_nothing_of_a_held_drop_not_even_in_the_residue() {
    let with = |held: bool| {
        let mut v = view(1);
        v.residue.push(Residue {
            band: Band::ElsewhereWords,
            pieces: 1 + u32::from(held),
            tokens: 100 + if held { 333 } else { 0 },
        });
        if held {
            v.dropped.push(Dropped {
                range: range(exchanges("discord:dm:7"), "far4"),
                slot: Slot::Elsewhere,
                band: Band::ElsewhereWords,
                reasons: BTreeSet::from([Reason::Elsewhere]),
                handle: Handle("x:far4".into()),
                tokens: 333,
                why: DropWhy::OverBudget,
            });
        }
        owner(&LocalOnly).show(&ViewRecord::new("c", v))
    };
    let (plain, held) = (with(false), with(true));
    assert_eq!(
        serde_json::to_string(&plain).unwrap(),
        serde_json::to_string(&held).unwrap()
    );
    assert!(plain
        .residue
        .iter()
        .any(|r| r.pieces == 1 && r.tokens == 100));
}

/// Two chains of one turn from one frozen view (the same assembly time, as a room's new chain reuses it):
/// the newest is the later chain's, whatever the ids, and `of_turn` lists them in the order kept.
#[test]
fn two_chains_of_one_frozen_view_read_newest_as_the_later() {
    let (_d, store) = store();
    // Ids that sort in the order made, as production ids (`cmp_<UUIDv7>`) do.
    let (first, second) = ("cmp_a".to_string(), "cmp_b".to_string());
    keep(&store, "t1", &first, &view(7)).unwrap();
    keep(&store, "t1", &second, &view(7)).unwrap();
    assert_eq!(newest(&store, "t1").unwrap().unwrap().compilation, second);
    let order: Vec<_> = (of_turn(&store, "t1").unwrap().into_iter())
        .map(|v| v.compilation)
        .collect();
    assert_eq!(order, [first, second], "listed in the order kept");
}

/// A turn's view record and its counts as `keep` wrote them before slot E (his words by age) was removed on
/// 2026-11-24: its bytes are this file's rig (`view`, with one more piece shown, of band `past7`) as the
/// writer of the commit before kept them, read back from the store. It names the bands `past7`, `past30` and
/// `past_older`, the slot `past` and the reason `past`.
const OLD_VIEW: &str = concat!(
    r#"{"compilation":"c-old","assembly":"view","pov":{"anchor":{"place":"local"},"person":"owner","pre"#,
    r#"sent":"owner_alone","at_hand":null,"now_ms":1000},"shown":[{"piece":{"range":{"space":{"code":{""#,
    r#"part":"frame"}},"first":{"at_ms":1,"id":"frame"},"last":{"at_ms":1,"id":"frame"}},"band":"frame""#,
    r#","slot":"frame","reasons":["fixed"],"form":"whole","marks":{"place":null,"audience":"private","a"#,
    r#"uthor":{"his":{"may_authorise":true}},"outside":false,"sealed":false,"veiled":false},"sizes":{"w"#,
    r#"hole":100,"spoken":40,"line":10},"handle":"x:frame","unit":null},"form":"whole","tokens":100},{""#,
    r#"piece":{"range":{"space":{"exchanges":{"place":"local"}},"first":{"at_ms":1,"id":"t2"},"last":{""#,
    r#"at_ms":1,"id":"t2"}},"band":"floor","slot":"here","reasons":["floor"],"form":"whole","marks":{"p"#,
    r#"lace":"local","audience":"private","author":{"his":{"may_authorise":true}},"outside":false,"seal"#,
    r#"ed":false,"veiled":false},"sizes":{"whole":100,"spoken":40,"line":10},"handle":"x:t2","unit":nul"#,
    r#"l},"form":"whole","tokens":100},{"piece":{"range":{"space":{"exchanges":{"place":"local"}},"firs"#,
    r#"t":{"at_ms":1,"id":"t1"},"last":{"at_ms":1,"id":"t1"}},"band":"window_words","slot":"here","reas"#,
    r#"ons":["window","named"],"form":"whole","marks":{"place":"local","audience":"private","author":{""#,
    r#"his":{"may_authorise":true}},"outside":false,"sealed":false,"veiled":false},"sizes":{"whole":100"#,
    r#","spoken":40,"line":10},"handle":"x:t1","unit":null},"form":"whole","tokens":100},{"piece":{"ran"#,
    r#"ge":{"space":{"exchanges":{"place":"discord:dm:9"}},"first":{"at_ms":1,"id":"far"},"last":{"at_m"#,
    r#"s":1,"id":"far"}},"band":"elsewhere_words","slot":"elsewhere","reasons":["elsewhere"],"form":"wh"#,
    r#"ole","marks":{"place":"discord:dm:9","audience":"private","author":{"his":{"may_authorise":true}"#,
    r#"},"outside":false,"sealed":false,"veiled":false},"sizes":{"whole":100,"spoken":40,"line":10},"ha"#,
    r#"ndle":"x:far","unit":null},"form":"whole","tokens":100},{"piece":{"range":{"space":{"exchanges":"#,
    r#"{"place":"discord:dm:9"}},"first":{"at_ms":1,"id":"his-d3"},"last":{"at_ms":1,"id":"his-d3"}},"b"#,
    r#"and":"past7","slot":"past","reasons":["past"],"form":"whole","marks":{"place":"discord:dm:9","au"#,
    r#"dience":"private","author":{"his":{"may_authorise":true}},"outside":false,"sealed":false,"veiled"#,
    r#"":false},"sizes":{"whole":100,"spoken":40,"line":10},"handle":"x:his-d3","unit":null},"form":"wh"#,
    r#"ole","tokens":100}],"dropped":[{"range":{"space":{"exchanges":{"place":"local"}},"first":{"at_ms"#,
    r#"":1,"id":"old"},"last":{"at_ms":1,"id":"old"}},"slot":"past","band":"past30","reasons":["past"],"#,
    r#""handle":"x:old","tokens":70,"why":"over_budget"},{"range":{"space":{"exchanges":{"place":"disco"#,
    r#"rd:dm:9"}},"first":{"at_ms":1,"id":"far-old"},"last":{"at_ms":1,"id":"far-old"}},"slot":"past",""#,
    r#"band":"past30","reasons":["past"],"handle":"x:far-old","tokens":70,"why":"over_budget"}],"residu"#,
    r#"e":[{"band":"past_older","pieces":3,"tokens":900}],"tokens":400}"#,
);
/// Its counts, as kept beside it (`view_tally.c-old`).
const OLD_TALLY: &str = concat!(
    r#"{"compilation":"c-old","present":"owner_alone","slots":{"frame":{"pieces":1,"tokens":100},"past""#,
    r#":{"pieces":1,"tokens":100},"elsewhere":{"pieces":1,"tokens":100},"here":{"pieces":2,"tokens":200"#,
    r#"}},"reasons":{"floor":{"pieces":1,"tokens":100},"window":{"pieces":1,"tokens":100},"elsewhere":{"#,
    r#""pieces":1,"tokens":100},"past":{"pieces":1,"tokens":100},"named":{"pieces":1,"tokens":100},"fix"#,
    r#"ed":{"pieces":1,"tokens":100}},"dropped":{"over_budget":{"pieces":2,"tokens":140}},"dropped_reas"#,
    r#"ons":{"past":{"pieces":2,"tokens":140}},"tokens":400}"#,
);

/// A record that names a retired band still decodes, as it was written: `view show` lists each piece of a band
/// or slot no longer laid as `retired`, everything else as before, and its counts read too. Nothing is
/// rewritten.
#[test]
fn an_old_view_record_naming_a_retired_band_still_reads() {
    use theseus_store::{kinds, NewRecord};
    let (_d, store) = store();
    store
        .append(&[
            NewRecord::bytes(
                kinds::META,
                Some("view.c-old"),
                OLD_VIEW.as_bytes().to_vec(),
            ),
            NewRecord::bytes(
                kinds::META,
                Some("view_tally.c-old"),
                OLD_TALLY.as_bytes().to_vec(),
            ),
        ])
        .unwrap();
    let rec = read(&store, "c-old")
        .unwrap()
        .expect("the old record decodes");
    assert_eq!(rec.shown.len(), 5);
    let v = owner(&All).show(&rec);
    let slots: Vec<_> = v.slots.iter().map(|s| s.slot.as_str()).collect();
    assert_eq!(
        slots,
        ["frame", "elsewhere", "here", "retired"],
        "the retired slot's piece is listed, after the slots laid"
    );
    let retired = &v.slots[3].pieces[0];
    assert_eq!(
        (retired.first.as_str(), retired.band.as_str()),
        ("his-d3", "retired")
    );
    assert_eq!(
        retired.reasons,
        ["past"],
        "a reason still laid keeps its name"
    );
    assert!(v
        .dropped
        .iter()
        .all(|d| (d.slot.as_str(), d.band.as_str()) == ("retired", "retired")));
    assert_eq!(v.dropped.len(), 2);
    assert_eq!(
        v.residue
            .iter()
            .map(|r| (r.band.as_str(), r.pieces))
            .collect::<Vec<_>>(),
        [("retired", 3)]
    );
    let said = lines(&v);
    assert!(said.contains("x:t1 exchanges t1 [window_words] whole 100 tokens, here: window, named"));
    assert!(said.contains("[retired]"), "{said}");
    let t = tally(&store, "c-old").unwrap().expect("its counts decode");
    let by_name = |m: &std::collections::BTreeMap<Slot, Count>| -> Vec<(String, u64)> {
        m.iter().map(|(k, c)| (show::name(k), c.pieces)).collect()
    };
    assert!(
        by_name(&t.slots).contains(&("retired".to_string(), 1)),
        "{:?}",
        t.slots
    );
    assert_eq!(t.tokens, 400);
    assert_eq!(
        t.dropped_reasons
            .iter()
            .map(|(k, c)| (show::name(k), c.pieces))
            .collect::<Vec<_>>(),
        [("past".to_string(), 2)]
    );
}

/// A band or a slot this build does not know, a newer build's or a misspelling, is refused, as a record, as a
/// tally's key and alone: only the names slot E's bands and slot E itself were stored under read as retired.
#[test]
fn a_band_or_slot_this_build_does_not_know_is_refused() {
    use std::collections::BTreeMap;
    for band in ["past90", "pasts7", "a_band_from_a_newer_build"] {
        let got = serde_json::from_str::<crate::view::select::Band>(&format!("\"{band}\""));
        assert!(got.is_err(), "{band}: {got:?}");
    }
    for slot in ["pasts", "e", "a_slot_from_a_newer_build"] {
        let got = serde_json::from_str::<crate::view::Slot>(&format!("\"{slot}\""));
        assert!(got.is_err(), "{slot}: {got:?}");
        let key =
            serde_json::from_str::<BTreeMap<crate::view::Slot, u32>>(&format!("{{\"{slot}\": 1}}"));
        assert!(key.is_err(), "{slot} as a tally's key: {key:?}");
    }
}
