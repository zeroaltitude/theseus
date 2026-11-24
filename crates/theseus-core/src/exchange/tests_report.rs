//! The measuring rows' report (issue 44, step 1): its three answers count his own messages' turns, a turn
//! whose first call was never measured is no miss, a turn naming two kinds is one turn that named something,
//! the context is averaged over measured turns and holds token parts only, `since` keeps the rows at or
//! after it, the report holds no place, exchange or task id, an old row still reads, and a field the
//! counter does not know is counted as present, never by its value.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use theseus_protocol::exchange::{ExchangeAnswers, ExchangeReportParams, ExchangeReportResult};

use super::ledger::{self, Elsewhere, Prev, Row, Sent};
use super::{exchange_record, Exchange, Opener, OWNER};
use crate::ledger::LedgerRow;
use crate::store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

fn exchange_of(turn: &str, opened_by: Opener, person: Option<&str>, start_ms: u64) -> Exchange {
    Exchange {
        turn_id: turn.into(),
        session_id: "s".into(),
        place: "local".into(),
        private: true,
        opened_by,
        person: person.map(str::to_string),
        start_ms,
        end_ms: Some(start_ms + 1),
        ask: None,
        reply: None,
        messages: Vec::new(),
        stop: None,
        rebuilt: false,
    }
}

/// The exchange's record, then its row at `at`, as a turn's last frame writes it.
fn put(store: &Store, ex: &Exchange, row: &Row, at: u64) {
    store.append(&[exchange_record(ex).unwrap()]).unwrap();
    put_row(store, &ex.turn_id, serde_json::to_value(row).unwrap(), at);
}

fn put_row(store: &Store, turn: &str, data: Value, at: u64) {
    store
        .append_ledger(&LedgerRow {
            at_unix_ms: at,
            kind: "context.turn".into(),
            session_id: Some("s".into()),
            turn_id: Some(turn.into()),
            data,
        })
        .unwrap();
}

fn measured() -> Sent {
    Sent {
        header: 50,
        words: 10,
        total: 60,
        ..Sent::default()
    }
}

fn row_of(ex: &Exchange, prev: Prev) -> Row {
    Row {
        exchange: ex.turn_id.clone(),
        place: ex.place.clone(),
        opened_by: Some(ex.opened_by),
        prev,
        sent: measured(),
        things: Some(BTreeMap::new()),
        ..Row::default()
    }
}

fn answers(store: &Store, p: &ExchangeReportParams) -> (ExchangeReportResult, ExchangeAnswers) {
    let r = ledger::report(store, p).unwrap();
    let a = r.answers.clone().expect("the answers");
    (r, a)
}

fn map(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
    pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
}

#[test]
fn the_previous_answer_counts_his_own_message_turns_only() {
    let (_d, s) = store();
    let first = exchange_of("turn_1", Opener::Message, Some(OWNER), 1_000);
    put(&s, &first, &row_of(&first, Prev::None), 1_001);
    let shown = exchange_of("turn_2", Opener::Message, Some(OWNER), 2_000);
    put(&s, &shown, &row_of(&shown, Prev::Transcript), 2_001);
    let missed = exchange_of("turn_3", Opener::Message, Some(OWNER), 3_000);
    let mut row = row_of(&missed, Prev::Missed);
    (row.prev_exchange, row.prev_same_session) = (Some("turn_2".into()), Some(true));
    put(&s, &missed, &row, 3_001);
    // A wake, a guest's message and an agent's process at the terminal: none of them his.
    let wake = exchange_of("turn_4", Opener::Wake, None, 4_000);
    put(&s, &wake, &row_of(&wake, Prev::Missed), 4_001);
    let guest = exchange_of("turn_5", Opener::Message, Some("discord:7"), 5_000);
    put(&s, &guest, &row_of(&guest, Prev::Missed), 5_001);
    let agent = exchange_of("turn_6", Opener::Message, None, 6_000);
    let mut row = row_of(&agent, Prev::Missed);
    row.peer = Some(super::Peer::Agent);
    put(&s, &agent, &row, 6_001);
    let (r, a) = answers(&s, &ExchangeReportParams::default());
    assert_eq!(r.rows, 6);
    assert_eq!((a.his_turns, a.unreadable), (3, 0));
    assert_eq!(
        a.previous,
        map(&[("shown", 1), ("missed", 1), ("first_here", 1)])
    );
    assert_eq!(a.missed_by_session, map(&[("same", 1)]));
}

