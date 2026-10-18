//! Questions about time (theseus-q1tbn): the reader's table, the subject,
//! the order, and the header's date, in the owner's zone.

use std::sync::Arc;

use theseus_memory::{Candidate, Place};

use super::render::{self, header_in, Sources};
use super::stamp::{self, Zone};
use super::temporal::{self, Kind};
use crate::node::{Node, RecalledRef};

fn phoenix(_: u64) -> Zone {
    Zone {
        offset_secs: -7 * 3600,
        name: "MST".into(),
    }
}

fn kind(q: &str) -> Option<Kind> {
    temporal::read(q).map(|a| a.kind)
}

/// Every phrase the reader knows, and the questions that only have a time
/// for their subject, which it leaves alone.
#[test]
fn the_reader_names_the_kind_of_a_question_about_time() {
    let table = [
        ("When did we switch the cache TTL?", Some(Kind::When)),
        ("when was the staging box rebuilt", Some(Kind::When)),
        ("When is the review of the ferry rota?", Some(Kind::When)),
        ("What day did the plover dashboard move?", Some(Kind::When)),
        (
            "what date was the tunnel certificate renewed",
            Some(Kind::When),
        ),
        (
            "At what time did the backup run on the 3rd?",
            Some(Kind::When),
        ),
        (
            "What day of the week was the harbor drill?",
            Some(Kind::When),
        ),
        (
            "How long ago did the weir gauge break?",
            Some(Kind::Duration),
        ),
        ("How long since we rotated the keys?", Some(Kind::Duration)),
        (
            "How many days between the outage and the fix?",
            Some(Kind::Duration),
        ),
        (
            "What was the first time the build broke?",
            Some(Kind::Earliest),
        ),
        (
            "When did we first talk about the tide tables?",
            Some(Kind::Earliest),
        ),
        (
            "the earliest mention of the pushgateway port",
            Some(Kind::Earliest),
        ),
        (
            "What was the last time we restarted the relay?",
            Some(Kind::Latest),
        ),
        ("When did we last rotate the keys?", Some(Kind::Latest)),
        (
            "What is the most recent change to the gate timeout?",
            Some(Kind::Latest),
        ),
        (
            "the latest decision about the ferry rota",
            Some(Kind::Latest),
        ),
        (
            "Which came first, the cache change or the outage?",
            Some(Kind::Order),
        ),
        (
            "Did we pick the cache TTL before the outage?",
            Some(Kind::Order),
        ),
        ("What did we do after the migration?", Some(Kind::Order)),
        // A time that is only the subject.
        ("What time zone do I use?", None),
        ("What date format does the log print?", None),
        ("What time does the nightly backup run now?", None),
        ("What time does the backup run?", None),
        (
            "When does the kestrel tower start its nightly backup?",
            None,
        ),
        ("When did we pick it, and what is it now?", None),
        ("What day is it?", None),
        ("what time is it", None),
        ("When I deploy, which port does it use?", None),
        ("What is the latest version of the ferry app?", None),
        ("How long does the build take?", None),
        ("How many days of WAL segments do we keep?", None),
        // A span is `when`'s, not an order.
        ("What did we do before March?", None),
        ("What happened after 2026-03-14?", None),
        ("The deploy runs before the daemon starts.", None),
    ];
    for (q, want) in table {
        assert_eq!(kind(q), want, "{q}");
    }
}

/// The index searches on the event: the question without its temporal words.
#[test]
fn the_subject_is_the_question_without_its_temporal_words() {
    assert_eq!(
        temporal::subject("When did we switch the cache TTL?"),
        "we switch the cache TTL?"
    );
    assert_eq!(
        temporal::subject("What was the first time the build broke?"),
        "What was the build broke?"
    );
    assert_eq!(
        temporal::subject("How long since we rotated the keys?"),
        "we rotated the keys?"
    );
    // Not a question about time: untouched.
    let plain = "What time zone do I use?";
    assert_eq!(temporal::subject(plain), plain);
    // Too little would be left to search on: untouched.
    assert_eq!(temporal::subject("When was it?"), "When was it?");
}

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

fn ids(order: &[String]) -> Vec<&str> {
    order
        .iter()
        .map(|k| k.split('#').next().unwrap_or(""))
        .collect()
}

const DAY: u64 = 86_400_000;
const T0: u64 = 1_773_000_000_000; // 2026-03-08

/// The event's note goes above three later notes that mention it, though
/// they match the subject better; and a plan from before it, which only
/// mentions it, goes below.
fn cache_notes() -> Vec<Candidate> {
    vec![
        cand("m1", 1, T0 + 40 * DAY, "The cache TTL is fine, thanks."),
        cand(
            "m2",
            2,
            T0 + 41 * DAY,
            "Remember the cache TTL when you size the box.",
        ),
        cand("m3", 3, T0 + 45 * DAY, "Cache TTL: still the new value."),
        cand(
            "plan",
            4,
            T0 + DAY,
            "Should we look at the cache TTL some time?",
        ),
        cand(
            "event",
            5,
            T0 + 10 * DAY,
            "We switched the cache TTL from 60 s to 5 minutes.",
        ),
    ]
}

