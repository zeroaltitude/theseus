//! The weekly digest's counting rule (theseus-1q2ho): a count of ledger rows
//! never stands in for work done. These tests pin the rule, not a number: a
//! week of budget bookkeeping reports no work, a gate that never started is
//! flagged and is no pass rate, an unjudged branch is not red, a capped read
//! is a floor, and the outcomes come before the bookkeeping, inside one post.

use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfLogRow, SelfMode, SelfState};

use super::{capped, digest_text, week_post, Outcomes, POST_CHARS};
use crate::ledger::LedgerRow;

const SINCE: u64 = 1_792_000_000_000;
const UNTIL: u64 = SINCE + 7 * 86_400_000;

fn state() -> SelfState {
    SelfState {
        mode: SelfMode::Act,
        ..SelfState::default()
    }
}

fn log_row(kind: &str, n: u64, numbers: Value) -> SelfLogRow {
    SelfLogRow {
        position: n,
        at_unix_ms: SINCE + n,
        kind: kind.into(),
        what: format!("{kind} {n}"),
        why: None,
        numbers,
        undo: None,
        session_id: None,
    }
}

/// `n` reserve/settle pairs and a rank each: the week the owner was told
/// "2000 changes" of.
fn bookkeeping(pairs: u64) -> Vec<SelfLogRow> {
    let mut rows = Vec::new();
    for i in 0..pairs {
        rows.push(log_row(
            "self.budget.reserved",
            3 * i,
            json!({"reserved_usd": 0.1}),
        ));
        rows.push(log_row(
            "self.budget.settled",
            3 * i + 1,
            json!({"spent_usd": 0.0}),
        ));
        rows.push(log_row("self.backlog.ranked", 3 * i + 2, Value::Null));
    }
    rows
}

fn built(gate: &str, exit: Value, secs: Value, n: u64) -> LedgerRow {
    let mut r = LedgerRow::named(
        "self.branch.built",
        None,
        None,
        json!({"gate": gate, "branch": format!("self/b{n}"), "item": format!("x{n}"),
               "numbers": {"exit": exit, "gate_secs": secs}}),
    );
    r.at_unix_ms = SINCE + n;
    r
}

fn infra(n: u64) -> LedgerRow {
    built("red", json!(127), json!(0), n)
}

fn week(rows: &[SelfLogRow], o: &Outcomes, capped: bool) -> String {
    digest_text(rows, o, capped, SINCE, UNTIL, &state())
}

#[test]
fn a_week_of_bookkeeping_reports_no_work() {
    let rows = bookkeeping(1_000);
    let o = Outcomes::of_rows(&[], SINCE, UNTIL);
    let t = week(&rows, &o, false);
    assert!(t.contains("- Joined: 0."), "{t}");
    assert!(t.contains("- Branches built: 0"), "{t}");
    assert!(t.contains("Judged: 0"), "{t}");
    assert!(
        !t.contains("changes"),
        "a count of ledger rows is never called changes: {t}"
    );
    assert!(t.contains("bookkeeping, not work done"), "{t}");
    // Nothing is named as a change: the list is of log entries.
    assert!(t.contains("Log entries, newest first:"), "{t}");
}

#[test]
fn gates_that_never_started_are_flagged_and_give_no_pass_rate() {
    let all = [infra(1), infra(2), infra(3)];
    let o = Outcomes::of_rows(&all, SINCE, UNTIL);
    assert_eq!((o.built, o.infrastructure, o.judged()), (3, 3, 0));
    let t = week(&bookkeeping(3), &o, false);
    assert!(t.contains("3 of 3 gates never started"), "{t}");
    assert!(t.contains("there is no pass rate"), "{t}");
    assert!(
        !t.contains("pass rate 0/0") && !t.contains("pass rate 0"),
        "no rate over zero judged: {t}"
    );
    assert!(!t.contains("red 3"), "never counted red: {t}");
}

#[test]
fn an_unjudged_branch_is_not_counted_red() {
    let all = [
        infra(1),
        built("red", json!(1), json!(300), 2),
        built("green", json!(0), json!(400), 3),
        built("halted", Value::Null, Value::Null, 4),
    ];
    let o = Outcomes::of_rows(&all, SINCE, UNTIL);
    assert_eq!(
        (o.built, o.green, o.red, o.infrastructure),
        (4, 1, 1, 1),
        "the halted attempt is built, and neither judged nor infrastructure"
    );
    let t = week(&[], &o, false);
    assert!(
        t.contains("Judged: 2 (green 1, red 1); pass rate 1/2"),
        "{t}"
    );
    assert!(
        t.contains("1 of 3 gates never started: unjudged, not counted red"),
        "{t}"
    );
}

#[test]
fn a_row_outside_the_week_is_not_this_weeks_outcome() {
    let mut old = built("green", json!(0), json!(9), 1);
    old.at_unix_ms = SINCE - 1;
    let mut j = LedgerRow::named("self.joined", None, None, json!({}));
    j.at_unix_ms = UNTIL + 1;
    assert_eq!(
        Outcomes::of_rows(&[old, j], SINCE, UNTIL),
        Outcomes::default()
    );
}

#[test]
fn a_capped_read_is_a_floor_and_never_exact() {
    let rows = bookkeeping(10);
    let o = Outcomes::default();
    let exact = week(&rows, &o, false);
    let floor = week(&rows, &o, true);
    assert!(
        exact.contains("bookkeeping, not work done): 30:"),
        "{exact}"
    );
    assert!(
        floor.contains("bookkeeping, not work done): at least 30:"),
        "{floor}"
    );
    assert!(floor.contains("Cost: at least $"), "{floor}");
    assert!(!exact.contains("at least"), "{exact}");
    // And the outcomes are not a function of the page: they say the same.
    let first = |t: &str| t.split("Ledger rows read").next().unwrap().to_string();
    assert!(!first(&floor)
        .replace("Cost: at least", "Cost:")
        .contains("at least"));
}

#[test]
fn outcomes_come_before_bookkeeping_and_fit_one_post() {
    let rows = bookkeeping(900);
    let all = [infra(1), infra(2)];
    let o = Outcomes::of_rows(&all, SINCE, UNTIL);
    let t = week(&rows, &o, true);
    let at = |s: &str| t.find(s).unwrap_or_else(|| panic!("{s} missing: {t}"));
    assert!(at("Outcomes this week") < at("Ledger rows read"));
    assert!(at("gates never started") < at("Ledger rows read"));
    assert!(at("- Joined:") < at("Log entries, newest first"));
    // The posted text keeps the outcomes, inside POST_CHARS.
    let post = week_post(&theseus_protocol::rsi::SelfDigestResult {
        since_ms: SINCE,
        until_ms: UNTIL,
        text: t.clone(),
        rows: rows.len() as u64,
    });
    assert!(post.chars().count() <= POST_CHARS + 60, "{}", post.len());
    assert!(post.contains("2 of 2 gates never started"), "{post}");
    assert!(post.contains("Joined: 0"), "{post}");
    assert_eq!(capped(&post), post, "already within one post");
}

#[test]
fn a_join_is_counted_from_its_own_row() {
    let mut j = LedgerRow::named("self.joined", None, None, json!({}));
    j.at_unix_ms = SINCE + 5;
    let o = Outcomes::of_rows(&[j], SINCE, UNTIL);
    assert_eq!(o.joined, 1);
    assert!(week(&[], &o, false).contains("- Joined: 1."));
}
