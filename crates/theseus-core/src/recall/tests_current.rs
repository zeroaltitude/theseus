//! Questions about how a value stands now (theseus-mo1vm): the reader, the
//! newest-first order, and the superseded mark.

use theseus_memory::recall::{Link, LinkKind};
use theseus_memory::{Candidate, Place};
use theseus_protocol::memory::RecallItem;

use super::current;
use super::temporal::{self, Kind};

fn kind(q: &str) -> Option<Kind> {
    temporal::read(q).map(|a| a.kind)
}

/// The exam's eight "now" shapes, in invented wording, read as `Current`;
/// what asks for no value, or for an event, reads as it did.
#[test]
fn a_value_asked_about_now_reads_as_current() {
    for q in [
        "How many days of audit logs does example.test keep now?",
        "What time is the weekly reef sync now?",
        "At what daily spend does the example.test billing alarm fire now?",
        "What's the relay's timeout now, in seconds?",
        "How many nightly archives do we keep now?",
        "Which port is the example.test collector on now?",
        "What does the lighthouse bot's status line say now?",
        "Where do the scratch workers keep their state now?",
        "How much memory does the example.test indexer get currently?",
    ] {
        assert_eq!(kind(q), Some(Kind::Current), "{q}");
    }
}

#[test]
fn a_now_that_asks_for_no_value_or_for_an_event_reads_as_before() {
    for q in [
        "What are we doing now?",
        "What am I working on now?",
        "What day is it now?",
        "What time does the nightly backup run now?",
        "When did we pick it, and what is it now?",
        "When did we first set the example.test quota, now that it changed?",
        "Can you restart the relay now?",
        "How many days of audit logs do we keep?",
    ] {
        assert_eq!(kind(q), None, "{q}");
    }
    assert_eq!(
        kind("When did we first talk about the tide tables?"),
        Some(Kind::Earliest)
    );
}

/// `still` asks about a value only with a number or an ask in it.
#[test]
fn still_reads_as_current_only_when_it_asks_about_a_value() {
    assert_eq!(
        kind("Do we still keep 30 days of logs?"),
        Some(Kind::Current)
    );
    assert_eq!(
        kind("Is the example.test relay still on port 9200?"),
        Some(Kind::Current)
    );
    assert_eq!(
        kind("What do we still keep on the example.test volume?"),
        Some(Kind::Current)
    );
    // The false-positive guards: no value in the question.
    assert_eq!(kind("Do we still use the old runner?"), None);
    assert_eq!(kind("Is the cache still enabled?"), None);
    assert_eq!(kind("Why is the build still slow?"), None);
}

#[test]
fn the_subject_leaves_out_the_now_word() {
    assert_eq!(
        temporal::subject("How many nightly archives do we keep now?"),
        "How many nightly archives do we keep ?"
    );
}

const DAY: u64 = 86_400_000;
const T0: u64 = 1_773_000_000_000;

fn cand(id: &str, rank: usize, time_ms: u64, text: &str) -> Candidate {
    Candidate {
        node_id: id.into(),
        chunk: 0,
        session_id: format!("ses_{id}"),
        position: rank as u64,
        kind: "user_message".into(),
        origin: "operator".into(),
        external: false,
        text: text.into(),
        fused: 1.0 / (60.0 + rank as f64),
        index_rank: rank,
        place: Place::Private,
        time_ms,
        entities: Vec::new(),
        matched: Vec::new(),
    }
}

const Q: &str = "How many days of audit logs does example.test keep now?";

/// The oldest value is repeated most, as the exam's chatter does, and comes
/// first in relevance; another subject's number sits among them.
fn notes() -> Vec<Candidate> {
    vec![
        cand("a1", 1, T0, "example.test keeps 14 days of audit logs."),
        cand(
            "a2",
            2,
            T0 + DAY,
            "Reminder: 14 days of audit logs on example.test.",
        ),
        cand(
            "a3",
            3,
            T0 + 2 * DAY,
            "example.test audit logs are pruned after 14 days.",
        ),
        cand(
            "other",
            4,
            T0 + 90 * DAY,
            "The example.test relay has 7 workers for audit tasks.",
        ),
        cand(
            "b1",
            5,
            T0 + 30 * DAY,
            "Raised example.test audit logs to 30 days.",
        ),
        cand(
            "c1",
            6,
            T0 + 60 * DAY,
            "example.test audit logs: keep 21 days now.",
        ),
        cand("prose", 7, T0 + 70 * DAY, "Audit logs are boring."),
    ]
}

