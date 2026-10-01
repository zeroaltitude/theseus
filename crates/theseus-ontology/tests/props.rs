//! Property tests (21a): a snapshot is built by a random run of writes
//! through `put` (most refused, which must leave it as it was), then:
//! - it rebuilds from its own records, in any order, with nothing dropped;
//! - any valid set of memberships composes without a skip, deterministically
//!   and whatever their order, every category in play admitted once, in
//!   precedence order with ancestors first, and the render well formed;
//! - a row the table accepts names only a rule the compiler reads and
//!   origins something writes (the reader rule).
//!
//! A run is a second or two; the seed is random, so the gate keeps looking.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::{select, Index};
use proptest::test_runner::Config;
use theseus_ontology::{
    Basis, CategoryId, Composition, Guidance, Kind, MemberList, Membership, Ontology, Origin,
    PerSession, Record, Rule, PREAMBLE,
};

use common::{category, id};

fn cases(n: u32) -> Config {
    Config {
        cases: n,
        // A failure prints its shrunk input; nothing is written into the tree.
        failure_persistence: None,
        ..Config::default()
    }
}

/// Names that collide (case aside), nest, and carry the path separator.
const NAMES: [&str; 7] = ["a", "b", "A", "theseus", "rust-harness", "x › y", "日本"];

/// Guidance: none, one line, two lines, a long line, a tab, and too much.
fn text(n: usize) -> String {
    match n {
        0 => String::new(),
        1 => "One line.".into(),
        2 => "Two\nlines.".into(),
        3 => "x".repeat(250),
        4 => "Tab\tthen text.".into(),
        _ => "y".repeat(17_000),
    }
}

#[derive(Debug, Clone)]
enum Op {
    Guild(usize),
    Channel(Index, usize),
    Person(usize),
    /// A category of an interpreted kind: topic, or culture when the table
    /// has it.
    Interp(bool, Option<Index>, usize),
    /// A topic, given a new parent (or none).
    Move(Index, Option<Index>),
    Guide(Index, usize),
    Members(usize, bool, Vec<Index>),
    TopicPrecedence(u32),
    TopicRule(bool),
    TopicNests(bool),
    Culture(u32, bool, bool),
}

fn op() -> impl Strategy<Value = Op> {
    let n = 0..NAMES.len();
    prop_oneof![
        1 => n.clone().prop_map(Op::Guild),
        2 => (any::<Index>(), n.clone()).prop_map(|(g, n)| Op::Channel(g, n)),
        2 => n.clone().prop_map(Op::Person),
        4 => (any::<bool>(), proptest::option::of(any::<Index>()), n)
            .prop_map(|(c, p, n)| Op::Interp(c, p, n)),
        1 => (any::<Index>(), proptest::option::of(any::<Index>()))
            .prop_map(|(t, p)| Op::Move(t, p)),
        4 => (any::<Index>(), 0..6usize).prop_map(|(c, t)| Op::Guide(c, t)),
        3 => (0..3usize, any::<bool>(), vec(any::<Index>(), 0..5))
            .prop_map(|(s, c, ts)| Op::Members(s, c, ts)),
        1 => select(vec![5u32, 15, 25, 35, 40, 45, 50]).prop_map(Op::TopicPrecedence),
        1 => any::<bool>().prop_map(Op::TopicRule),
        1 => any::<bool>().prop_map(Op::TopicNests),
        1 => (select(vec![5u32, 35, 40, 45, 50]), any::<bool>(), any::<bool>())
            .prop_map(|(p, n, i)| Op::Culture(p, n, i)),
    ]
}

fn of_kind(o: &Ontology, kind: &str) -> Vec<CategoryId> {
    o.categories()
        .filter(|c| c.kind() == kind)
        .map(|c| c.id.clone())
        .collect()
}

fn pick(ids: &[CategoryId], i: &Index) -> Option<CategoryId> {
    (!ids.is_empty()).then(|| i.get(ids).clone())
}

/// The next version of a row, changed by `f`.
fn changed(o: &Ontology, name: &str, f: impl FnOnce(&mut Kind)) -> Kind {
    let mut k = o.kind(name).unwrap().clone();
    k.version += 1;
    k.added_by = "eddie".into();
    f(&mut k);
    k
}

