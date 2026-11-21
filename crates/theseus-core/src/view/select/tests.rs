//! The selectors over a fake store, and the budgeter's properties over generated pieces: the order the
//! selectors run in does not change the result; nothing the gate denies changes it, not even a count; the
//! floor is always present; the result is deterministic.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::super::budget::{Budget, View};
use super::super::fold::{FoldNode, LadderRead};
use super::super::gate::{Admitted, Gate};
use super::super::timeline::{Cursor, ExchangeRef, Timeline, TimelineRoot};
use super::super::*;
use super::sources::*;
use super::*;

const H: u64 = 3_600_000;
const NOW: u64 = 10_000 * H;

/// One exchange of the fake store.
#[derive(Clone)]
struct Ex {
    turn: String,
    place: String,
    at: u64,
    person: Option<String>,
    audience: Audience,
    kind: Kind,
    tools: u64,
}

fn ex(turn: &str, place: &str, hours_ago: u64) -> Ex {
    Ex {
        turn: turn.into(),
        place: place.into(),
        at: NOW - hours_ago * H,
        person: Some("owner".into()),
        audience: Audience::Private,
        kind: Kind::Ask,
        tools: 0,
    }
}

#[derive(Default)]
struct Store {
    ex: Vec<Ex>,
    marks: Vec<(String, Pos)>,
    nodes: Vec<FoldNode>,
    named: Vec<(String, String)>,
}

impl Timeline for Store {
    fn newest(
        &self,
        root: &TimelineRoot,
        before: Option<&Cursor>,
        n: usize,
    ) -> anyhow::Result<Vec<ExchangeRef>> {
        let mut out: Vec<ExchangeRef> = self
            .ex
            .iter()
            .filter(|e| match root {
                TimelineRoot::Place(p) => e.place == *p,
                TimelineRoot::Person(p) => e.person.as_deref() == Some(p),
                TimelineRoot::Thing(_) => false,
            })
            .map(|e| ExchangeRef {
                turn_id: e.turn.clone(),
                session_id: "s".into(),
                at_ms: e.at,
            })
            .filter(|e| before.is_none_or(|c| e.cursor() < *c))
            .collect();
        out.sort_by_key(|e| std::cmp::Reverse(e.cursor()));
        out.truncate(n);
        Ok(out)
    }
}

impl Snapshot for Store {
    fn now_ms(&self) -> u64 {
        NOW
    }
    fn timeline(&self) -> &dyn Timeline {
        self
    }
    fn at_hand(&self, _: &Anchor) -> Option<AtHand> {
        None
    }
}

impl Exchanges for Store {
    fn facts(&self, turn: &str) -> anyhow::Result<Option<ExchangeFacts>> {
        Ok(self
            .ex
            .iter()
            .find(|e| e.turn == turn)
            .map(|e| ExchangeFacts {
                place: e.place.clone(),
                marks: Marks {
                    place: Some(e.place.clone()),
                    audience: e.audience,
                    author: Author::His {
                        may_authorise: true,
                    },
                    outside: false,
                    sealed: false,
                    veiled: false,
                },
                person: e.person.clone(),
                kind: e.kind,
                standing: false,
                waiting: false,
                his: 10,
                reply: 2_000,
                tools: e.tools,
                stubs: 5,
                did: None,
                handle: Handle(format!("x:{}", e.turn)),
            }))
    }
}

impl super::super::fold::Ladder for Store {
    fn read(&self, place: &str, n: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let mut window = self.newest(&TimelineRoot::Place(place.into()), None, 2 * n)?;
        window.retain(|e| since.is_none_or(|m| (e.at_ms, &e.turn_id) >= (m.at_ms, &m.id)));
        window.reverse();
        Ok(LadderRead {
            nodes: self
                .nodes
                .iter()
                .filter(|n| n.place == place)
                .cloned()
                .collect(),
            window,
        })
    }
}

impl Folds for Store {
    fn sizes(&self, _: &FoldNode) -> anyhow::Result<Sizes> {
        Ok(Sizes {
            whole: 900,
            spoken: 600,
            line: 20,
        })
    }
}

impl Places for Store {
    fn new_mark(&self, place: &str) -> anyhow::Result<Option<Pos>> {
        Ok(self
            .marks
            .iter()
            .find(|(p, _)| p == place)
            .map(|(_, m)| m.clone()))
    }
    fn places_of(&self, person: &str) -> anyhow::Result<Vec<String>> {
        Ok(self
            .ex
            .iter()
            .filter(|e| e.person.as_deref() == Some(person))
            .map(|e| e.place.clone())
            .collect())
    }
}

