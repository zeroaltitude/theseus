//! A snapshot that reads its membership lists from a store holds none of
//! them, and answers every question as the one that holds them all does
//! (theseus-mem-daemon-rest).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use super::*;

const OP: Origin = Origin::Operator;
const IMPORT: Origin = Origin::Import;

fn id(s: &str) -> CategoryId {
    CategoryId::parse(s).unwrap()
}

fn entry(i: &str, origin: Origin, as_of_ms: u64) -> Membership {
    Membership {
        category: id(i),
        origin,
        confidence: None,
        as_of_ms,
    }
}

fn list(session: &str, kind: &str, members: Vec<Membership>) -> MemberList {
    MemberList {
        session: session.into(),
        kind: kind.into(),
        members,
    }
}

/// A store of lists in memory, as the core's is on disk.
#[derive(Debug, Default)]
struct Mem(Mutex<BTreeMap<(String, String), MemberList>>);

impl Mem {
    fn set(&self, l: &MemberList) {
        let key = (l.session.clone(), l.kind.clone());
        self.0.lock().unwrap().insert(key, l.clone());
    }
}

impl MemberSource for Mem {
    fn list(&self, session: &str, kind: &str) -> Option<MemberList> {
        let key = (session.to_string(), kind.to_string());
        self.0.lock().unwrap().get(&key).cloned()
    }
    fn of_session(&self, session: &str) -> Vec<MemberList> {
        let all = self.0.lock().unwrap();
        all.iter()
            .filter(|((s, _), _)| s == session)
            .map(|(_, l)| l.clone())
            .collect()
    }
    fn all(&self) -> Vec<MemberList> {
        self.0.lock().unwrap().values().cloned().collect()
    }
}

/// The categories and the lists a store would hold, as records.
fn records(lists: &[MemberList]) -> Vec<Record> {
    let mut out: Vec<Record> = ["a", "b", "c", "d"]
        .iter()
        .map(|n| Record::Category(Category::new(id(&format!("topic:{n}")), *n, "operator")))
        .collect();
    out.extend(lists.iter().cloned().map(Record::Members));
    out
}

/// Both snapshots from the same records: the one that holds the lists, and
/// the one that reads them from `mem`.
fn both(lists: &[MemberList]) -> (Ontology, Ontology, Arc<Mem>) {
    let mem = Arc::new(Mem::default());
    lists.iter().for_each(|l| mem.set(l));
    let recs = records(lists);
    let held = Ontology::load(recs.clone()).0;
    let lean = Ontology::load_from(recs, mem.clone()).0;
    (held, lean, mem)
}

/// Every question a reader asks, asked of both.
fn same(held: &Ontology, lean: &Ontology, sessions: &[&str]) {
    let owned = |o: &Ontology| o.lists().map(|l| l.into_owned()).collect::<Vec<_>>();
    assert_eq!(owned(held), owned(lean), "lists");
    assert_eq!(held.member_counts(), lean.member_counts(), "counts");
    assert_eq!(held.last_held(), lean.last_held(), "last held");
    assert_eq!(held.records(), lean.records(), "records");
    assert_eq!(held.merge_pairs(), lean.merge_pairs(), "merge pairs");
    for s in sessions {
        assert_eq!(
            held.memberships(s),
            lean.memberships(s),
            "memberships of {s}"
        );
        for kind in ["topic", "person", "event"] {
            assert_eq!(
                held.member_list(s, kind),
                lean.member_list(s, kind),
                "{s} {kind}"
            );
        }
    }
    assert_eq!(held, lean);
}

fn to_lists() -> Vec<MemberList> {
    vec![
        list(
            "s1",
            "topic",
            vec![entry("topic:a", OP, 10), entry("topic:b", IMPORT, 30)],
        ),
        list("s2", "topic", vec![entry("topic:a", IMPORT, 20)]),
        list("s3", "topic", vec![entry("topic:b", OP, 5)]),
        list("s4", "topic", vec![]),
    ]
}