/// Apply one write; a refused one must leave the snapshot as it was.
fn apply(o: &mut Ontology, op: &Op, serial: &mut u64) {
    *serial += 1;
    let snowflake = |kind: &str, n: u64| id(&format!("{kind}:{}", 100_000_000_000_000_000 + n));
    let (record, by) = match op {
        Op::Guild(n) => (
            Record::Category(category(
                snowflake("guild", *serial).as_str(),
                NAMES[*n],
                None,
            )),
            Origin::Transport,
        ),
        Op::Channel(g, n) => {
            let parent = pick(&of_kind(o, "guild"), g);
            let mut c = category(snowflake("channel", *serial).as_str(), NAMES[*n], None);
            c.parent = parent;
            (Record::Category(c), Origin::Transport)
        }
        Op::Person(n) => (
            Record::Category(category(
                snowflake("person", *serial).as_str(),
                NAMES[*n],
                None,
            )),
            Origin::Transport,
        ),
        Op::Interp(culture, p, n) => {
            let kind = if *culture { "culture" } else { "topic" };
            let Ok(new) = o.mint_id(kind, NAMES[*n]) else {
                return;
            };
            let mut c = category(new.as_str(), NAMES[*n], None);
            c.parent = p.as_ref().and_then(|p| pick(&of_kind(o, kind), p));
            (Record::Category(c), Origin::Operator)
        }
        Op::Move(t, p) => {
            let topics = of_kind(o, "topic");
            let Some(t) = pick(&topics, t) else {
                return;
            };
            let mut c = o.category(&t).unwrap().clone();
            c.parent = p.as_ref().and_then(|p| pick(&topics, p));
            (Record::Category(c), Origin::Operator)
        }
        Op::Guide(c, t) => {
            let all: Vec<CategoryId> = o.categories().map(|c| c.id.clone()).collect();
            let Some(c) = pick(&all, c) else {
                return;
            };
            let version = o.guidance(&c).map_or(1, |g| g.version + 1);
            (
                Record::Guidance(Guidance::new(c, &text(*t), version, "eddie")),
                Origin::Operator,
            )
        }
        Op::Members(s, culture, picks) => {
            let kind = if *culture { "culture" } else { "topic" };
            let ids = of_kind(o, kind);
            let members = picks
                .iter()
                .filter_map(|i| pick(&ids, i))
                .map(|c| Membership::operator(c, *serial))
                .collect();
            (
                Record::Members(MemberList {
                    session: format!("s{s}"),
                    kind: kind.into(),
                    members,
                }),
                Origin::Operator,
            )
        }
        Op::TopicPrecedence(p) => (
            Record::Kind(changed(o, "topic", |k| k.precedence = *p)),
            Origin::Operator,
        ),
        Op::TopicRule(intent) => (
            Record::Kind(changed(o, "topic", |k| {
                k.rule = if *intent {
                    Rule::IntentLine
                } else {
                    Rule::Chain
                }
            })),
            Origin::Operator,
        ),
        Op::TopicNests(nests) => (
            Record::Kind(changed(o, "topic", |k| {
                k.parent = nests.then(|| "topic".to_string())
            })),
            Origin::Operator,
        ),
        Op::Culture(precedence, nests, intent) => {
            let row = Kind {
                name: "culture".into(),
                basis: Basis::Interpreted,
                assigned_by: vec![Origin::Operator],
                per_session: PerSession::AtMost(2),
                parent: nests.then(|| "culture".to_string()),
                precedence: *precedence,
                rule: if *intent {
                    Rule::IntentLine
                } else {
                    Rule::Chain
                },
                description: String::new(),
                version: o.kind("culture").map_or(1, |k| k.version + 1),
                added_by: "eddie".into(),
            };
            (Record::Kind(row), Origin::Operator)
        }
    };
    let before = o.clone();
    if o.put(record, by).is_err() {
        assert_eq!(*o, before, "a refused write changed the snapshot");
    }
}

fn snapshot() -> impl Strategy<Value = Ontology> {
    vec(op(), 0..48).prop_map(|ops| {
        let mut o = Ontology::seeded();
        let mut serial = 0;
        for op in &ops {
            apply(&mut o, op, &mut serial);
        }
        o
    })
}

/// A valid set of memberships for session `s`: at most one guild and one
/// channel, distinct people, and its interpreted lists.
fn memberships(o: &Ontology, s: usize, picks: &[Index]) -> Vec<Membership> {
    let mut out = Vec::new();
    let mut picks = picks.iter();
    for kind in ["guild", "channel"] {
        if let Some(c) = picks.next().and_then(|i| pick(&of_kind(o, kind), i)) {
            out.push(Membership::given(c, 7));
        }
    }
    let people = of_kind(o, "person");
    let chosen: BTreeSet<CategoryId> = picks.filter_map(|i| pick(&people, i)).collect();
    out.extend(chosen.into_iter().map(|c| Membership::given(c, 7)));
    out.extend(o.memberships(&format!("s{s}")));
    out
}

fn precedence(o: &Ontology, c: &CategoryId) -> u32 {
    o.kind(c.kind()).unwrap().precedence
}