#[test]
fn a_temporal_question_ranks_the_event_above_later_notes_that_mention_it() {
    let notes = cache_notes();
    let relevance: Vec<&str> = vec!["m1", "m2", "m3", "plan", "event"];
    assert_eq!(
        notes.iter().map(|c| c.node_id.as_str()).collect::<Vec<_>>(),
        relevance
    );
    let when = temporal::order(Kind::When, &notes);
    assert_eq!(ids(&when)[0], "event", "{when:?}");
    // The mentions follow, oldest first.
    assert_eq!(ids(&when), ["event", "plan", "m1", "m2", "m3"]);
    // The latest mention of a thing, among the notes that state it.
    let mut again = cache_notes();
    again.push(cand(
        "event2",
        6,
        T0 + 30 * DAY,
        "We changed the cache TTL back to 60 s.",
    ));
    let latest = temporal::order(Kind::Latest, &again);
    assert_eq!(ids(&latest)[..2], ["event2", "event"]);
    let earliest = temporal::order(Kind::Earliest, &again);
    assert_eq!(ids(&earliest)[..2], ["event", "event2"]);
    // Both ends for an order or a duration.
    let ends = temporal::order(Kind::Duration, &again);
    assert_eq!(ids(&ends)[..2], ["event", "event2"]);
}

/// A question that is not about time keeps the index's order: the pack
/// takes the same notes, in the same order, as it did.
#[test]
fn a_question_that_is_not_about_time_is_not_reordered() {
    assert_eq!(kind("What is the cache TTL?"), None);
    let q = "What is the cache TTL?";
    assert_eq!(temporal::subject(q), q);
    assert!(temporal::of_turn(&Default::default(), "turn_1").is_none());
}

/// The header says when the note was said in the owner's zone, not UTC: an
/// evening message is that evening's day, where UTC read the next.
#[test]
fn an_evening_note_is_dated_in_the_owners_zone() {
    // 2026-03-18 02:40 UTC is Tuesday the 17th, 19:40 in Phoenix.
    let at = 1_773_801_600_000;
    let mut n = Node::user("ses_weir", None, "cli", "The ferry rota is settled.");
    n.created_at_ms = at;
    let header = header_in(&n, 42, "#harbor", "The ferry rota is settled.", &phoenix);
    assert_eq!(
        header,
        "a message from cli in #harbor, said Tue 2026-03-17 19:40 MST (as of @42)"
    );
    assert!(!header.contains("03-18"), "{header}");
}

/// A relative time in the text is resolved against the note's own day.
#[test]
fn yesterday_is_resolved_against_the_notes_own_day() {
    let at = 1_773_801_600_000;
    let mut n = Node::user("ses_weir", None, "cli", "We moved the rota yesterday.");
    n.created_at_ms = at;
    let header = header_in(&n, 7, "#harbor", "We moved the rota yesterday.", &phoenix);
    assert_eq!(
        header,
        "a message from cli in #harbor, said Tue 2026-03-17 19:40 MST; \
         \"yesterday\" = Mon 2026-03-16 (as of @7)"
    );
    // The clock does not enter: the same note, built again, is the same bytes.
    assert_eq!(
        header,
        header_in(&n, 7, "#harbor", "We moved the rota yesterday.", &phoenix)
    );
    // A whole date needs no resolving; a week is a range.
    let whole = header_in(&n, 7, "#harbor", "Decided on 2026-03-02.", &phoenix);
    assert!(!whole.contains(" = "), "{whole}");
    let week = header_in(&n, 7, "#harbor", "It broke last week.", &phoenix);
    assert!(
        week.contains("\"last week\" = Mon 2026-03-09 to Sun 2026-03-15"),
        "{week}"
    );
}

/// A header is frozen when the item is recalled: the render prints its
/// stored bytes, whatever the zone or the clock is when it is rendered.
#[test]
fn a_frozen_header_renders_its_stored_bytes() {
    let mut n = Node::user("ses_weir", None, "cli", "We moved the rota yesterday.");
    n.id = "msg_rota".into();
    n.created_at_ms = 1_773_801_600_000;
    let stored = header_in(&n, 7, "#harbor", "We moved the rota yesterday.", &phoenix);
    let item = RecalledRef {
        node_id: "msg_rota".into(),
        session_id: "ses_weir".into(),
        position: 7,
        chunk: (0, 28),
        header: stored.clone(),
        tokens: 8,
    };
    let sources: Sources = [("msg_rota".to_string(), Arc::new(n))]
        .into_iter()
        .collect();
    let first = render::render(std::slice::from_ref(&item), &sources);
    assert!(
        first.contains(&format!("\n(1) {stored}\n    \"We moved")),
        "{first}"
    );
    assert_eq!(first, render::render(&[item], &sources));
    assert_eq!(
        stamp::said_in(1_773_801_600_000, &phoenix),
        "Tue 2026-03-17 19:40 MST"
    );
}

/// A note states the event when it carries a time or says something
/// changed; a note that only comes back to the subject does neither.
#[test]
fn a_note_that_says_a_change_or_a_time_states_the_event() {
    let at = T0 + 20 * DAY;
    let states = |text: &str| temporal::states_event(&cand("n", 1, at, text));
    assert!(states("We switched the relay to the new rack."));
    assert!(states("Cut the log retention to 21 days."));
    assert!(states("The relay broke last week."));
    assert!(states("Use the east rack from now on."));
    assert!(!states("The relay is on the east rack."));
    assert!(!states("Did anyone look at the relay?"));
}

#[test]
fn a_span_is_the_wires_own_type_and_serialises_as_it_always_did() {
    use super::when::When;
    let both = When {
        from_ms: Some(1_000),
        to_ms: Some(2_000),
        said: "march".into(),
    };
    assert_eq!(
        serde_json::to_string(&both).unwrap(),
        r#"{"said":"march","from_ms":1000,"to_ms":2000}"#
    );
    let open = When {
        from_ms: None,
        to_ms: Some(2_000),
        said: "before march".into(),
    };
    assert_eq!(
        serde_json::to_string(&open).unwrap(),
        r#"{"said":"before march","to_ms":2000}"#
    );
    assert!(both.holds(1_000) && both.holds(1_999) && !both.holds(2_000) && !both.holds(999));
    assert!(open.holds(0) && !open.holds(2_000));
}
