//! The self budget's account (theseus-pw1q.3): a branch stops at its line
//! with its work kept, the day stops across branches, a restart keeps the day
//! and settles an open hold at its worst case, a hold over a line is refused
//! before any spend, and the mode off spends nothing and writes no row.

use theseus_protocol::rsi::SelfMode;

use crate::config::rsi::SelfBudgetConfig;
use crate::ledger::LedgerRow;
use crate::rsi::budget::{Budget, Line, Reservation, Reserved};
use crate::store::Store;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    (dir, store)
}

fn lines(branch_usd: f64, day_usd: f64) -> SelfBudgetConfig {
    SelfBudgetConfig {
        branch_usd,
        day_usd,
    }
}

fn budget(store: &Store, mode: SelfMode, cfg: &SelfBudgetConfig) -> Budget {
    Budget::new(mode, cfg, store.clone())
}

fn granted(r: Reserved) -> Reservation {
    match r {
        Reserved::Granted(r) => r,
        Reserved::Stopped(s) => panic!("stopped: {s}"),
    }
}

fn stopped(r: Reserved) -> crate::rsi::budget::Stopped {
    match r {
        Reserved::Stopped(s) => s,
        Reserved::Granted(r) => panic!("granted ${}", r.usd()),
    }
}

fn rows(store: &Store, kind: &str) -> Vec<serde_json::Value> {
    store
        .ledger_tail::<LedgerRow>(10_000)
        .unwrap()
        .into_iter()
        .map(|(_, r)| r)
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

fn spend(b: &Budget, branch: &str, worst: f64, actual: f64) {
    let r = granted(b.reserve("turn", Some(branch), worst).unwrap());
    b.settle(r, actual).unwrap();
}

/// A branch stops at $10, its spend and its work kept: the refusal says the
/// line, where it stands and what is left, and the row is written once.
#[test]
fn a_branch_stops_at_its_line_with_its_work_kept() {
    let (_dir, st) = store();
    let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
    spend(&b, "tsk_a", 4.0, 3.5);
    spend(&b, "tsk_a", 4.0, 4.0);
    // $7.50 is settled; $4 more would pass $10.
    let s = stopped(b.reserve("turn", Some("tsk_a"), 4.0).unwrap());
    assert_eq!(s.line, Line::Branch);
    assert_eq!((s.line_usd, s.spent_usd, s.left_usd), (10.0, 7.5, 2.5));
    assert!(s.to_string().contains("branch line, $10.00"), "{s}");
    // Asked again: still refused, and no second row.
    stopped(b.reserve("turn", Some("tsk_a"), 4.0).unwrap());
    let row = rows(&st, "self.budget.stopped");
    assert_eq!(row.len(), 1, "{row:?}");
    assert_eq!(row[0]["line"], "branch");
    assert_eq!(row[0]["branch"], "tsk_a");
    // What fits still goes, and another branch is untouched.
    spend(&b, "tsk_a", 2.0, 2.0);
    spend(&b, "tsk_b", 4.0, 1.0);
    let v = b.view().unwrap();
    let a = v.branches.iter().find(|x| x.branch == "tsk_a").unwrap();
    assert_eq!(a.spent_usd, 9.5);
    assert!(a.stopped);
    assert_eq!(v.day_spent_usd, 10.5);
}

/// The day stops at $30 across two branches, each under its own $10.
#[test]
fn the_day_stops_across_two_branches() {
    let (_dir, st) = store();
    let b = budget(&st, SelfMode::Act, &lines(20.0, 30.0));
    spend(&b, "tsk_a", 15.0, 15.0);
    spend(&b, "tsk_b", 10.0, 10.0);
    // $25 spent: tsk_c is far under its branch line and over the day's.
    let s = stopped(b.reserve("turn", Some("tsk_c"), 6.0).unwrap());
    assert_eq!(s.line, Line::Day);
    assert_eq!((s.line_usd, s.spent_usd, s.left_usd), (30.0, 25.0, 5.0));
    let row = rows(&st, "self.budget.stopped");
    assert_eq!(row.len(), 1);
    assert_eq!(row[0]["line"], "day");
    // A reservation with no branch counts to the day too.
    let r = granted(b.reserve("backlog", None, 5.0).unwrap());
    stopped(b.reserve("turn", Some("tsk_c"), 0.5).unwrap());
    b.settle(r, 5.0).unwrap();
    assert_eq!(b.view().unwrap().day_spent_usd, 30.0);
}

/// An open hold counts against the line before it settles, so two calls in
/// flight cannot pass it together.
#[test]
fn a_hold_counts_against_the_line_until_it_settles() {
    let (_dir, st) = store();
    let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
    let one = granted(b.reserve("turn", Some("tsk_a"), 6.0).unwrap());
    let s = stopped(b.reserve("turn", Some("tsk_a"), 6.0).unwrap());
    assert_eq!((s.spent_usd, s.left_usd), (6.0, 4.0));
    // The first came in cheap: the rest of its hold is released.
    b.settle(one, 1.0).unwrap();
    let _ = granted(b.reserve("turn", Some("tsk_a"), 6.0).unwrap());
    let settled = rows(&st, "self.budget.settled");
    assert_eq!(settled[0]["spent_usd"], 1.0);
    assert_eq!(settled[0]["released_usd"], 5.0);
}

/// A reservation over a line is refused before any spend, and no hold is
/// left behind.
#[test]
fn a_reservation_over_the_line_is_refused_before_any_spend() {
    let (_dir, st) = store();
    let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
    let s = stopped(b.reserve("turn", Some("tsk_a"), 10.5).unwrap());
    assert_eq!((s.line, s.left_usd), (Line::Branch, 10.0));
    let v = b.view().unwrap();
    assert_eq!(v.day_spent_usd, 0.0);
    assert!(rows(&st, "self.budget.reserved").is_empty());
    assert_eq!(rows(&st, "self.budget.stopped").len(), 1);
    // The same worst case with no branch is the day's to judge.
    let _ = granted(b.reserve("backlog", None, 10.5).unwrap());
}

/// A restart keeps the day's spend and settles a hold the old process left
/// open at its worst case.
#[test]
fn a_restart_keeps_the_day_and_settles_an_open_hold_at_its_worst_case() {
    let (_dir, st) = store();
    {
        let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
        spend(&b, "tsk_a", 3.0, 1.0);
        let _in_flight = granted(b.reserve("turn", Some("tsk_a"), 4.0).unwrap());
        assert_eq!(b.view().unwrap().day_spent_usd, 5.0);
    }
    // The next process: reading counts the open hold as spent, writes nothing.
    let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
    let before = rows(&st, "self.budget.settled").len();
    assert_eq!(b.view().unwrap().day_spent_usd, 5.0);
    assert_eq!(rows(&st, "self.budget.settled").len(), before);
    // Its first reservation settles the old hold, then holds its own.
    let r = granted(b.reserve("turn", Some("tsk_a"), 1.0).unwrap());
    let settled = rows(&st, "self.budget.settled");
    let recovered: Vec<_> = settled.iter().filter(|r| r["recovered"] == true).collect();
    assert_eq!(recovered.len(), 1, "{settled:?}");
    assert_eq!(recovered[0]["spent_usd"], 4.0);
    assert_eq!(recovered[0]["released_usd"], 0.0);
    let v = b.view().unwrap();
    assert_eq!(v.day_spent_usd, 6.0, "$1 settled, $4 recovered, $1 held");
    let a = &v.branches[0];
    assert_eq!((a.spent_usd, a.open_usd), (6.0, 1.0));
    b.settle(r, 0.5).unwrap();
    // A third process sees the settled day, and recovers nothing.
    let c = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
    assert_eq!(c.view().unwrap().day_spent_usd, 5.5);
    let _ = granted(c.reserve("turn", Some("tsk_a"), 0.1).unwrap());
    assert_eq!(
        rows(&st, "self.budget.settled")
            .iter()
            .filter(|r| r["recovered"] == true)
            .count(),
        1
    );
}

/// A new local day starts the day's account at zero; a branch's spend is its
/// own and stays.
#[test]
fn a_new_day_starts_the_days_account_and_a_branch_keeps_its_own() {
    let (_dir, st) = store();
    {
        let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
        spend(&b, "tsk_a", 8.0, 8.0);
    }
    // Yesterday's account, as the store holds it.
    let mut rec: crate::rsi::budget::State = st
        .get_meta(crate::rsi::budget::BUDGET_KEY)
        .unwrap()
        .unwrap();
    rec.day = "2000-01-01".into();
    st.put_meta(crate::rsi::budget::BUDGET_KEY, &rec).unwrap();
    let b = budget(&st, SelfMode::Act, &SelfBudgetConfig::default());
    let v = b.view().unwrap();
    assert_eq!(v.day_spent_usd, 0.0);
    assert_eq!(v.branches[0].spent_usd, 8.0);
    stopped(b.reserve("turn", Some("tsk_a"), 3.0).unwrap());
    let _ = granted(b.reserve("turn", Some("tsk_b"), 3.0).unwrap());
}

/// With the mode off the budget spends nothing, reads nothing and writes no
/// row.
#[test]
fn the_mode_off_spends_nothing_and_writes_no_row() {
    let (_dir, st) = store();
    let b = budget(&st, SelfMode::Off, &SelfBudgetConfig::default());
    let before = st.ledger_tail::<LedgerRow>(10_000).unwrap().len();
    let s = stopped(b.reserve("turn", Some("tsk_a"), 0.01).unwrap());
    assert_eq!(s.line, Line::Off);
    assert!(s.to_string().contains("is off"), "{s}");
    assert!(b.view().is_none());
    assert_eq!(st.ledger_tail::<LedgerRow>(10_000).unwrap().len(), before);
    assert!(st
        .get_meta::<crate::rsi::budget::State>(crate::rsi::budget::BUDGET_KEY)
        .unwrap()
        .is_none());
}