/// The composition's own rules, checked against the snapshot.
fn well_formed(o: &Ontology, ms: &[Membership], c: &Composition) {
    assert_eq!(c.skipped, vec![], "a valid membership was skipped");
    assert_eq!(c.memberships.len(), ms.len());
    // The categories in play with guidance, each admitted once.
    let mut in_play = BTreeSet::new();
    for m in ms {
        match o.kind(m.kind()).unwrap().rule {
            Rule::Chain => in_play.extend(o.path(&m.category).into_iter().map(|c| c.id.clone())),
            _ => {
                in_play.insert(m.category.clone());
            }
        }
    }
    let with_guidance: BTreeSet<CategoryId> = in_play
        .into_iter()
        .filter(|c| o.guidance(c).is_some_and(|g| !g.is_empty()))
        .collect();
    let admitted: Vec<&CategoryId> = c.guidance.iter().map(|g| &g.category).collect();
    let set: BTreeSet<CategoryId> = admitted.iter().map(|c| (*c).clone()).collect();
    assert_eq!(set, with_guidance);
    assert_eq!(set.len(), admitted.len(), "a category was admitted twice");
    // Precedence, lowest first; an ancestor before its descendants.
    let order: Vec<u32> = admitted.iter().map(|c| precedence(o, c)).collect();
    assert!(order.windows(2).all(|w| w[0] <= w[1]), "{order:?}");
    let at: BTreeMap<&CategoryId, usize> =
        admitted.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    for (i, cat) in admitted.iter().enumerate() {
        for a in o.path(cat).iter().rev().skip(1) {
            if let Some(j) = at.get(&a.id) {
                assert!(*j < i, "{} comes after its descendant {cat}", a.id);
            }
        }
    }
    // The versions and digests are the guidance's.
    for g in &c.guidance {
        let held = o.guidance(&g.category).unwrap();
        assert_eq!((g.version, &g.digest), (held.version, &held.digest));
    }
    // The render: the preamble, a block per chain category, one per
    // intent-line kind.
    let chain = c
        .guidance
        .iter()
        .filter(|g| o.kind(g.category.kind()).unwrap().rule == Rule::Chain)
        .count();
    let intent: BTreeSet<&str> = c
        .guidance
        .iter()
        .map(|g| g.category.kind())
        .filter(|k| o.kind(k).unwrap().rule == Rule::IntentLine)
        .collect();
    if c.guidance.is_empty() {
        assert_eq!(c.sections, Vec::<String>::new());
    } else {
        assert_eq!(c.sections.len(), 1 + chain + intent.len());
        assert_eq!(c.sections[0], PREAMBLE);
        assert!(c.sections[1..]
            .iter()
            .all(|s| s.starts_with("# Guidance (")));
    }
    let render = c.render();
    assert_eq!(render, c.sections.join("\n\n"));
    for g in &c.guidance {
        assert!(render.contains(&o.guidance(&g.category).unwrap().text));
    }
}

/// A snapshot, and its records shuffled.
fn snapshot_and_records() -> impl Strategy<Value = (Ontology, Vec<Record>)> {
    snapshot().prop_flat_map(|o| {
        let records = o.records();
        (Just(o), Just(records).prop_shuffle())
    })
}

/// A snapshot, a valid set of memberships, and the same set shuffled.
fn snapshot_and_memberships() -> impl Strategy<Value = (Ontology, Vec<Membership>, Vec<Membership>)>
{
    (snapshot(), 0..3usize, vec(any::<Index>(), 0..6)).prop_flat_map(|(o, s, picks)| {
        let ms = memberships(&o, s, &picks);
        (Just(o), Just(ms.clone()), Just(ms).prop_shuffle())
    })
}

proptest! {
    #![proptest_config(cases(400))]

    #[test]
    fn a_snapshot_rebuilds_from_its_own_records_in_any_order(
        (o, records) in snapshot_and_records(),
    ) {
        let (back, dropped) = Ontology::load(records);
        prop_assert_eq!(dropped, vec![]);
        prop_assert_eq!(back, o);
    }

    #[test]
    fn a_valid_snapshot_always_composes_deterministically_in_any_order(
        (o, ms, shuffled) in snapshot_and_memberships(),
    ) {
        let c = o.compose(&ms);
        well_formed(&o, &ms, &c);
        prop_assert_eq!(&o.compose(&ms), &c);
        // Another order and another as-of: the same bytes and the same
        // memberships used.
        let other: Vec<Membership> = shuffled
            .into_iter()
            .map(|mut m| {
                m.as_of_ms += 1_000;
                m
            })
            .collect();
        let d = o.compose(&other);
        prop_assert_eq!(&d.sections, &c.sections);
        prop_assert_eq!(&d.guidance, &c.guidance);
        let cats = |c: &Composition| -> Vec<CategoryId> {
            c.memberships.iter().map(|m| m.category.clone()).collect()
        };
        prop_assert_eq!(cats(&d), cats(&c));
    }
}

