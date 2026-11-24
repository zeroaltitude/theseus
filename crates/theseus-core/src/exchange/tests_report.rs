//! The measuring rows' report (issue 44, step 1): its three answers count his own messages' turns, a turn
//! whose first call was never measured is no miss, a turn naming two kinds is one turn that named something,
//! the context is averaged over measured turns and holds token parts only, `since` keeps the rows at or
//! after it, the report holds no place, exchange or task id, an old row still reads, and a field the
//! counter does not know is counted as present, never by its value. No stored string leaves it, by
//! value or by key: a word its producer does not make counts as `unlisted`. `since` reads every row,
//! however their times are ordered.

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

/// Every stored string the row accepts, and every map key, holds a marker; none may leave.
fn marked_row(ex: &Exchange, marker: &str) -> Row {
    let mut row = row_of(ex, Prev::Missed);
    row.place = format!("discord:channel:{marker}_PLACE");
    row.prev_exchange = Some(format!("turn_{marker}_PREV"));
    row.prev_why = Some(format!("{marker}_WHY"));
    row.situation = Some(format!("{marker}_SITUATION"));
    row.trigger = Some(format!("{marker}_TRIGGER"));
    row.elsewhere = vec![Elsewhere {
        place: format!("discord:dm:{marker}_ELSE"),
        exchange: format!("turn_{marker}_ELSE"),
        age_ms: 5,
        in_view: true,
    }];
    let (name, name2) = (format!("{marker}_NAME"), format!("{marker}_NAME2"));
    row.things = Some(map(&[(name.as_str(), 1), (name2.as_str(), 2), ("path", 1)]));
    let reply = format!("turn_{marker}_REPLY");
    row.reply_things = Some(map(&[(reply.as_str(), 1)]));
    row.judge = [
        ("kind".to_string(), format!("task_{marker}_KIND")),
        ("addressed_task".into(), format!("task_{marker}_TASK")),
        ("fragment".into(), format!("{marker}_FRAGMENT")),
        (
            "mentions_other_conversation".into(),
            format!("{marker}_OTHER"),
        ),
        ("wants_fresh_look".into(), format!("{marker}_FRESH")),
        (format!("{marker}_QUESTION"), format!("{marker}_ANSWER")),
    ]
    .into_iter()
    .collect();
    row
}

#[test]
fn no_stored_string_leaves_the_report_by_value_or_by_key() {
    let (_d, s) = store();
    // A row with words, a row with numeric ids, a row of nulls, a row of known words.
    let words = exchange_of("turn_PRIVATE_Q1", Opener::Message, Some(OWNER), 1_000);
    put(&s, &words, &marked_row(&words, "PRIVATE_Q"), 1_001);
    let ids = exchange_of("turn_2", Opener::Message, Some(OWNER), 2_000);
    put(&s, &ids, &marked_row(&ids, "112233445566778899"), 2_001);
    let nulls = exchange_of("turn_3", Opener::Message, Some(OWNER), 3_000);
    let mut row = row_of(&nulls, Prev::None);
    (row.things, row.reply_things) = (None, None);
    put(&s, &nulls, &row, 3_001);
    let known = exchange_of("turn_4", Opener::Message, Some(OWNER), 4_000);
    let mut row = row_of(&known, Prev::Missed);
    row.prev_why = Some(ledger::WHY_NEW.into());
    (row.situation, row.trigger) = (Some("continuation".into()), Some("view_turn".into()));
    row.things = Some(map(&[("path", 1), ("commit", 2)]));
    row.reply_things = Some(map(&[("file", 1)]));
    row.judge = [("kind", "follow_up"), ("addressed_task", "none")]
        .into_iter()
        .map(|(q, a)| (q.to_string(), a.to_string()))
        .collect();
    put(&s, &known, &row, 4_001);
    // Rows the typed row does not read, as before: a word where a class goes, a number where a word goes.
    let mut raw = serde_json::to_value(row_of(&known, Prev::None)).unwrap();
    raw["peer"] = json!("PRIVATE_Q_PEER");
    put_row(&s, "turn_4", raw.clone(), 4_002);
    raw["peer"] = Value::Null;
    raw["situation"] = json!(112_233_445_566_778_899u64);
    put_row(&s, "turn_4", raw, 4_003);
    let r = ledger::report(&s, &ExchangeReportParams::default()).unwrap();
    let text = serde_json::to_string(&r).unwrap();
    for marker in ["PRIVATE", "112233445566778899"] {
        assert!(
            !text.contains(marker),
            "{marker} left in the report: {text}"
        );
    }
    let u = ledger::UNLISTED;
    assert_eq!(r.rows, 4);
    assert_eq!(r.counts["prev_why"], map(&[(u, 2), ("new", 1)]));
    assert_eq!(
        r.counts["situation"],
        map(&[(u, 2), ("none", 1), ("continuation", 1)])
    );
    assert_eq!(
        r.counts["trigger"],
        map(&[(u, 2), ("none", 1), ("view_turn", 1)])
    );
    // A row naming two kinds nobody listed counts once under the one word.
    assert_eq!(
        r.counts["things"],
        map(&[(u, 2), ("path", 3), ("commit", 1), ("unknown", 1)])
    );
    assert_eq!(
        r.counts["reply_things"],
        map(&[(u, 2), ("file", 1), ("unknown", 1)])
    );
    assert_eq!(r.counts["judge.kind"], map(&[(u, 2), ("follow_up", 1)]));
    assert_eq!(
        r.counts["judge.addressed_task"],
        map(&[("a_task", 2), ("none", 1)])
    );
    for q in [
        "fragment",
        "mentions_other_conversation",
        "wants_fresh_look",
    ] {
        assert_eq!(r.counts[&format!("judge.{q}")], map(&[(u, 2)]), "{q}");
    }
    // An unknown question is counted present, never by its name or its answer.
    assert_eq!(r.counts["judge"], map(&[("present", 2), ("none", 1)]));
    assert_eq!(r.counts["elsewhere"], map(&[("in_view", 2)]));
    let a = r.answers.unwrap();
    assert_eq!(a.named_kinds, map(&[(u, 2), ("path", 3), ("commit", 1)]));
    assert_eq!(a.named, map(&[("some", 3), ("unknown", 1)]));
}

