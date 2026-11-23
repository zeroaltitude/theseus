//! The selectors over a fake store, and the properties through the real selectors over generated stores:
//! the order the selectors run in does not change the result; records the gate denies change nothing, not a
//! piece, not a count, not a residue; the floor is always present; the result is deterministic.

use std::collections::BTreeSet;

use proptest::prelude::*;

use super::super::budget::{self, Budget, View};
use super::super::fold::{FoldNode, LadderRead, Writer};
use super::super::gate::{Admitted, Gate};
use super::super::timeline::{Cursor, ExchangeRef, Timeline, TimelineRoot};
use super::super::*;
use super::sources::*;
use super::*;

const H: u64 = 3_600_000;
const NOW: u64 = 10_000 * H;

/// One exchange of the fake store.
#[derive(Debug, Clone)]
struct Ex {
    turn: String,
    place: String,
    at: u64,
    person: Option<String>,
    audience: Audience,
    kind: Kind,
    tools: u64,
    sealed: bool,
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
        sealed: false,
    }
}

#[derive(Default)]
struct Store {
    ex: Vec<Ex>,
    marks: Vec<(String, Pos)>,
    nodes: Vec<FoldNode>,
    named: Vec<(String, String)>,
    /// Recall returns a stored reference that claims code's own space.
    code_recall: bool,
    /// Cards carry his authorising mark (a fact first said in a marked message).
    his_cards: bool,
    /// Cards by thing, overriding the default one.
    cards: Vec<(String, Ref)>,
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
            .filter(|e| {
                before.is_none_or(|c| {
                    if c.turn_id.is_empty() {
                        e.at_ms < c.at_ms
                    } else {
                        e.cursor() < *c
                    }
                })
            })
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
                    sealed: e.sealed,
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
    /// A place with no fold yet: everything since the mark is the window (the count of N to 2N is the
    /// ladder's own, and not the selectors' to test).
    fn read(&self, place: &str, _: usize, since: Option<&Pos>) -> anyhow::Result<LadderRead> {
        let mut window = self.newest(&TimelineRoot::Place(place.into()), None, usize::MAX)?;
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
    card_by(thing, Author::Theseus)
}

fn card_by(thing: &str, author: Author) -> Ref {
    Ref {
        range: SourceRange {
            space: Space::Card {
                thing: thing.into(),
                from: None,
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
            author,
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
        let set: Vec<Ref> = self
            .cards
            .iter()
            .filter(|(t, _)| t == thing)
            .map(|(_, r)| r.clone())
            .collect();
        if !set.is_empty() {
            return Ok(set);
        }
        let author = if self.his_cards {
            Author::His {
                may_authorise: true,
            }
        } else {
            Author::Theseus
        };
        Ok(vec![card_by(thing, author)])
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
        let mut r = card("recalled");
        if self.code_recall {
            r.range.space = Space::Code {
                part: "system".into(),
            };
            r.marks.sealed = true;
        }
        Ok(vec![r])
    }
}

impl People for Store {
    fn about(&self, _: &str) -> anyhow::Result<Vec<Ref>> {
        Ok(Vec::new())
    }
}

fn sources<'a>(s: &'a Store, message: &'a str) -> Sources<'a> {
    sources_with(s, message, &AllowPrivate)
}

fn sources_with<'a>(s: &'a Store, message: &'a str, gate: &'a dyn Gate) -> Sources<'a> {
    Sources {
        gate,
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
    assert_eq!(
        floor.iter().map(|p| p.form).collect::<Vec<_>>(),
        [Form::Spoken, Form::Spoken, Form::Spoken],
        "the floor is the newest exchanges' words; their tool results are the window's"
    );
    assert_eq!(floor[1].sizes.spoken, 10 + 2_000 + 5);
    assert_eq!(
        floor[0].marks.author,
        Author::His {
            may_authorise: false
        },
        "a retrieved word of his never authorises"
    );
}

#[test]
fn elsewhere_is_his_last_two_days_and_his_newest_in_five_places() {
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
    let pov = pov(Present::OwnerAlone);
    let gate = permit::Permit::of(crate::view_gate::Verified::of_fresh(
        theseus_memory::recall::Place::Private,
        "local",
    ));
    let select = |band| {
        elsewhere::Elsewhere::new(band, sources_with(&s, "", &gate))
            .select(&pov, &s)
            .unwrap()
    };
    let words = select(Band::ElsewhereWords);
    assert_eq!(ids(&words), ["a1", "sh", "a2", "p1x", "p2x", "p3x"]);
    assert!(words.iter().all(|p| p.form == Form::Line));
    let replies = select(Band::ElsewhereReplies);
    assert_eq!(ids(&replies), ["a1", "sh", "p1x", "p2x", "p3x"]);
    assert_eq!(
        replies[0].sizes.spoken,
        10 + 1_500,
        "a long reply is its two ends"
    );
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
            .then(|| Admitted::for_tests(piece))
    }
}

/// Denies everything.
struct DenyAll;

impl Gate for DenyAll {
    fn admit(&self, _: &PointOfView, _: Piece) -> Option<Admitted<Piece>> {
        None
    }
}

fn parts() -> (Vec<Part>, Vec<Part>) {
    (
        vec![Part {
            name: "system".into(),
            tokens: 13_000,
        }],
        vec![Part {
            name: "message".into(),
            tokens: 50,
        }],
    )
}

fn assembled(s: &Store, gate: &dyn Gate, message: &str) -> Assembled {
    let (frame, now) = parts();
    let table = table(sources_with(s, message, gate), frame, now);
    assemble_counted(
        &pov(Present::OwnerAlone),
        s,
        &table,
        gate,
        &Budget::new(1_000_000, 8_000),
    )
    .expect("reads")
}

#[test]
fn only_slots_a_and_i_lay_code_text_and_a_source_that_claims_it_is_a_fault() {
    let mut s = Store::default();
    s.ex.push(ex("t1", "local", 1));
    s.code_recall = true;
    for gate in [&DenyAll as &dyn Gate, &AllowPrivate] {
        let a = assembled(&s, gate, "see crate:x");
        assert_eq!(a.faults, 1, "recall's code-claiming reference is a fault");
        let v = a.view.expect("fits");
        assert!(v.shown.iter().all(|x| x.piece.handle.0 != "card:recalled"));
        assert!(v.dropped.iter().all(|d| d.handle.0 != "card:recalled"));
    }
    let v = assembled(&s, &DenyAll, "see crate:x").view.expect("fits");
    let bands: Vec<Band> = v.shown.iter().map(|x| x.piece.band).collect();
    assert_eq!(
        bands,
        [Band::Frame, Band::Now],
        "a gate that denies all shows nothing from a source"
    );
    assert!(v.dropped.is_empty() && v.residue.is_empty());
}

#[test]
fn no_retrieved_piece_may_authorise_whatever_made_it() {
    let mut s = Store::default();
    s.ex.push(ex("t1", "local", 1));
    s.his_cards = true;
    let his = Marks {
        place: Some("local".into()),
        audience: Audience::Private,
        author: Author::His {
            may_authorise: true,
        },
        outside: false,
        sealed: false,
        veiled: false,
    };
    let at = |h: u64| Pos {
        at_ms: NOW - h * H,
        id: format!("n{h}"),
    };
    s.nodes.push(FoldNode {
        level: 1,
        place: "local".into(),
        first: at(900),
        last: at(800),
        exchanges: 20,
        his_words: Vec::new(),
        his_counted: 0,
        title: String::new(),
        account: String::new(),
        marks: his,
        handle: Handle("fold:1".into()),
        writer: Writer {
            by: "model".into(),
            version: "1".into(),
        },
    });
    let v = assembled(&s, &AllowPrivate, "see crate:x")
        .view
        .expect("fits");
    let handles: Vec<&str> = v.shown.iter().map(|x| x.piece.handle.0.as_str()).collect();
    assert!(
        handles.contains(&"fold:1")
            && handles.contains(&"card:crate:x")
            && handles.contains(&"x:t1")
    );
    for x in &v.shown {
        assert_ne!(
            x.piece.marks.author,
            Author::His {
                may_authorise: true
            },
            "{}",
            x.piece.handle.0
        );
    }
}

#[test]
fn a_unit_with_a_member_the_gate_denies_is_left_out_whole() {
    let mut s = Store::default();
    let mut stale = card("stale");
    stale.unit = Some("u".into());
    let mut fix = card("fix");
    fix.unit = Some("u".into());
    fix.marks.sealed = true;
    s.cards.push(("crate:x".into(), stale));
    s.cards.push(("crate:x".into(), fix.clone()));
    let v = assembled(&s, &AllowPrivate, "see crate:x")
        .view
        .expect("fits");
    assert!(
        v.shown.iter().all(|x| x.piece.band != Band::Named),
        "no stale fact without its correction"
    );
    assert!(
        v.dropped.is_empty() && v.residue.is_empty(),
        "and nothing says it was there"
    );
    fix.marks.sealed = false;
    s.cards[1].1 = fix;
    let v = assembled(&s, &AllowPrivate, "see crate:x")
        .view
        .expect("fits");
    assert_eq!(
        v.shown
            .iter()
            .filter(|x| x.piece.band == Band::Named)
            .count(),
        2
    );
}

#[test]
fn the_newest_exchange_keeps_its_words_and_its_tool_results_go_when_there_is_no_room() {
    let mut s = Store::default();
    for i in 0..3 {
        s.ex.push(ex(&format!("t{i}"), "local", 10 - i));
    }
    s.ex[2].tools = 160_000;
    let (frame, now) = parts();
    let table = table(sources(&s, ""), frame, now);
    let small = Budget::new(100_000, 8_000);
    let r = assemble(&pov(Present::OwnerAlone), &s, &table, &AllowPrivate, &small).expect("reads");
    let v = r.expect("the floor's words fit: no tool result of a previous exchange is mandatory");
    let newest = v
        .shown
        .iter()
        .find(|x| x.piece.handle.0 == "x:t2")
        .expect("its words are the floor");
    assert_eq!(newest.form, Form::Spoken);
    assert!(
        !(v.shown.iter()).any(|x| x.piece.handle.0 == "x:t2" && x.form == Form::Whole),
        "its tool results are not laid whole when there is no room"
    );
}

#[test]
fn his_newest_word_in_a_place_is_found_past_many_other_exchanges() {
    let mut s = Store::default();
    s.ex.push(ex("a-his", "a", 200));
    for i in 0..25 {
        let mut wake = ex(&format!("a-wake{i}"), "a", 100 - i);
        wake.person = None;
        s.ex.push(wake);
    }
    for i in 0..150 {
        s.ex.push(ex(&format!("old{i}"), "local", 50 + i));
    }
    let words = select(&s, Band::ElsewhereWords, Present::OwnerAlone);
    assert_eq!(ids(&words), ["a-his"]);
}

#[test]
fn his_words_here_before_a_recent_new_mark_are_his_past() {
    let mut s = Store::default();
    s.ex.push(ex("before", "local", 1));
    s.ex.push(ex("long-before", "local", 3 * 24));
    s.marks.push((
        "local".into(),
        Pos {
            at_ms: NOW - H / 2,
            id: String::new(),
        },
    ));
    let p7 = select(&s, Band::Past7, Present::OwnerAlone);
    assert_eq!(ids(&p7), ["before", "long-before"]);
    let did = super::make::UNSUMMARISED.len().div_ceil(3) as u64;
    assert_eq!(p7[0].sizes.line, 10 + did, "with its did line");
    assert!(
        select(&s, Band::Floor, Present::OwnerAlone).is_empty(),
        "and not the window's"
    );
}

/// One generated exchange: place, hours ago, kind, tools, whose, private.
fn gen_ex(prefix: &'static str) -> impl Strategy<Value = Ex> {
    (
        0usize..5,
        0u64..2_000,
        0u8..4,
        0u64..3_000,
        0u8..3,
        any::<bool>(),
        0u32..1_000_000,
    )
        .prop_map(move |(place, hours, kind, tools, who, private, n)| Ex {
            turn: format!("{prefix}{n}"),
            place: ["local", "a", "b", "c", "d"][place].into(),
            at: NOW - hours * H - u64::from(n % 997),
            person: match who {
                0 => None,
                1 => Some("guest".into()),
                _ => Some("owner".into()),
            },
            audience: if private {
                Audience::Private
            } else {
                Audience::Shared
            },
            kind: [Kind::Ask, Kind::FollowUp, Kind::Fragment, Kind::Correction][kind as usize],
            tools,
            sealed: false,
        })
}

fn view_of(s: &Store, budget: &Budget, reverse: bool) -> Result<View, budget::Overflow> {
    let (frame, now) = parts();
    let mut table = table(sources(s, "see crate:x"), frame, now);
    if reverse {
        table.reverse();
    }
    assemble(&pov(Present::OwnerAlone), s, &table, &AllowPrivate, budget).expect("reads")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn the_order_of_selectors_and_what_the_gate_denies_do_not_change_the_view(
        base in prop::collection::vec(gen_ex("e"), 0..40),
        denied in prop::collection::vec(gen_ex("d"), 1..30),
        mark in prop::option::of(0u64..100),
        ceiling in 0u64..20_000,
        here in 0u64..20_000,
    ) {
        let budget = Budget { ceiling, here_allowance: here, provider_window: 400_000, reserve: 0, floor_only: false, bare: false };
        let mut s = Store { ex: base, ..Store::default() };
        s.ex.sort_by(|a, b| a.turn.cmp(&b.turn));
        s.ex.dedup_by(|a, b| a.turn == b.turn);
        if let Some(h) = mark {
            s.marks.push(("local".into(), Pos { at_ms: NOW - h * H, id: String::new() }));
        }
        let a = view_of(&s, &budget, false);
        prop_assert_eq!(&a, &view_of(&s, &budget, true), "the order selectors run in");
        prop_assert_eq!(&a, &view_of(&s, &budget, false), "deterministic");
        let mut more = Store { ex: s.ex.clone(), marks: s.marks.clone(), ..Store::default() };
        let mut seen = BTreeSet::new();
        more.ex.extend(denied.into_iter().filter(|d| seen.insert(d.turn.clone())).map(|mut d| {
            d.sealed = true;
            d
        }));
        prop_assert_eq!(&a, &view_of(&more, &budget, false), "what the gate denies");
        if let Ok(v) = &a {
            prop_assert!(v.tokens <= budget.total());
            let since = s.marks.first().map(|(_, m)| m.clone());
            let mut floor: Vec<&Ex> = s.ex.iter()
                .filter(|e| e.place == "local" && e.audience == Audience::Private)
                .filter(|e| since.as_ref().is_none_or(|m| (e.at, &e.turn) >= (m.at_ms, &m.id)))
                .collect();
            floor.sort_by_key(|e| std::cmp::Reverse((e.at, e.turn.clone())));
            for e in floor.iter().take(super::FLOOR_EXCHANGES) {
                prop_assert!(
                    v.shown.iter().any(|x| x.piece.handle.0 == format!("x:{}", e.turn)),
                    "the floor is always present: {}", e.turn
                );
            }
        }
    }
}

#[test]
fn a_source_without_old_candidates_does_not_walk_the_archive() {
    struct NoArchive;
    impl Timeline for NoArchive {
        fn newest(
            &self,
            _: &TimelineRoot,
            _: Option<&Cursor>,
            _: usize,
        ) -> anyhow::Result<Vec<ExchangeRef>> {
            anyhow::bail!("old archive must not be scanned when the source has no eligible facts")
        }
    }
    impl Snapshot for NoArchive {
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
    let dir = tempfile::tempdir().unwrap();
    let real = crate::store::Store::open(&dir.path().join("store")).unwrap();
    let sessions = super::super::sources_store::Sessions::default();
    let real_sources = super::super::sources_store::StoreSources {
        store: &real,
        mark: None,
        recalled: vec![],
        sessions: &sessions,
        own: None,
    };
    let fake = Store::default();
    let mut src = sources(&fake, "");
    src.exchanges = &real_sources;
    let selected = past::Past::new(Band::PastOlder, src)
        .select(&pov(Present::OwnerAlone), &NoArchive)
        .unwrap();
    assert!(selected.is_empty());
    assert_eq!(sessions.reads.get(), 0);
}