/// The lean snapshot holds no list after its load, and answers as the held
/// one does after the load and after each write the core makes: put, then
/// the store has it, then settle.
#[test]
fn a_lean_snapshot_holds_no_list_and_answers_as_a_held_one_does() {
    let (mut held, mut lean, mem) = both(&to_lists());
    assert_eq!(lean.held_lists(), 0);
    assert_eq!(held.held_lists(), 4);
    let sessions = ["s1", "s2", "s3", "s4", "s9"];
    same(&held, &lean, &sessions);

    let steps = [
        // The one that held the newest of `topic:b` is dropped.
        list("s1", "topic", vec![entry("topic:a", OP, 10)]),
        // A new session, with the newest of everything.
        list(
            "s9",
            "topic",
            vec![entry("topic:b", OP, 99), entry("topic:c", OP, 98)],
        ),
        // An empty list is a list.
        list("s2", "topic", vec![]),
        // Back to what it was.
        list(
            "s1",
            "topic",
            vec![entry("topic:a", OP, 10), entry("topic:b", IMPORT, 30)],
        ),
    ];
    for l in steps {
        held.put(Record::Members(l.clone()), OP).unwrap();
        lean.put(Record::Members(l.clone()), OP).unwrap();
        // Staged lists answer before the store has them.
        assert_eq!(lean.member_list(&l.session, &l.kind).as_deref(), Some(&l));
        mem.set(&l);
        lean.settle();
        assert_eq!(lean.held_lists(), 0, "settled");
        same(&held, &lean, &sessions);
    }
}

/// A category some session holds is not retired, in either; once nothing
/// holds it, both retire it.
#[test]
fn what_holds_a_category_is_counted_without_the_lists() {
    let (held, lean, _) = both(&to_lists());
    let retire = |o: &Ontology, c: &str| {
        let mut gone = o.category(&id(c)).unwrap().clone();
        gone.retired_ms = Some(1);
        o.check(&Record::Category(gone), OP)
    };
    let (a, b) = (retire(&held, "topic:a"), retire(&lean, "topic:a"));
    assert!(matches!(a, Err(Refusal::InUse { .. })), "{a:?}");
    assert_eq!(a, b);
    assert_eq!(retire(&held, "topic:d"), Ok(()));
    assert_eq!(retire(&lean, "topic:d"), Ok(()));
}

/// What the load trimmed or dropped stays trimmed or dropped: an entry whose
/// category is unknown, a list of a kind that is not held.
#[test]
fn what_the_load_left_out_is_left_out_of_the_lean_snapshot_too() {
    let lists = vec![
        list(
            "s1",
            "topic",
            vec![entry("topic:a", OP, 10), entry("topic:gone", OP, 11)],
        ),
        list("s2", "topic", vec![entry("topic:gone", OP, 12)]),
        list("s3", "nonesuch", vec![entry("topic:a", OP, 13)]),
        list("s4", "topic", vec![entry("topic:b", OP, 14)]),
    ];
    let (held, lean, _) = both(&lists);
    assert_eq!(lean.held_lists(), 2, "the two the load trimmed");
    same(&held, &lean, &["s1", "s2", "s3", "s4"]);
    assert!(lean.member_list("s3", "nonesuch").is_none());
    let s1 = lean.member_list("s1", "topic").unwrap().into_owned();
    assert_eq!(s1.members, vec![entry("topic:a", OP, 10)]);
}

/// A write that puts two lists of one session sees the first when it checks
/// the second, as it does in a held snapshot.
#[test]
fn a_write_sees_what_the_writes_before_it_put() {
    let (mut held, mut lean, _) = both(&to_lists());
    let first = list(
        "s3",
        "topic",
        vec![entry("topic:b", OP, 5), entry("topic:c", OP, 6)],
    );
    let second = list(
        "s3",
        "topic",
        vec![
            entry("topic:b", OP, 5),
            entry("topic:c", OP, 6),
            entry("topic:d", OP, 7),
        ],
    );
    for o in [&mut held, &mut lean] {
        o.put(Record::Members(first.clone()), OP).unwrap();
        o.put(Record::Members(second.clone()), OP).unwrap();
    }
    assert_eq!(lean.member_list("s3", "topic").as_deref(), Some(&second));
    assert_eq!(held.member_counts(), lean.member_counts());
}