/// One test per producer: when it makes a word the report does not list, this fails, so the two cannot
/// drift apart without a test saying so. (The index's kinds are checked from `theseus-index`, which
/// can see both.)
#[test]
fn the_reports_words_are_the_words_their_producers_make() {
    use theseus_protocol::Situation;
    // Every situation, by a match that stops compiling when one is added.
    let every = [
        Situation::ConversationStart,
        Situation::TaskStart,
        Situation::Continuation,
        Situation::Recompile {
            trigger: String::new(),
        },
        Situation::Resume,
        Situation::Detour,
        Situation::Unknown,
    ];
    for s in &every {
        match s {
            Situation::ConversationStart
            | Situation::TaskStart
            | Situation::Continuation
            | Situation::Recompile { .. }
            | Situation::Resume
            | Situation::Detour
            | Situation::Unknown => {}
        }
    }
    let kinds: Vec<&str> = every.iter().map(Situation::kind).collect();
    assert_eq!(kinds, ledger::SITUATIONS);
    assert!(ledger::TRIGGERS.contains(&crate::compiler::view::TRIGGER));
    let pack = theseus_judge::pack::by_name(crate::judge::inbound::CLASSIFY_PACK).unwrap();
    let options = |q: &str| -> Vec<String> {
        let def = pack
            .question(q)
            .unwrap_or_else(|| panic!("{q} in the pack"));
        def.options.iter().map(|o| o.id.clone()).collect()
    };
    assert_eq!(options("kind"), ledger::JUDGE_KINDS);
    // A task's id is the dynamic part, counted only as whether there was one.
    assert_eq!(options("addressed_task"), ["none"]);
    for q in [
        "fragment",
        "mentions_other_conversation",
        "wants_fresh_look",
    ] {
        let def = pack.question(q).unwrap();
        assert_eq!(
            def.kind,
            theseus_judge::client::Kind::Noul,
            "{q} records no choice"
        );
    }
    for q in ledger::JUDGED {
        assert!(pack.question(q).is_some(), "{q} in the pack");
    }
}

/// Rows written in this order, at these times of their own.
fn rows_at(store: &Store, times: &[u64]) {
    for (i, at) in times.iter().enumerate() {
        let ex = exchange_of(&format!("turn_{i}"), Opener::Message, Some(OWNER), *at);
        put(store, &ex, &row_of(&ex, Prev::None), *at);
    }
}

fn since(since_ms: Option<u64>, limit: Option<u64>) -> ExchangeReportParams {
    ExchangeReportParams { limit, since_ms }
}

#[test]
fn since_reads_every_row_however_their_times_are_ordered() {
    let (_d, s) = store();
    rows_at(&s, &[3_000, 1_000, 4_000]);
    let r = ledger::report(&s, &since(Some(2_000), None)).unwrap();
    assert_eq!((r.rows, r.from_ms, r.to_ms), (2, Some(3_000), Some(4_000)));
    // Without `since`, the window is the least and the greatest time, not the first and last row.
    let r = ledger::report(&s, &since(None, None)).unwrap();
    assert_eq!((r.rows, r.from_ms, r.to_ms), (3, Some(1_000), Some(4_000)));
}

#[test]
fn since_reads_past_a_clock_that_went_back() {
    let (_d, s) = store();
    rows_at(&s, &[5_000, 6_000, 2_000, 7_000]);
    let r = ledger::report(&s, &since(Some(4_000), None)).unwrap();
    assert_eq!((r.rows, r.from_ms, r.to_ms), (3, Some(5_000), Some(7_000)));
}

#[test]
fn since_reads_past_a_page_of_older_rows() {
    let (_d, s) = store();
    // Ten new rows written first, then more than a page of older ones after them.
    for i in 0..10u64 {
        put_row(
            &s,
            &format!("turn_n{i}"),
            json!({"prev": "none"}),
            9_000 + i,
        );
    }
    for i in 0..600u64 {
        put_row(&s, &format!("turn_o{i}"), json!({"prev": "none"}), 1_000);
    }
    let r = ledger::report(&s, &since(Some(5_000), None)).unwrap();
    assert_eq!((r.rows, r.from_ms, r.to_ms), (10, Some(9_000), Some(9_009)));
}

#[test]
fn since_with_a_limit_keeps_the_newest_rows_that_qualify() {
    let (_d, s) = store();
    rows_at(&s, &[1_000, 5_000, 6_000, 2_000, 7_000]);
    let r = ledger::report(&s, &since(Some(4_000), Some(2))).unwrap();
    assert_eq!((r.rows, r.from_ms, r.to_ms), (2, Some(6_000), Some(7_000)));
}
