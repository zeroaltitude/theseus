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
    /// The owner's records of people: a ledger name, and the id of the record that holds it.
    people: Vec<(String, String)>,
    /// The effective owner handles beside the ledger's own `owner`.
    owners: Vec<String>,
    /// What laying a recent line adds beyond its estimate and one heading (names, wrappers).
    framing: u64,
    /// The topics the judge filed an exchange under: a turn, and a topic.
    filed: Vec<(String, String)>,
    /// A task is joined: its card is the one the task's source returns.
    task: bool,
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

    fn persons(&self) -> anyhow::Result<Vec<String>> {
        let all: BTreeSet<String> = self.ex.iter().filter_map(|e| e.person.clone()).collect();
        Ok(all.into_iter().collect())
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
    /// A place with no fold yet: everything since the mark is the window (the count of 30 to 49 is the
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
    fn filed_in(&self, turn: &str) -> anyhow::Result<Vec<String>> {
        Ok((self.filed.iter())
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
        Ok(self.task.then(|| card("task")).into_iter().collect())
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

impl super::sources::People for Store {
    fn held(&self, person: &str) -> anyhow::Result<Option<Held>> {
        let Some(id) = (self.people.iter())
            .find(|(p, _)| p == person)
            .map(|(_, id)| id.clone())
        else {
            return Ok(None);
        };
        let owners: BTreeSet<&str> = (self.people.iter())
            .filter(|(p, _)| self.owner(p))
            .map(|(_, id)| id.as_str())
            .collect();
        Ok(Some(Held {
            owner: self.owner(person) || owners.contains(id.as_str()),
            handles: (self.people.iter())
                .filter(|(_, i)| *i == id)
                .map(|(p, _)| p.clone())
                .collect(),
            id,
        }))
    }
    fn owner(&self, person: &str) -> bool {
        person == "owner" || self.owners.iter().any(|o| o == person)
    }
    /// One fact piece per person, its handle saying the bound asked for.
    fn about(&self, id: &str, most: u64) -> anyhow::Result<Vec<Ref>> {
        let mut r = card(id);
        r.range.space = Space::Facts { thing: id.into() };
        r.handle = Handle(format!("{id} most={most}"));
        Ok(vec![r])
    }
    fn speaking(&self, id: &str) -> anyhow::Result<Vec<Ref>> {
        let mut r = card(id);
        r.range.space = Space::Facts {
            thing: format!("{id}#speaking"),
        };
        r.handle = Handle(format!("{id} speaking"));
        Ok(vec![r])
    }
    /// The line's estimate, one heading, and the framing a test gives every line.
    fn laid(&self, piece: &Piece, _: &PointOfView) -> anyhow::Result<u64> {
        Ok(piece.sizes.line + super::super::budget::HEADING_TOKENS + self.framing)
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
        sizes: Default::default(),
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

/// KEY (the owner's ruling, 2026-11-24): EVERYWHERE is the newest fifteen exchanges from every place this
/// conversation does not show, whoever opened them, a guest's in a shared place too: each one line of its own
/// place, newest first, within its line's bound.
#[test]
fn everywhere_shows_the_newest_fifteen_not_shown_here_with_their_places() {
    let places = ["a", "b", "chan", "discord:dm:9"];
    let mut ex_ = vec![by("owner", "here", "local", 1)];
    for i in 0..20u64 {
        let place = places[(i % 4) as usize];
        let (who, audience) = match place {
            "chan" if i % 8 == 2 => ("discord:bob", Audience::Shared),
            "chan" => ("owner", Audience::Shared),
            _ => ("owner", Audience::Private),
        };
        ex_.push(by_in(who, &format!("e{i:02}"), place, 2 + i, audience));
    }
    let s = company(
        &[("owner", "person:owner"), ("discord:bob", "person:bob")],
        ex_,
    );
    let laid = select_as(&s, Band::Everywhere, &pov_of("owner", "local"));
    let newest: Vec<String> = (0..EVERYWHERE_EXCHANGES)
        .map(|i| format!("e{i:02}"))
        .collect();
    assert_eq!(ids(&laid), newest);
    assert!(
        ids(&laid).contains(&"e02"),
        "a guest's words in a shared place"
    );
    for p in &laid {
        assert_eq!((p.slot, p.form), (Slot::Elsewhere, Form::Line));
        assert_eq!(p.reasons, BTreeSet::from([Reason::Elsewhere]));
        let Space::Exchanges { place } = &p.range.space else {
            panic!("not an exchange: {:?}", p.range)
        };
        assert!(places.contains(&place.as_str()), "{place}");
        assert_eq!(p.marks.place.as_deref(), Some(place.as_str()));
        assert!(p.sizes.line <= everywhere::EVERYWHERE_LINE_TOKENS);
    }
}

/// KEY: how many lines EVERYWHERE lays is the setting `[context] everywhere_exchanges`, as HERE's size is a
/// setting: a setting of 5 lays the newest 5.
#[test]
fn everywhere_lays_as_many_as_its_setting_names() {
    let mut s = Store::default();
    for i in 0..20u64 {
        s.ex.push(ex(&format!("e{i:02}"), ["a", "b"][(i % 2) as usize], 1 + i));
    }
    let mut src = sources(&s, "");
    src.sizes = crate::config::context::Sizes {
        everywhere: 5,
        ..Default::default()
    };
    let table = table(src, Vec::new(), Vec::new());
    let sel = (table.iter())
        .find(|t| t.band() == Band::Everywhere)
        .expect("a selector for EVERYWHERE");
    let mut laid = sel.select(&pov(Present::OwnerAlone), &s).expect("selects");
    laid.sort_by_key(|p| std::cmp::Reverse(p.range.last.clone()));
    assert_eq!(ids(&laid), ["e00", "e01", "e02", "e03", "e04"]);
}

/// KEY (the conductor's ruling on the plan's Q2): this place's exchanges since its `/new` mark are its window
/// and ladder's, and left out; so are those from before it, his past, which nothing lays as anything else.
#[test]
fn everywhere_leaves_out_every_exchange_of_this_place() {
    let mut s = Store::default();
    s.ex.push(ex("now1", "local", 1));
    s.ex.push(ex("now2", "local", 3));
    s.ex.push(ex("far", "a", 5));
    s.ex.push(ex("before", "local", 20));
    s.marks.push((
        "local".into(),
        Pos {
            at_ms: NOW - 15 * H,
            id: String::new(),
        },
    ));
    assert_eq!(
        ids(&select(&s, Band::Everywhere, Present::OwnerAlone)),
        ["far"]
    );
    s.marks.clear();
    assert_eq!(
        ids(&select(&s, Band::Everywhere, Present::OwnerAlone)),
        ["far"]
    );
}

/// KEY (the conductor's ruling on the plan's Q2): a task's own workings and a wake, which no person opened,
/// are not among the lines from every place.
#[test]
fn everywhere_leaves_out_tasks_and_wakes() {
    let mut s = Store::default();
    let mut wake = ex("wake", "a", 1);
    wake.person = None;
    s.ex.push(wake);
    let task = format!(
        "{}{}1",
        crate::task_graph::place::PREFIX,
        crate::task_graph::ID_PREFIX
    );
    s.ex.push(ex("work", &task, 2));
    s.ex.push(ex("said", "a", 3));
    assert_eq!(
        ids(&select(&s, Band::Everywhere, Present::OwnerAlone)),
        ["said"]
    );
}

/// KEY: the look across every place reads at most its bound of references, and says it stopped there; with
/// room to spare it reads to the end and says it did not stop.
#[test]
fn everywhere_reads_at_most_its_bound_and_says_so() {
    let mut ex_: Vec<Ex> = (0..400u64)
        .map(|i| by("owner", &format!("h{i:03}"), "local", 1 + i))
        .collect();
    ex_.push(by("owner", "far", "a", 500));
    let s = Store {
        ex: ex_,
        ..Store::default()
    };
    let look = |s: &Store, most: usize| {
        let bounded = Bounded {
            s,
            most,
            read: Default::default(),
        };
        everywhere::Everywhere::new(sources_with(s, "", &AdmitUnsealed))
            .look(&pov_of("owner", "local"), &bounded)
            .expect("reads within its bound")
    };
    let busy = look(&s, everywhere::EVERYWHERE_READ_MOST);
    assert!(busy.stopped, "this place's 400 spend the bound");
    assert!(busy.pieces.is_empty(), "the older place is past the bound");
    let quiet = Store {
        ex: s.ex[390..].to_vec(),
        ..Store::default()
    };
    let roomy = look(&quiet, everywhere::EVERYWHERE_READ_MOST);
    assert!(!roomy.stopped);
    assert_eq!(ids(&roomy.pieces), ["far"]);
}

/// KEY (the owner's ruling, 2026-11-24, "no conversations are private"; the conductor's ruling on the plan's
/// Q1): on a guest's turn in a shared place, the lines from every place are the owner's turn's, his own
/// private ones among them. One switch says so. A person present but the owner has their newest elsewhere in
/// the people block, not here.
#[test]
fn a_guests_turn_lays_everywhere_as_the_owners_does() {
    let s = company(
        &[
            ("owner", "person:owner"),
            ("discord:bob", "person:bob"),
            ("discord:cy", "person:cy"),
        ],
        vec![
            by("owner", "dm1", "discord:dm:9", 1),
            by_in("discord:cy", "c1", "general", 2, Audience::Shared),
            by_in("discord:bob", "b1", "general", 3, Audience::Shared),
            by_in("discord:bob", "here", "chan", 4, Audience::Shared),
        ],
    );
    let owners = select_as(&s, Band::Everywhere, &pov_of("owner", "chan"));
    let guests = select_as(&s, Band::Everywhere, &pov_of("discord:bob", "chan"));
    assert_eq!(ids(&owners), ["dm1", "c1"]);
    assert_eq!(ids(&guests), ids(&owners));
    let elsewhere = select_as(&s, Band::Everywhere, &pov_of("owner", "local"));
    assert_eq!(ids(&elsewhere), ["dm1", "c1", "b1", "here"]);
    const { assert!(everywhere::THE_OWNERS_LINES_ON_A_GUESTS_TURN) };
    assert!(everywhere::may_lay(false, true));
}

/// A self-loop session's turn lays its own place only: nothing from every place.
#[test]
fn a_self_loop_turn_lays_no_everywhere() {
    struct OwnOnly;
    impl Gate for OwnOnly {
        fn admit(&self, pov: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
            AdmitUnsealed.admit(pov, piece)
        }
        fn own_place_only(&self) -> bool {
            true
        }
    }
    let mut s = Store::default();
    s.ex.push(ex("t1", "local", 1));
    s.ex.push(ex("far", "a", 2));
    let laid = |gate: &dyn Gate| {
        let v = assembled(&s, gate, "hello").view.expect("fits");
        (v.shown.iter())
            .filter(|x| x.piece.band == Band::Everywhere)
            .count()
    };
    assert_eq!(laid(&AdmitUnsealed), 1);
    assert_eq!(laid(&OwnOnly), 0);
}

/// An old view record naming slot F's two bands still reads: both are `retired`; slot F keeps its stored
/// name; a band name no build wrote is refused, as it always was.
#[test]
fn an_old_view_record_naming_slot_fs_bands_still_reads() {
    for old in ["elsewhere_words", "elsewhere_replies"] {
        let band: Band = serde_json::from_value(serde_json::json!(old)).expect(old);
        assert_eq!(band, Band::Retired, "{old}");
    }
    assert_eq!(
        serde_json::to_value(Band::Everywhere).unwrap(),
        "everywhere"
    );
    assert_eq!(serde_json::to_value(Slot::Elsewhere).unwrap(), "elsewhere");
    assert!(serde_json::from_value::<Band>(serde_json::json!("elsewhere_lately")).is_err());
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
    assert!(bands.contains(&Band::Everywhere));
}

/// Admits what is private and not sealed.
struct AllowPrivate;

impl Gate for AllowPrivate {
    fn admit(&self, _: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        (piece.marks.audience == Audience::Private && !piece.marks.sealed)
            .then(|| Admitted::for_tests(piece))
    }
}

/// Admits what today's permit admits in the owner's trusted places: every piece but a sealed one, of any
/// audience.
struct AdmitUnsealed;

impl Gate for AdmitUnsealed {
    fn admit(&self, _: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
        (!piece.marks.sealed).then(|| Admitted::for_tests(piece))
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

/// One exchange of another place is found past many exchanges here and many wakes there.
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
    let words = select(&s, Band::Everywhere, Present::OwnerAlone);
    assert_eq!(ids(&words), ["a-his"]);
}

/// One generated exchange: place, hours ago, tools, whose, private.
fn gen_ex(prefix: &'static str) -> impl Strategy<Value = Ex> {
    (
        0usize..5,
        0u64..2_000,
        0u64..3_000,
        0u8..3,
        any::<bool>(),
        0u32..1_000_000,
    )
        .prop_map(move |(place, hours, tools, who, private, n)| Ex {
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
        let budget = Budget { ceiling, here_allowance: here, provider_window: 400_000, reserve: 0, floor_only: false, bare: false, topic: ceiling };
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

/// The owner's ruling of 2026-11-24: the slices of his words by age (slot E) are removed. No row of the table
/// lays the past, by any band's or slot's name.
#[test]
fn the_view_has_no_band_by_age() {
    for row in ADMISSION {
        let band = serde_json::to_value(row.band).unwrap();
        let slot = serde_json::to_value(row.slot).unwrap();
        assert_ne!(slot, "past", "{band}");
        assert!(
            !band.as_str().unwrap_or_default().starts_with("past"),
            "{band}"
        );
    }
}

/// The people of one shared place, `chan`, as the selectors of slot B see them.
fn company(people: &[(&str, &str)], ex_: Vec<Ex>) -> Store {
    Store {
        ex: ex_,
        people: (people.iter())
            .map(|(p, id)| (p.to_string(), id.to_string()))
            .collect(),
        ..Store::default()
    }
}

fn by(person: &str, turn: &str, place: &str, hours_ago: u64) -> Ex {
    Ex {
        person: Some(person.into()),
        ..ex(turn, place, hours_ago)
    }
}

/// [`by`], in a place of `audience` (the place rule's class when it was said).
fn by_in(person: &str, turn: &str, place: &str, hours_ago: u64, audience: Audience) -> Ex {
    Ex {
        audience,
        ..by(person, turn, place, hours_ago)
    }
}

fn pov_of(person: &str, place: &str) -> PointOfView {
    PointOfView {
        anchor: Anchor::Place(place.into()),
        person: Some(person.into()),
        present: Present::Shared,
        at_hand: None,
        now_ms: NOW,
    }
}

fn select_as(s: &Store, band: Band, pov: &PointOfView) -> Vec<Piece> {
    let table = table(sources_with(s, "", &AdmitUnsealed), Vec::new(), Vec::new());
    let sel = table
        .iter()
        .find(|t| t.band() == band)
        .expect("a selector per band");
    let mut out = sel.select(pov, s).expect("selects");
    out.sort_by_key(|p| std::cmp::Reverse(p.range.last.clone()));
    out
}

fn things(pieces: &[Piece]) -> Vec<String> {
    let mut out: Vec<String> = (pieces.iter())
        .map(|p| match &p.range.space {
            Space::Facts { thing } => thing.clone(),
            other => panic!("not a person's facts: {other:?}"),
        })
        .collect();
    out.sort();
    out
}

/// The recent lines among what is laid beside the message: the exchanges, newest first.
fn recent(pieces: &[Piece]) -> Vec<&str> {
    (pieces.iter())
        .filter(|p| matches!(p.range.space, Space::Exchanges { .. }))
        .map(|p| p.range.last.id.as_str())
        .collect()
}

/// The facts laid beside the message: the speaker's line.
fn speaking(pieces: &[Piece]) -> Vec<String> {
    let facts: Vec<Piece> = (pieces.iter())
        .filter(|p| matches!(p.range.space, Space::Facts { .. }))
        .cloned()
        .collect();
    things(&facts)
}

/// KEY (the owner's word, 2026-11-24): the people present are the speaker and the distinct persons who opened
/// the window's exchanges, each named once whatever handle they spoke by; a person no record holds is not
/// named, nor one who spoke only elsewhere. Their facts are in the front (slot B), alike for every person.
#[test]
fn in_company_every_person_in_the_window_is_named_once() {
    let s = company(
        &[
            ("owner", "person:owner"),
            ("discord:bob", "person:bob"),
            ("discord:carol", "person:carol"),
            ("slack:carol", "person:carol"),
            ("discord:frank", "person:frank"),
        ],
        vec![
            by("owner", "o1", "chan", 6),
            by("discord:bob", "b1", "chan", 5),
            by("discord:carol", "c1", "chan", 4),
            by("discord:bob", "b2", "chan", 3),
            by("slack:carol", "c2", "chan", 2),
            by("discord:eve", "e1", "chan", 1),
            by("discord:frank", "f1", "other", 1),
        ],
    );
    let laid = select_as(&s, Band::WhoAndWhere, &pov_of("discord:bob", "chan"));
    assert_eq!(
        things(&laid),
        ["person:bob", "person:carol", "person:owner"]
    );
    for p in &laid {
        assert_eq!((p.slot, p.form), (Slot::WhoAndWhere, Form::Whole));
        assert_eq!(p.reasons, BTreeSet::from([Reason::Fixed]));
        let asked = format!("most={PERSON_TOKENS}");
        assert!(p.handle.0.ends_with(&asked), "{}", p.handle.0);
    }
    // Alone, the one person has the whole bound; a turn with no speaker is told nobody.
    let alone = select_as(&s, Band::WhoAndWhere, &pov_of("owner", "elsewhere"));
    assert_eq!(things(&alone), ["person:owner"]);
    assert!(alone[0]
        .handle
        .0
        .ends_with(&format!("most={PEOPLE_TOKENS}")));
    let nobody = PointOfView {
        person: None,
        ..pov_of("owner", "chan")
    };
    assert!(select_as(&s, Band::WhoAndWhere, &nobody).is_empty());
    assert!(select_as(&s, Band::PeopleRecent, &nobody).is_empty());
}

/// KEY (the owner's word, 2026-11-24): the front names the same people in the same words whoever of them
/// speaks; the one line beside the message names the speaker.
#[test]
fn the_front_is_the_same_whoever_speaks_and_the_speaker_is_named_beside() {
    let s = company(
        &[
            ("owner", "person:owner"),
            ("discord:bob", "person:bob"),
            ("discord:carol", "person:carol"),
        ],
        vec![
            by("discord:bob", "b1", "chan", 3),
            by("discord:carol", "c1", "chan", 2),
            by("owner", "o1", "chan", 1),
        ],
    );
    let front = |who: &str| -> Vec<(String, String)> {
        let laid = select_as(&s, Band::WhoAndWhere, &pov_of(who, "chan"));
        let mut out: Vec<(String, String)> = (laid.iter())
            .map(|p| (format!("{:?}", p.range), p.handle.0.clone()))
            .collect();
        out.sort();
        out
    };
    assert_eq!(front("discord:bob"), front("discord:carol"));
    assert_eq!(front("discord:bob"), front("owner"));
    for (who, id) in [
        ("discord:bob", "person:bob"),
        ("discord:carol", "person:carol"),
    ] {
        let beside = select_as(&s, Band::PeopleRecent, &pov_of(who, "chan"));
        assert_eq!(speaking(&beside), [format!("{id}#speaking")], "{who}");
    }
    // A speaker no record holds is named nowhere.
    let beside = select_as(&s, Band::PeopleRecent, &pov_of("discord:zed", "chan"));
    assert!(speaking(&beside).is_empty());
}

/// KEY (the owner's word, 2026-11-24): a person's recent lines are their newest three exchanges in other
/// places, on their own timeline, leaving out every exchange the window lays; one line each, with its place
/// and the handle that opens it, beside the message.
#[test]
fn a_persons_recent_lines_leave_out_this_window() {
    let s = company(
        &[("owner", "person:owner"), ("discord:bob", "person:bob")],
        vec![
            by("owner", "o1", "chan", 9),
            by("discord:bob", "b1", "chan", 4),
            by("discord:bob", "b2", "chan", 1),
            by("discord:bob", "bd1", "dm", 2),
            by("discord:bob", "bg1", "general", 5),
            by("discord:bob", "bg2", "general", 6),
            by("discord:bob", "bg3", "general", 7),
            by("owner", "o2", "dm-owner", 3),
        ],
    );
    let lines = select_as(&s, Band::PeopleRecent, &pov_of("owner", "chan"));
    assert_eq!(recent(&lines), ["bd1", "bg1", "bg2"]);
    for p in lines
        .iter()
        .filter(|p| p.reasons.contains(&Reason::Present))
    {
        assert_eq!((p.slot, p.form), (Slot::WhoAndWhere, Form::Line));
        assert_eq!(p.reasons, BTreeSet::from([Reason::Present]));
        assert_ne!(p.marks.place.as_deref(), Some("chan"));
        assert_eq!(p.handle.0, format!("x:{}", p.range.last.id));
        assert!(p.sizes.line <= RECENT_LINE_TOKENS);
    }
}

/// KEY (the owner's word, 2026-11-24): the owner gets no recent lines of his own: they are in this place's
/// window and the look-backs, and his facts in the frame. Beside the message he is named as the speaker.
#[test]
fn alone_the_owner_gets_no_recent_lines_of_his_own() {
    let s = company(
        &[("owner", "person:owner")],
        vec![
            by("owner", "o1", "local", 3),
            by("owner", "o2", "dm", 2),
            by("owner", "o3", "general", 1),
        ],
    );
    let me = pov_of("owner", "local");
    let beside = select_as(&s, Band::PeopleRecent, &me);
    assert!(recent(&beside).is_empty(), "{:?}", recent(&beside));
    assert_eq!(speaking(&beside), ["person:owner#speaking"]);
    assert_eq!(
        things(&select_as(&s, Band::WhoAndWhere, &me)),
        ["person:owner"]
    );
}

/// KEY: the owner is the owner by identity, not by one ledger spelling. Each of his effective handles, a
/// raw transport handle an older unproven mark kept (later held as his), a handle of his record that no
/// setting names, and a second record another owner handle names: none gets a recent line, while a guest
/// speaks and he is met only through the window. The guest's are laid.
#[test]
fn the_owner_gets_no_recent_lines_by_any_handle_of_his() {
    let shared = Audience::Shared;
    let mut s = company(
        &[
            ("owner", "person:me"),
            ("discord:9", "person:me"),
            ("slack:me", "person:me"),
            ("discord:7", "person:me-too"),
            ("discord:bob", "person:bob"),
        ],
        vec![
            // The window: his words under three spellings, the newest a raw handle; the guest's.
            by("owner", "o1", "chan", 9),
            by("discord:9", "r1", "chan", 8),
            by("slack:me", "k1", "chan", 7),
            by("discord:7", "t1", "chan", 6),
            by("discord:bob", "b1", "chan", 5),
            // Elsewhere, newest first.
            by_in("discord:9", "r2", "general", 1, shared),
            by_in("slack:me", "k2", "general", 2, shared),
            by_in("discord:7", "t2", "general", 3, shared),
            by_in("discord:bob", "b2", "general", 4, shared),
        ],
    );
    s.owners = vec!["discord:9".into(), "discord:7".into()];
    let bob = pov_of("discord:bob", "chan");
    let beside = select_as(&s, Band::PeopleRecent, &bob);
    assert_eq!(recent(&beside), ["b2"]);
    let front = things(&select_as(&s, Band::WhoAndWhere, &bob));
    assert_eq!(front, ["person:bob", "person:me", "person:me-too"]);
    // He speaking by his raw handle is the owner too, and the guest's line is still laid.
    let raw = select_as(&s, Band::PeopleRecent, &pov_of("discord:9", "chan"));
    assert_eq!(recent(&raw), ["b2"]);
}

/// KEY: "their newest three" are the held person's, by every handle: one who spoke here by one handle and
/// more recently elsewhere by another has the newer ones; one shared allowance of touches read is spent on
/// all their handles together.
#[test]
fn a_persons_newest_three_are_theirs_by_every_handle() {
    let shared = Audience::Shared;
    let s = company(
        &[
            ("owner", "person:owner"),
            ("discord:bob", "person:bob"),
            ("slack:bob", "person:bob"),
        ],
        vec![
            by("discord:bob", "b1", "chan", 1),
            by_in("discord:bob", "d1", "general", 10, shared),
            by_in("discord:bob", "d2", "general", 11, shared),
            by_in("discord:bob", "d3", "general", 12, shared),
            by_in("slack:bob", "s1", "team", 2, shared),
            by_in("slack:bob", "s2", "team", 3, shared),
        ],
    );
    let beside = select_as(&s, Band::PeopleRecent, &pov_of("owner", "chan"));
    assert_eq!(recent(&beside), ["s1", "s2", "d1"]);

    // 200 touches here by each handle, newer than any line elsewhere: 400 to read, one allowance of 300.
    let mut ex_ = Vec::new();
    for i in 0..200u64 {
        ex_.push(by("discord:bob", &format!("hd{i}"), "chan", 1 + 2 * i));
        ex_.push(by("slack:bob", &format!("hs{i}"), "chan", 2 + 2 * i));
    }
    ex_.push(by_in("discord:bob", "far-d", "general", 900, shared));
    ex_.push(by_in("slack:bob", "far-s", "team", 901, shared));
    let s = Store { ex: ex_, ..s };
    let beside = select_as(&s, Band::PeopleRecent, &pov_of("owner", "chan"));
    assert!(recent(&beside).is_empty(), "{:?}", recent(&beside));
}

/// The store as a snapshot, counting every reference its timeline returns.
struct Counted<'a> {
    s: &'a Store,
    read: std::cell::Cell<usize>,
}

impl Timeline for Counted<'_> {
    fn newest(
        &self,
        root: &TimelineRoot,
        before: Option<&Cursor>,
        n: usize,
    ) -> anyhow::Result<Vec<ExchangeRef>> {
        let page = self.s.newest(root, before, n)?;
        self.read.set(self.read.get() + page.len());
        Ok(page)
    }

    fn persons(&self) -> anyhow::Result<Vec<String>> {
        self.s.persons()
    }
}

impl Snapshot for Counted<'_> {
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

/// KEY: one person's recent lines read at most `RECENT_READ_MOST` references of their timelines in all,
/// every handle and every page, a touch met twice counted twice: four handles, each with more than a page
/// and none of it laid; one handle with one exchange past the allowance, none of it laid.
#[test]
fn a_persons_recent_lines_read_one_allowance_in_all() {
    let four = ["discord:bob", "slack:bob", "discord:bob2", "slack:bob2"];
    let mut ex_ = Vec::new();
    for (k, h) in four.iter().enumerate() {
        for i in 0..150u64 {
            ex_.push(by(h, &format!("{k}-{i}"), "chan", 1 + 4 * i + k as u64));
        }
    }
    let mut held: Vec<(&str, &str)> = vec![("owner", "person:owner")];
    held.extend(four.iter().map(|h| (*h, "person:bob")));
    let s4 = company(&held, ex_);
    let one = company(
        &[("owner", "person:owner"), ("discord:cy", "person:cy")],
        (0..=people::RECENT_READ_MOST as u64)
            .map(|i| by("discord:cy", &format!("c{i}"), "chan", 1 + i))
            .collect(),
    );
    let mut reads = Vec::new();
    for s in [&s4, &one] {
        let table = table(sources_with(s, "", &AdmitUnsealed), Vec::new(), Vec::new());
        let sel = (table.iter())
            .find(|t| t.band() == Band::PeopleRecent)
            .expect("a selector per band");
        let counted = Counted {
            s,
            read: Default::default(),
        };
        let beside = sel
            .select(&pov_of("owner", "chan"), &counted)
            .expect("selects");
        assert!(recent(&beside).is_empty(), "{:?}", recent(&beside));
        reads.push(counted.read.get());
    }
    let most = people::RECENT_READ_MOST;
    assert!(
        reads.iter().all(|r| (1..=most).contains(r)),
        "references read: {reads:?}"
    );
}

/// KEY (the owner's word, 2026-11-24: no conversation is private): in company, a person's line from a
/// one-to-one place is laid, as is one from a place the place rule held private and one from a shared place,
/// whoever speaks and whoever else is here, held or not.
#[test]
fn in_company_a_persons_line_from_a_one_to_one_place_is_laid() {
    let s = company(
        &[
            ("owner", "person:owner"),
            ("discord:bob", "person:bob"),
            ("discord:carol", "person:carol"),
        ],
        vec![
            by("discord:bob", "b1", "chan", 9),
            by("discord:carol", "c1", "chan", 8),
            by("discord:zed", "z1", "chan", 7),
            by_in("discord:bob", "dm1", "discord:dm:44", 1, Audience::Shared),
            by_in("discord:bob", "lab1", "lab", 2, Audience::Private),
            by_in("discord:bob", "gen1", "general", 3, Audience::Shared),
        ],
    );
    for who in ["owner", "discord:carol", "discord:zed"] {
        let beside = select_as(&s, Band::PeopleRecent, &pov_of(who, "chan"));
        assert_eq!(recent(&beside), ["dm1", "lab1", "gen1"], "{who} speaking");
    }
}

/// At most eight people are named: the speaker, then the window's people who spoke most recently.
#[test]
fn a_person_past_the_eighth_is_not_laid() {
    let names: Vec<String> = (0..10).map(|i| format!("discord:p{i}")).collect();
    let records: Vec<(&str, String)> = (names.iter())
        .map(|n| (n.as_str(), format!("person:{}", &n[8..])))
        .collect();
    let people: Vec<(&str, &str)> = records.iter().map(|(p, id)| (*p, id.as_str())).collect();
    // p0 spoke first and p9 last.
    let window = (0..10u64)
        .map(|i| by(&names[i as usize], &format!("t{i}"), "chan", 20 - i))
        .collect();
    let s = company(&people, window);
    let laid = select_as(&s, Band::WhoAndWhere, &pov_of("discord:p0", "chan"));
    assert_eq!(laid.len(), MOST_PEOPLE);
    let named = things(&laid);
    assert!(
        named.contains(&"person:p0".to_string()),
        "the speaker is always named: {named:?}"
    );
    for gone in ["person:p1", "person:p2"] {
        assert!(!named.contains(&gone.to_string()), "{gone}: {named:?}");
    }
}

/// Every person's recent lines together keep to their bound as laid (`People::laid`), whole lines only,
/// however much a name's framing costs; a self-loop session's turn, which lays its own place only, lays
/// none.
#[test]
fn the_recent_lines_keep_to_their_bound_and_a_self_loop_lays_none() {
    let mut ex_ = Vec::new();
    let mut people = vec![("owner".to_string(), "person:owner".to_string())];
    for i in 0..8u64 {
        let p = format!("discord:q{i}");
        ex_.push(by(&p, &format!("w{i}"), "local", 30 + i));
        for k in 0..4u64 {
            ex_.push(by_in(
                &p,
                &format!("e{i}-{k}"),
                &format!("place{k}"),
                1 + i + k,
                Audience::Shared,
            ));
        }
        people.push((p, format!("person:q{i}")));
    }
    let mut s = Store {
        ex: ex_,
        people,
        ..Store::default()
    };
    let mut counts = Vec::new();
    for framing in [0, 150] {
        s.framing = framing;
        let lines = select_as(&s, Band::PeopleRecent, &pov(Present::OwnerAlone));
        let lines: Vec<&Piece> = (lines.iter())
            .filter(|p| p.reasons.contains(&Reason::Present))
            .collect();
        assert!(!lines.is_empty());
        let spent: u64 = (lines.iter())
            .map(|p| p.sizes.line + super::super::budget::HEADING_TOKENS + framing)
            .sum();
        assert!(spent <= RECENT_TOKENS, "framing {framing}: {spent}");
        counts.push(lines.len());
    }
    assert!(counts[1] < counts[0], "{counts:?}");
    struct OwnOnly;
    impl Gate for OwnOnly {
        fn admit(&self, pov: &PointOfView, piece: Piece) -> Option<Admitted<Piece>> {
            AdmitUnsealed.admit(pov, piece)
        }
        fn own_place_only(&self) -> bool {
            true
        }
    }
    let band = |a: &Assembled| {
        let v = a.view.as_ref().expect("fits");
        v.shown
            .iter()
            .filter(|s| s.piece.band == Band::PeopleRecent)
            .count()
    };
    s.framing = 0;
    assert!(band(&assembled(&s, &AdmitUnsealed, "hello")) > 0);
    assert_eq!(band(&assembled(&s, &OwnOnly, "hello")), 0);
}

/// What a walk gives, in order, each exchange once, until it ends.
fn walked(w: &mut walk::Walk<'_>) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(touch) = w.next_touch().expect("reads") {
        out.extend(touch.map(|e| e.turn_id));
    }
    out
}

/// KEY: every person's timeline walked as one is each exchange a person opened, once, newest first, across
/// places and persons; a wake, which no person opened, is not on it.
#[test]
fn every_persons_walk_is_each_exchange_with_a_person_once_newest_first() {
    let wake = Ex {
        person: None,
        ..ex("w1", "chan", 2)
    };
    let s = Store {
        ex: vec![
            by("owner", "o1", "chan", 1),
            wake,
            by("discord:bob", "b1", "general", 3),
            by("owner", "o2", "local", 4),
            by("discord:bob", "b2", "discord:dm:44", 5),
            by("slack:cy", "c1", "team", 6),
            // As an older store could hold it: one exchange touched under a second person too.
            by("slack:cy", "o2", "local", 4),
        ],
        ..Store::default()
    };
    let mut w = walk::Walk::every_person(&s, people::RECENT_READ_MOST).expect("lists persons");
    assert_eq!(walked(&mut w), ["o1", "b1", "o2", "b2", "c1"]);
    assert!(!w.stopped());
}

/// The store as a snapshot whose timeline fails a read past `most` references in all.
struct Bounded<'a> {
    s: &'a Store,
    most: usize,
    read: std::cell::Cell<usize>,
}

impl Timeline for Bounded<'_> {
    fn newest(
        &self,
        root: &TimelineRoot,
        before: Option<&Cursor>,
        n: usize,
    ) -> anyhow::Result<Vec<ExchangeRef>> {
        let page = self.s.newest(root, before, n)?;
        self.read.set(self.read.get() + page.len());
        anyhow::ensure!(
            self.read.get() <= self.most,
            "{} references read, past {}",
            self.read.get(),
            self.most
        );
        Ok(page)
    }

    fn persons(&self) -> anyhow::Result<Vec<String>> {
        self.s.persons()
    }
}

impl Snapshot for Bounded<'_> {
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

/// KEY: a walk over every person reads at most its allowance of references, and says it stopped there: what
/// it gave is the newest, in order. With room to spare it reads every timeline to its end, and says it did
/// not stop.
#[test]
fn the_walk_reads_at_most_its_allowance() {
    let four = ["owner", "discord:bob", "slack:cy", "discord:dee"];
    let all: Vec<String> = (0..400u64).map(|i| format!("t{i:03}")).collect();
    let s = Store {
        ex: (0..400u64)
            .map(|i| by(four[(i % 4) as usize], &all[i as usize], "chan", 1 + i))
            .collect(),
        ..Store::default()
    };
    let most = people::RECENT_READ_MOST;
    let bounded = Bounded {
        s: &s,
        most,
        read: Default::default(),
    };
    let mut w = walk::Walk::every_person(&bounded, most).expect("lists persons");
    let got = walked(&mut w);
    assert!(w.stopped(), "{} given", got.len());
    assert!(!got.is_empty() && got.len() < all.len(), "{}", got.len());
    assert_eq!(got, all[..got.len()], "the newest, in order");
    let roomy = Bounded {
        s: &s,
        most: 1_000,
        read: Default::default(),
    };
    let mut w = walk::Walk::every_person(&roomy, 1_000).expect("lists persons");
    assert_eq!(walked(&mut w), all);
    assert!(!w.stopped());
}

/// The cards of the topic block (slot H) as a whole turn shows them, in order.
fn topic_block(s: &Store, message: &str) -> Vec<(Band, String)> {
    let v = assembled(s, &AllowPrivate, message).view.expect("fits");
    (v.shown.iter())
        .filter(|x| x.piece.slot == Slot::Called)
        .map(|x| (x.piece.band, x.piece.handle.0.clone()))
        .collect()
}

#[test]
fn topic_lays_the_task_card_then_named_then_in_play_then_recall() {
    let mut s = Store {
        task: true,
        ..Store::default()
    };
    s.ex.push(ex("t1", "local", 1));
    s.named.push(("t1".into(), "issue:44".into()));
    s.filed.push(("t1".into(), "topic:ledger".into()));
    let block = topic_block(&s, "see crate:x");
    let want = [
        (Band::TaskCard, "card:task"),
        (Band::Named, "card:crate:x"),
        (Band::InPlay, "card:topic:ledger"),
        (Band::InPlay, "card:issue:44"),
        (Band::Recall, "card:recalled"),
    ]
    .map(|(b, h)| (b, h.to_string()));
    assert_eq!(
        block, want,
        "one block, in one order: the task, named, in play, recall"
    );
}

#[test]
fn a_topic_the_judge_filed_the_last_exchange_under_is_in_view() {
    let mut s = Store::default();
    s.ex.push(ex("t1", "local", 1));
    s.ex.push(ex("a1", "a", 1));
    s.filed.push(("t1".into(), "topic:ledger".into()));
    s.filed.push(("a1".into(), "topic:elsewhere".into()));
    let played: Vec<String> = (select(&s, Band::InPlay, Present::OwnerAlone).into_iter())
        .map(|p| p.handle.0)
        .collect();
    assert_eq!(
        played,
        ["card:topic:ledger"],
        "the topic of an exchange here, not of one in another place"
    );
}

#[test]
fn a_thing_named_twice_is_one_card() {
    let mut s = Store::default();
    s.ex.push(ex("t1", "local", 2));
    s.ex.push(ex("t2", "local", 1));
    s.filed.push(("t1".into(), "topic:ledger".into()));
    s.filed.push(("t2".into(), "topic:ledger".into()));
    let v = assembled(&s, &AllowPrivate, "see topic:ledger")
        .view
        .expect("fits");
    let cards: Vec<&budget::Shown> = (v.shown.iter())
        .filter(|x| x.piece.handle.0 == "card:topic:ledger")
        .collect();
    assert_eq!(cards.len(), 1, "one card: {cards:?}");
    assert_eq!(
        cards[0].piece.reasons,
        BTreeSet::from([Reason::Named, Reason::InPlay]),
        "named in this message, and in play by the last two exchanges' filing"
    );
}

#[test]
fn an_old_record_naming_in_play_and_reached_still_reads() {
    for (band, want) in [
        ("\"in_play\"", Band::InPlay),
        ("\"reached\"", Band::Reached),
    ] {
        assert_eq!(serde_json::from_str::<Band>(band).unwrap(), want);
    }
    assert_eq!(
        serde_json::from_str::<Slot>("\"in_play\"").unwrap(),
        Slot::InPlay,
        "an older record's slot D reads as it was written"
    );
    assert_eq!(
        Band::InPlay.row().slot,
        Slot::Called,
        "what is in play is laid in the topic block now"
    );
}