#[test]
fn a_turn_with_no_measured_call_is_not_a_miss() {
    let (_d, s) = store();
    let ex = exchange_of("turn_1", Opener::Message, Some(OWNER), 1_000);
    let mut row = row_of(&ex, Prev::Missed);
    row.sent = Sent::default();
    put(&s, &ex, &row, 1_001);
    let (_, a) = answers(&s, &ExchangeReportParams::default());
    assert_eq!(a.previous, map(&[("not_measured", 1)]));
    assert!(a.missed_by_session.is_empty());
}

#[test]
fn a_turn_naming_two_kinds_is_one_turn_that_named_something() {
    let (_d, s) = store();
    let things = [
        Some(map(&[("path", 2), ("commit", 1)])),
        Some(BTreeMap::new()),
        None,
    ];
    for (i, t) in things.into_iter().enumerate() {
        let at = 1_000 * (i as u64 + 1);
        let ex = exchange_of(&format!("turn_{i}"), Opener::Message, Some(OWNER), at);
        let mut row = row_of(&ex, Prev::None);
        row.things = t;
        put(&s, &ex, &row, at + 1);
    }
    let (_, a) = answers(&s, &ExchangeReportParams::default());
    assert_eq!(a.named, map(&[("some", 1), ("none", 1), ("unknown", 1)]));
    assert_eq!(a.named_kinds, map(&[("path", 1), ("commit", 1)]));
}

#[test]
fn the_context_is_averaged_over_measured_turns_and_holds_token_parts_only() {
    let (_d, s) = store();
    for (i, total) in [60, 60, 0].into_iter().enumerate() {
        let at = 1_000 * (i as u64 + 1);
        let ex = exchange_of(&format!("turn_{i}"), Opener::Wake, None, at);
        let mut row = row_of(&ex, Prev::None);
        if total == 0 {
            (row.sent, row.ask_chars) = (Sent::default(), Some(30));
        } else {
            (row.cache_read, row.cache_write, row.ask_chars) = (1_000, 200, Some(120));
        }
        put(&s, &ex, &row, at + 1);
    }
    let (r, a) = answers(&s, &ExchangeReportParams::default());
    assert_eq!(a.context_turns, 2);
    assert_eq!((a.context["header"], a.context["words"]), (100, 20));
    assert_eq!(a.context["total"], 120);
    assert_eq!((a.cache_read, a.cache_write), (2_000, 400));
    for k in ["ask_chars", "cache_read", "cache_write"] {
        assert!(!r.tokens.contains_key(k), "{k} is no token part: {r:?}");
    }
    assert_eq!(
        r.counts["ask_chars"],
        map(&[("under_80", 1), ("under_400", 2)])
    );
}

#[test]
fn since_keeps_only_rows_at_or_after_it() {
    let (_d, s) = store();
    for at in [1_000, 2_000, 3_000] {
        let ex = exchange_of(&format!("turn_{at}"), Opener::Message, Some(OWNER), at);
        put(&s, &ex, &row_of(&ex, Prev::None), at);
    }
    let p = ExchangeReportParams {
        since_ms: Some(2_000),
        ..ExchangeReportParams::default()
    };
    let (r, a) = answers(&s, &p);
    assert_eq!(r.rows, 2);
    assert_eq!((r.from_ms, r.to_ms), (Some(2_000), Some(3_000)));
    assert_eq!(a.his_turns, 2);
}