fn ids(order: &[String]) -> Vec<&str> {
    order
        .iter()
        .map(|k| k.split('#').next().unwrap_or(""))
        .collect()
}

#[test]
fn the_newest_statement_of_the_subject_ranks_first_and_others_follow() {
    let order = current::order(Q, &[], &notes());
    assert_eq!(
        ids(&order),
        ["c1", "b1", "a3", "a2", "a1", "other", "prose"]
    );
}

/// A `supersedes` edge between two statements wins over their times.
#[test]
fn a_supersession_edge_wins_over_time() {
    // The edges say a1 replaced every later statement.
    let links = ["a2", "a3", "b1", "c1"].map(|older| Link {
        kind: LinkKind::Supersedes,
        newer: "a1".into(),
        older: older.into(),
    });
    let order = current::order(Q, &links, &notes());
    let pos = |id: &str| ids(&order).iter().position(|x| *x == id).unwrap();
    assert!(pos("a1") < pos("c1"), "{order:?}");
    // The edge's newest still leads the time order of the rest.
    assert_eq!(ids(&order)[0], "a1");
    let marks = current::marks(Q, &links, &notes());
    assert!(!marks.contains_key("a1#0"));
    assert!(marks.contains_key("c1#0"));
}

#[test]
fn older_values_are_marked_and_the_newest_is_not() {
    let marks = current::marks(Q, &[], &notes());
    let mut got: Vec<&str> = marks.keys().map(String::as_str).collect();
    got.sort_unstable();
    assert_eq!(got, ["a1#0", "a2#0", "a3#0", "b1#0"]);
    let day = super::render::utc_day(T0 + 60 * DAY);
    assert_eq!(marks["a1#0"], day);
    // Another subject's number and a note with no value are never marked.
    assert!(!marks.contains_key("other#0") && !marks.contains_key("prose#0"));
}

/// A pack with no mark renders the header's bytes as they were; a marked
/// one says it, once, at the header's end.
#[test]
fn the_mark_rides_the_header_and_an_unmarked_one_is_untouched() {
    let header = "a message in #harbor, said Mon 2026-03-02 09:10 MST (as of @7)".to_string();
    let plain = RecallItem::default();
    assert_eq!(current::with_mark(header.clone(), &plain), header);
    let mut old = RecallItem::default();
    let mut n = notes();
    n.truncate(1);
    n[0].node_id = String::new();
    let day = super::render::utc_day(T0);
    // Alone, a statement is its own newest; with a later one it is marked.
    n.push(cand(
        "z",
        2,
        T0 + DAY,
        "example.test keeps 30 days of audit logs.",
    ));
    current::mark_for(
        std::slice::from_mut(&mut old),
        Some(Kind::Current),
        Q,
        &[],
        &n,
    );
    assert_eq!(
        old.superseded.as_deref(),
        Some(super::render::utc_day(T0 + DAY).as_str())
    );
    old.superseded = Some(day);
    let mut other = RecallItem::default();
    current::mark_for(
        std::slice::from_mut(&mut other),
        Some(Kind::Latest),
        Q,
        &[],
        &n,
    );
    assert_eq!(other.superseded, None);
    assert_eq!(
        current::with_mark(header.clone(), &old),
        format!(
            "{header}, superseded: older than the {} statement",
            super::render::utc_day(T0)
        )
    );
}

/// The probe's order for a current question is the ranker's.
#[test]
fn the_probe_orders_a_current_question_as_recall_does() {
    let n = notes();
    assert_eq!(
        current::order_of(Kind::Current, Q, &n),
        current::order(Q, &[], &n)
    );
    assert_eq!(
        current::order_of(Kind::Latest, Q, &n),
        temporal::order(Kind::Latest, &n)
    );
}
