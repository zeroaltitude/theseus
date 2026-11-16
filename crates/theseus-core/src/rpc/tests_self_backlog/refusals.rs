//! The refusal alarm through the whole night: a tool's refusals are filed as
//! ONE item when they are many and unusual, and an ordinary night of them
//! files nothing. Every name here is invented to the test.
//!
//! The index dates a row by the frame's write, so a night is read in real
//! time. A night that files nothing writes nothing, not even the time it read
//! to, so it reads the day before its `now`: a night one day after `t0` reads
//! the rows written after `t0`, and its history is the rows written before it.

use super::*;

const EDIT: &str = "inproc:fs.edit";
const PATCH: &str = "inproc:fs.patch";
const CAUSE: &str = "refusal.rate:inproc:fs.edit";

/// `n` calls of `producer` that ended at `at`, the first `refused` of them
/// refused by the tool and the rest done.
fn calls(c: &Core, producer: &str, refused: u64, n: u64, at: u64) {
    for i in 0..n {
        let mut d = json!({"producer": producer, "duration_ms": 3,
                           "correlation_id": format!("act_{producer}_{at}_{i}")});
        if i < refused {
            d["failure"] = json!("refused");
            d["detail"] = json!({"error": "the text to replace is not in the file"});
            seed(c, "action.failed", d, at);
        } else {
            seed(c, "action.succeeded", d, at);
        }
    }
}

/// The refusal items on the backlog, by cause.
fn alarms(c: &Core) -> Vec<theseus_protocol::rsi::SelfBacklogItem> {
    listed(c)
        .into_iter()
        .filter(|i| i.cause.starts_with("refusal.rate:"))
        .collect()
}

/// A time later than every row written so far.
fn step() -> u64 {
    std::thread::sleep(std::time::Duration::from_millis(3));
    tick()
}

/// The nights before: each `(producer, refused, calls)` as rows dated three
/// days back. Returns the time after them, where the next night begins.
fn after_history(c: &Core, history: &[(&str, u64, u64)]) -> u64 {
    for (p, refused, n) in history {
        calls(c, p, *refused, *n, tick() - 3 * DAY);
    }
    step()
}

/// The night a day after `t0`, which reads the rows written since.
fn night_after(c: &Core, t0: u64) -> Night {
    step();
    c.backlog_night(t0 + DAY).unwrap()
}

use crate::rsi::backlog::Night;

#[test]
fn a_night_of_ordinary_refusals_files_nothing() {
    let c = acting(0, 20);
    // 12 of 100 calls refused in the nights before; 30 of 250 tonight: more
    // than twenty refusals, at the usual share.
    let t0 = after_history(&c, &[(EDIT, 12, 100)]);
    calls(&c, EDIT, 30, 250, step());
    let night = night_after(&c, t0);
    assert!(alarms(&c).is_empty(), "{:?}", listed(&c));
    assert!(night.filed.is_empty(), "{:?}", night.filed);
}

#[test]
fn a_night_where_one_producers_share_doubles_past_twenty_files_one_item() {
    let c = acting(0, 20);
    let t0 = after_history(&c, &[(EDIT, 12, 100)]);
    calls(&c, EDIT, 41, 60, step());
    let night = night_after(&c, t0);
    let got = alarms(&c);
    assert_eq!(got.len(), 1, "{:?}", listed(&c));
    assert_eq!(night.filed, vec![got[0].id.clone()]);
    assert_eq!(got[0].cause, CAUSE);
    assert_eq!(
        got[0].title,
        "fs.edit refused 41 of 60 calls last night; its usual share is 12%"
    );
    // The numbers, then a few of the refused rows.
    assert_eq!(got[0].evidence.len(), 6, "{:?}", got[0].evidence);
    assert!(
        got[0].evidence[0].starts_with("refusals:"),
        "{:?}",
        got[0].evidence
    );
    assert!(
        got[0].evidence[1..]
            .iter()
            .all(|e| e.starts_with("ledger:")),
        "{:?}",
        got[0].evidence
    );
}

/// Nights in real time here (a filed night keeps the time it read to): the
/// night after the filing reads only the rows written since, and its history
/// holds the filing night, so the item's own openness is proved by
/// `tests_refusal_rate`, which reads one night twice.
#[test]
fn the_next_night_with_the_same_spike_files_nothing_more() {
    let c = acting(0, 20);
    calls(&c, EDIT, 41, 60, step());
    let first = c.backlog_night(step()).unwrap();
    assert_eq!(first.filed.len(), 1, "{first:?}");
    let id = alarms(&c)[0].id.clone();
    calls(&c, EDIT, 59, 60, step());
    let night = c.backlog_night(step()).unwrap();
    assert!(
        night.filed.is_empty() && night.updated.is_empty(),
        "{night:?}"
    );
    let mut t = crate::task_graph::get(&c.store, &id).unwrap().unwrap();
    t.state = TaskState::Failed;
    t.version += 1;
    c.store
        .append(&[crate::task_graph::record(&t).unwrap()])
        .unwrap();
    calls(&c, EDIT, 59, 60, step());
    let night = c.backlog_night(step()).unwrap();
    assert!(night.filed.is_empty(), "{night:?}");
    assert_eq!(alarms(&c).len(), 1);
}

#[test]
fn a_producer_with_no_history_is_held_to_sixty_percent() {
    let c = acting(0, 20);
    let t0 = after_history(&c, &[]);
    // Both over twenty refusals; fs.edit's 30 of 100 is under 60%, and
    // fs.patch's 36 of 60 is at it.
    let at = step();
    calls(&c, EDIT, 30, 100, at);
    calls(&c, PATCH, 36, 60, at);
    night_after(&c, t0);
    let got = alarms(&c);
    assert_eq!(got.len(), 1, "{:?}", listed(&c));
    assert_eq!(got[0].cause, "refusal.rate:inproc:fs.patch");
    assert!(
        got[0].title.contains("no usual share yet"),
        "{}",
        got[0].title
    );
}

#[test]
fn another_producers_calls_neither_dilute_nor_trigger_it() {
    let c = acting(0, 20);
    let t0 = after_history(&c, &[(EDIT, 12, 100), (PATCH, 25, 200)]);
    // fs.edit's 41 of 60 stands against a thousand clean calls of another tool
    // (a dilution would put its share under twice 12%); fs.patch's 25 of 200
    // is at its own usual share, which fs.edit's spike does not move.
    let at = step();
    calls(&c, EDIT, 41, 60, at);
    calls(&c, "inproc:fs.read", 0, 1000, at);
    calls(&c, PATCH, 25, 200, at);
    night_after(&c, t0);
    let got = alarms(&c);
    assert_eq!(got.len(), 1, "{:?}", listed(&c));
    assert_eq!(got[0].cause, CAUSE);
}