#[test]
fn the_report_holds_no_place_no_exchange_id_and_no_task_id() {
    let (_d, s) = store();
    let mut prev = exchange_of("turn_PREVQ80", Opener::Message, Some(OWNER), 1_000);
    prev.place = "discord:channel:PLACEQ7731".into();
    let mut ex = exchange_of("turn_EXQ81", Opener::Message, Some(OWNER), 2_000);
    ex.place = prev.place.clone();
    let mut row = row_of(&ex, Prev::Missed);
    (row.prev_exchange, row.prev_same_session) = (Some(prev.turn_id.clone()), Some(false));
    row.elsewhere = vec![Elsewhere {
        place: "discord:dm:ELSEQ55".into(),
        exchange: "turn_ELSEQ9".into(),
        age_ms: 5,
        in_view: false,
    }];
    row.judge = [("addressed_task", "task_TASKQ42"), ("kind", "follow_up")]
        .into_iter()
        .map(|(q, a)| (q.to_string(), a.to_string()))
        .collect();
    row.things = Some(map(&[("path", 1)]));
    s.append(&[exchange_record(&prev).unwrap()]).unwrap();
    put(&s, &ex, &row, 2_001);
    let r = ledger::report(&s, &ExchangeReportParams::default()).unwrap();
    let text = serde_json::to_string(&r).unwrap();
    for id in [
        "PLACEQ7731",
        "PREVQ80",
        "EXQ81",
        "ELSEQ55",
        "ELSEQ9",
        "TASKQ42",
    ] {
        assert!(!text.contains(id), "{id} left in the report: {text}");
    }
    assert_eq!(r.counts["judge.addressed_task"], map(&[("a_task", 1)]));
    assert_eq!(r.counts["judge.kind"], map(&[("follow_up", 1)]));
    assert_eq!(r.answers.unwrap().missed_by_session, map(&[("other", 1)]));
}

#[test]
fn an_old_row_without_the_new_key_still_reads_and_counts() {
    // A row as the build before `prev_why` wrote it, word for word in shape.
    let old = json!({
        "exchange": "turn_1", "place": "local", "opened_by": "message", "peer": "person", "person": true,
        "ask_chars": 12, "prev": "missed", "prev_exchange": "turn_0", "prev_same_session": true,
        "gap_ms": 5000, "overlap": false, "answered": null, "elsewhere": [],
        "sent": {"header": 50, "tools": 0, "files": 0, "replies": 0, "calls": 0, "words": 10,
                 "results_whole": 0, "results_cleared": 0, "recall": 0, "other": 0, "total": 60},
        "situation": null, "trigger": "view", "cold": false, "cache_read": 0, "cache_write": 0,
        "things": {}, "reply_things": null, "judge": {}
    });
    let row: Row = serde_json::from_value(old.clone()).expect("an old row decodes");
    assert_eq!((row.prev, row.prev_why.as_deref()), (Prev::Missed, None));
    // A key a newer build adds is ignored by this one, as this build's key is by an older one.
    let mut newer = old.clone();
    newer["a_later_key"] = json!("x");
    assert_eq!(serde_json::from_value::<Row>(newer).unwrap(), row);
    let (_d, s) = store();
    let ex = exchange_of("turn_1", Opener::Message, Some(OWNER), 2_000);
    s.append(&[exchange_record(&ex).unwrap()]).unwrap();
    put_row(&s, "turn_1", old, 2_001);
    let (r, a) = answers(&s, &ExchangeReportParams::default());
    assert_eq!((r.rows, r.counts["prev"]["missed"]), (1, 1));
    assert_eq!(a.previous, map(&[("missed", 1)]));
}

#[test]
fn a_field_the_counter_does_not_know_is_counted_present_never_by_value() {
    let mut fields = serde_json::to_value(Row {
        prev: Prev::Transcript,
        ..Row::default()
    })
    .unwrap();
    fields["later_name"] = json!("Zephyrine-Q-4471");
    fields["later_list"] = json!(["Zephyrine-Q-4472"]);
    fields["later_map"] = json!({"Zephyrine-Q-4473": "Zephyrine-Q-4474"});
    let mut r = ExchangeReportResult::default();
    ledger::add(&mut r, &fields);
    let text = serde_json::to_string(&r).unwrap();
    assert!(!text.contains("Zephyrine"), "{text}");
    assert_eq!(r.counts["later_name"], map(&[("present", 1)]));
    // The fields it knows keep their values.
    assert_eq!(r.counts["prev"], map(&[("transcript", 1)]));
}