fn card(thing: &str) -> Ref {
    Ref {
        range: SourceRange {
            space: Space::Card {
                thing: thing.into(),
            },
            first: Pos {
                at_ms: 0,
                id: thing.into(),
            },
            last: Pos {
                at_ms: 0,
                id: thing.into(),
            },
        },
        marks: Marks {
            place: Some("local".into()),
            audience: Audience::Private,
            author: Author::Theseus,
            outside: false,
            sealed: false,
            veiled: false,
        },
        sizes: Sizes {
            whole: 300,
            spoken: 100,
            line: 10,
        },
        handle: Handle(format!("card:{thing}")),
        form: Form::Whole,
        unit: None,
    }
}

impl Waiting for Store {
    fn waiting(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(vec![card("question")])
    }
}

impl Things for Store {
    fn named_in(&self, turn: &str) -> anyhow::Result<Vec<String>> {
        Ok(self
            .named
            .iter()
            .filter(|(t, _)| t == turn)
            .map(|(_, x)| x.clone())
            .collect())
    }
    fn card(&self, thing: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(vec![card(thing)])
    }
    fn at_hand(&self, _: &AtHand) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Names for Store {
    fn named(&self, message: &str) -> anyhow::Result<Vec<String>> {
        Ok(message
            .split_whitespace()
            .filter(|w| w.contains(':'))
            .map(str::to_owned)
            .collect())
    }
}

impl TaskCards for Store {
    fn joined(&self, _: &PointOfView, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

impl Recall for Store {
    fn recall(&self, _: &PointOfView, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(vec![card("recalled")])
    }
}

impl People for Store {
    fn about(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

fn sources<'a>(s: &'a Store, message: &'a str) -> Sources<'a> {
    Sources {
        exchanges: s,
        ladder: s,
        folds: s,
        places: s,
        waiting: s,
        things: s,
        names: s,
        tasks: s,
        recall: s,
        people: s,
        message,
    }
}

fn pov(present: Present) -> PointOfView {
    PointOfView {
        anchor: Anchor::Place("local".into()),
        person: Some("owner".into()),
        present,
        at_hand: None,
        now_ms: NOW,
    }
}

fn ids(pieces: &[Piece]) -> Vec<&str> {
    pieces.iter().map(|p| p.range.last.id.as_str()).collect()
}

fn select(s: &Store, band: Band, present: Present) -> Vec<Piece> {
    let table = table(sources(s, "see crate:x"), Vec::new(), Vec::new());
    let sel = table
        .iter()
        .find(|t| t.band() == band)
        .expect("a selector per band");
    let mut out = sel.select(&pov(present), s).expect("selects");
    out.sort_by_key(|p| std::cmp::Reverse(p.range.last.clone()));
    out
}

#[test]
fn there_is_one_selector_per_row_of_the_table_in_its_order() {
    let s = Store::default();
    let table = table(sources(&s, ""), Vec::new(), Vec::new());
    let bands: Vec<Band> = table.iter().map(|t| t.band()).collect();
    let rows: Vec<Band> = ADMISSION.iter().map(|r| r.band).collect();
    assert_eq!(bands, rows);
}

#[test]
fn the_window_lays_the_floor_spoken_the_rest_spoken_and_tool_results_whole() {
    let mut s = Store::default();
    for i in 0..5 {
        let mut e = ex(&format!("t{i}"), "local", 10 - i);
        e.tools = if i == 4 { 500 } else { 0 };
        s.ex.push(e);
    }
    s.ex.push(ex("before-new", "local", 20));
    s.marks.push((
        "local".into(),
        Pos {
            at_ms: NOW - 15 * H,
            id: String::new(),
        },
    ));
    assert_eq!(
        ids(&select(&s, Band::Floor, Present::OwnerAlone)),
        ["t4", "t3", "t2"]
    );
    assert_eq!(
        ids(&select(&s, Band::WindowWords, Present::OwnerAlone)),
        ["t1", "t0"]
    );
    let tools = select(&s, Band::WindowTools, Present::OwnerAlone);
    assert_eq!(ids(&tools), ["t4"]);
    assert_eq!(tools[0].form, Form::Whole);
    let floor = select(&s, Band::Floor, Present::Shared);
    assert_eq!(floor[0].form, Form::Spoken);
    assert_eq!(floor[0].sizes.spoken, 10 + 2_000 + 5);
    assert_eq!(
        floor[0].marks.author,
        Author::His {
            may_authorise: false
        },
        "a retrieved word of his never authorises"
    );
    assert!(
        select(&s, Band::Floor, Present::Unknown).is_empty(),
        "unknown admits nothing"
    );
}

#[test]
fn elsewhere_is_his_last_two_days_and_his_newest_in_five_places_only_where_he_is_alone() {
    let mut s = Store::default();
    s.ex.push(ex("here", "local", 1));
    s.ex.push(ex("a1", "a", 2));
    s.ex.push(ex("a2", "a", 30));
    s.ex.push(ex("a3", "a", 100));
    let mut shared = ex("sh", "chan", 3);
    shared.audience = Audience::Shared;
    s.ex.push(shared);
    let mut other = ex("other", "b", 4);
    other.person = Some("guest".into());
    s.ex.push(other);
    for (i, p) in ["p1", "p2", "p3", "p4", "p5", "p6"].iter().enumerate() {
        s.ex.push(ex(&format!("{p}x"), p, 200 + i as u64));
    }
    let words = select(&s, Band::ElsewhereWords, Present::OwnerAlone);
    assert_eq!(ids(&words), ["a1", "a2", "p1x", "p2x", "p3x", "p4x"]);
    assert!(words.iter().all(|p| p.form == Form::Line));
    let replies = select(&s, Band::ElsewhereReplies, Present::OwnerAlone);
    assert_eq!(ids(&replies), ["a1", "p1x", "p2x", "p3x", "p4x"]);
    assert_eq!(
        replies[0].sizes.spoken,
        10 + 1_500,
        "a long reply is its two ends"
    );
    assert!(select(&s, Band::ElsewhereWords, Present::Private).is_empty());
    assert!(select(&s, Band::ElsewhereWords, Present::Shared).is_empty());
}

#[test]
fn the_past_thins_with_age_and_a_fragment_is_never_bare() {
    let mut s = Store::default();
    s.ex.push(ex("lately", "a", 10));
    s.ex.push(ex("d3", "a", 72));
    s.ex.push(ex("here-d3", "local", 72));
    let mut frag = ex("d10", "a", 240);
    frag.kind = Kind::Fragment;
    s.ex.push(frag);
    s.ex.push(ex("d20", "a", 480));
    s.ex.push(ex("d60", "a", 1_440 - 1));
    let mut fix = ex("d61", "a", 1_464 - 1);
    fix.kind = Kind::Correction;
    s.ex.push(fix);
    let p7 = select(&s, Band::Past7, Present::OwnerAlone);
    assert_eq!(ids(&p7), ["d3"], "this place's own past is its ladder's");
    let did = super::make::UNSUMMARISED.len().div_ceil(3) as u64;
    assert_eq!(p7[0].sizes.line, 10 + did);
    let p30 = select(&s, Band::Past30, Present::OwnerAlone);
    assert_eq!(ids(&p30), ["d10", "d20"]);
    assert_eq!(
        p30[0].sizes.line,
        10 + did,
        "a fragment carries its did line"
    );
    assert_eq!(p30[1].sizes.line, 10, "his words alone");
    assert_eq!(
        ids(&select(&s, Band::PastOlder, Present::OwnerAlone)),
        ["d61"]
    );
    assert!(
        select(&s, Band::Past7, Present::Shared).is_empty(),
        "in company: this place only"
    );
}

#[test]
fn in_play_named_waiting_and_recall_ask_for_their_sources() {
    let mut s = Store::default();
    s.ex.push(ex("t1", "local", 1));
    s.named.push(("t1".into(), "issue:44".into()));
    let card_of =
        |pieces: Vec<Piece>| -> Vec<String> { pieces.into_iter().map(|p| p.handle.0).collect() };
    assert_eq!(
        card_of(select(&s, Band::InPlay, Present::Shared)),
        ["card:issue:44"]
    );
    assert_eq!(
        card_of(select(&s, Band::Named, Present::OwnerAlone)),
        ["card:crate:x"]
    );
    assert_eq!(
        card_of(select(&s, Band::Waiting, Present::OwnerAlone)),
        ["card:question"]
    );
    assert!(select(&s, Band::Waiting, Present::Private).is_empty());
    assert_eq!(
        card_of(select(&s, Band::Recall, Present::Private)),
        ["card:recalled"]
    );
    assert!(
        select(&s, Band::Reached, Present::OwnerAlone).is_empty(),
        "no search before the first word"
    );
}

#[test]
fn a_whole_turn_assembles_and_the_floor_is_in_view() {
    let mut s = Store::default();
    for i in 0..4 {
        s.ex.push(ex(&format!("t{i}"), "local", 10 - i));
    }
    s.ex.push(ex("a1", "a", 2));
    let frame = vec![Part {
        name: "system".into(),
        tokens: 13_000,
    }];
    let now = vec![Part {
        name: "message".into(),
        tokens: 50,
    }];
    let table = table(sources(&s, ""), frame, now);
    let p = pov(Present::OwnerAlone);
    let v = assemble(&p, &s, &table, &AllowPrivate, &Budget::new(200_000, 8_000))
        .expect("reads")
        .expect("fits");
    let bands: Vec<Band> = v.shown.iter().map(|x| x.piece.band).collect();
    assert_eq!(bands[..2], [Band::Frame, Band::Now]);
    assert_eq!(bands.iter().filter(|b| **b == Band::Floor).count(), 3);
    assert!(bands.contains(&Band::ElsewhereWords));
}

/// Admits what is private and not sealed.
struct AllowPrivate;

impl Gate for AllowPrivate {
    fn admit(&self, _: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        (piece.marks.audience == Audience::Private && !piece.marks.sealed)
            .then(|| Admitted::new(piece))
    }
}

/// A selector that returns what it was given.
struct Canned(Band, Vec<Piece>);

impl Selector for Canned {
    fn band(&self) -> Band {
        self.0
    }
    fn select(&self, _: &PointOfView, _: &dyn Snapshot) -> anyhow::Result<Vec<Piece>> {
        Ok(self.1.clone())
    }
}

fn gen_piece() -> impl Strategy<Value = Piece> {
    (
        0..ADMISSION.len(),
        0u64..12,
        0u8..3,
        0u8..3,
        (0u64..400, 0u64..200, 0u64..50),
        prop::option::of(0u8..3),
        any::<bool>(),
    )
        .prop_map(|(row, at, space, form, (w, s, l), unit, sealed)| {
            let band = ADMISSION[row].band;
            let pos = |at: u64| Pos {
                at_ms: at,
                id: format!("p{at}"),
            };
            let space = match space {
                0 => Space::Exchanges {
                    place: "local".into(),
                },
                1 => Space::Facts { thing: "x".into() },
                _ => Space::Folds {
                    place: "local".into(),
                    level: 1,
                },
            };
            Piece {
                range: SourceRange {
                    space,
                    first: pos(at),
                    last: pos(at + at % 2),
                },
                band,
                slot: band.row().slot,
                reasons: BTreeSet::from([Reason::Window]),
                form: [Form::Line, Form::Spoken, Form::Whole][form as usize],
                marks: Marks {
                    place: Some("local".into()),
                    audience: Audience::Private,
                    author: Author::Theseus,
                    outside: false,
                    sealed,
                    veiled: false,
                },
                sizes: Sizes {
                    whole: w + s + l,
                    spoken: s + l,
                    line: l,
                },
                handle: Handle(format!("h{at}")),
                unit: unit.map(|u| format!("u{u}")),
            }
        })
}

fn view_of(selectors: &[Box<dyn Selector>], budget: &Budget) -> Result<View, budget::Overflow> {
    let s = Store::default();
    assemble(
        &pov(Present::OwnerAlone),
        &s,
        selectors,
        &AllowPrivate,
        budget,
    )
    .expect("reads")
}

/// The pieces as one canned selector per band, in the given order of bands.
fn canned(pieces: &[Piece], reverse: bool) -> Vec<Box<dyn Selector>> {
    let mut bands: Vec<Band> = ADMISSION.iter().map(|r| r.band).collect();
    if reverse {
        bands.reverse();
    }
    bands
        .into_iter()
        .map(|b| {
            let mut mine: Vec<Piece> = pieces.iter().filter(|p| p.band == b).cloned().collect();
            if reverse {
                mine.reverse();
            }
            Box::new(Canned(b, mine)) as Box<dyn Selector>
        })
        .collect()
}

proptest! {
    #[test]
    fn the_order_of_selectors_and_what_the_gate_denies_do_not_change_the_view(
        pieces in prop::collection::vec(gen_piece(), 0..40),
        ceiling in 0u64..2_000,
        here in 0u64..2_000,
        window in 0u64..5_000,
    ) {
        let budget = Budget { ceiling, here_allowance: here, provider_window: window, reserve: 0 };
        let a = view_of(&canned(&pieces, false), &budget);
        let b = view_of(&canned(&pieces, true), &budget);
        prop_assert_eq!(&a, &b, "the order selectors run in");
        let allowed: Vec<Piece> = pieces.iter().filter(|p| !p.marks.sealed).cloned().collect();
        let c = view_of(&canned(&allowed, false), &budget);
        prop_assert_eq!(&a, &c, "what the gate denies");
        prop_assert_eq!(&a, &view_of(&canned(&pieces, false), &budget), "deterministic");
        if let Ok(v) = &a {
            prop_assert!(v.tokens <= budget.total());
            for f in allowed.iter().filter(|p| p.band.row().floor) {
                prop_assert!(
                    v.shown.iter().any(|s| s.piece.range.space == f.range.space
                        && s.piece.range.first <= f.range.first
                        && f.range.last <= s.piece.range.last
                        && s.form >= f.form),
                    "the floor is always present, in at least the form it asked for"
                );
            }
        }
    }
}