/// Rows that are mostly valid but for their rule, which is any of the four:
/// a new `culture`, or `topic`'s next version, so that the rule and the
/// origins decide. A share of each other field is bad (a given basis, an
/// origin nothing writes, no count, a missing parent, a taken precedence, a
/// wrong version, a seed of the transport's, a bad name), to be refused.
fn any_row() -> impl Strategy<Value = Kind> {
    let name = prop_oneof![
        6 => Just("culture"),
        3 => Just("topic"),
        1 => select(vec!["guild", "person", "Bad", ""]),
    ];
    let given = prop_oneof![9 => Just(false), 1 => Just(true)];
    let origins = prop_oneof![
        5 => Just(vec![Origin::Operator]),
        3 => select(Origin::ALL.to_vec()).prop_map(|o| vec![o]),
        2 => vec(select(Origin::ALL.to_vec()), 0..3),
    ];
    let per = prop_oneof![
        8 => (1..4u32).prop_map(PerSession::AtMost),
        1 => Just(PerSession::Many),
        1 => Just(PerSession::AtMost(0)),
    ];
    // "self" is the row's own name: its categories nest.
    let parent = prop_oneof![
        6 => Just(None),
        2 => Just(Some("self")),
        1 => Just(Some("guild")),
        1 => Just(Some("none")),
    ];
    let precedence = prop_oneof![
        8 => select(vec![45u32, 50, 60]),
        2 => select(vec![5u32, 10, 20, 30, 40]),
    ];
    // `None`: the version the row's next write must have.
    let version = prop_oneof![8 => Just(None), 2 => (0..4u32).prop_map(Some)];
    (
        name,
        given,
        origins,
        per,
        parent,
        precedence,
        select(Rule::ALL.to_vec()),
        version,
    )
        .prop_map(
            |(name, given, assigned_by, per_session, parent, precedence, rule, version)| Kind {
                name: name.into(),
                basis: if given {
                    Basis::Given
                } else {
                    Basis::Interpreted
                },
                assigned_by,
                per_session,
                parent: parent.map(|p| if p == "self" { name } else { p }.to_string()),
                precedence,
                rule,
                description: String::new(),
                version: version.unwrap_or(if name == "topic" { 2 } else { 1 }),
                added_by: "eddie".into(),
            },
        )
}

/// The reader rule over 3,000 rows. It counts the rows the table accepts,
/// so a generator that drifts until nothing is accepted fails here instead
/// of passing on no evidence.
#[test]
fn the_table_accepts_only_rows_with_a_reader() {
    let accepted = std::cell::Cell::new(0u32);
    let mut runner = proptest::test_runner::TestRunner::new(cases(3000));
    let run = runner.run(&any_row(), |row| {
        let mut o = Ontology::seeded();
        if o.put(Record::Kind(row.clone()), Origin::Operator).is_ok() {
            accepted.set(accepted.get() + 1);
            // M4's built rules and origins, named here rather than asked of
            // the code, so one counted as built by mistake fails this test.
            prop_assert!(
                matches!(row.rule, Rule::Chain | Rule::IntentLine),
                "{:?}",
                row.rule
            );
            prop_assert!(row
                .assigned_by
                .iter()
                .all(|a| matches!(a, Origin::Transport | Origin::Operator)));
            prop_assert!(!row.assigned_by.is_empty());
            prop_assert_eq!(
                row.is_given(),
                theseus_ontology::GIVEN.contains(&row.name.as_str())
            );
            let (back, dropped) = Ontology::load(o.records());
            prop_assert_eq!(dropped, vec![]);
            prop_assert_eq!(back, o);
        } else {
            prop_assert_eq!(o, Ontology::seeded());
        }
        Ok(())
    });
    if let Err(e) = run {
        panic!("{e}");
    }
    let n = accepted.get();
    assert!(
        n >= 300,
        "only {n} of 3,000 rows were accepted: the property is not exercised"
    );
}

proptest! {
    #![proptest_config(cases(3000))]

    #[test]
    fn any_text_parses_as_a_category_id_or_is_refused(s in any::<String>()) {
        if let Ok(id) = CategoryId::parse(&s) {
            prop_assert_eq!(id.as_str(), s.as_str());
            prop_assert_eq!(format!("{}:{}", id.kind(), id.local()), s);
        }
    }
}
