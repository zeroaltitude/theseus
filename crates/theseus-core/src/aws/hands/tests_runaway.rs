//! Runaway mode across a restart (theseus-6hkx): its mark yields to a
//! config that gives its line more room, and keeps a lowered line's.

use super::runaway::{self, Sink};
use super::tests_hands::{calls, rig, rows};
use crate::aws::Account;

const DAY_LINE: u64 = 1;

/// The rig's account, rebuilt from its config with `f` applied, over the same
/// store: what a restart onto an edited config binds.
fn rebuilt(
    r: &super::tests_hands::Rig,
    f: impl FnOnce(&mut crate::config::AwsAccountConfig),
) -> Account {
    let aws = r.core.tools.aws.clone().unwrap();
    let old = aws.account(None).unwrap().clone();
    let mut cfg = old.cfg.clone();
    f(&mut cfg);
    Account::new(&old.id, &cfg, crate::aws::tests::board())
}

macro_rules! sink {
    ($r:expr) => {
        Sink {
            kernel: &$r.core.kernel,
            store: &$r.core.store,
            outbox: &$r.core.outbox,
            rec: $r.core.rec(None),
        }
    };
}

/// The hour's line, 5 micro-dollars at a factor of 2: a reservation of 10
/// enters runaway mode.
fn hour_rig() -> (super::tests_hands::Rig, Account) {
    let r = rig(calls(serde_json::json!({"argv": ["true"]})));
    let a = rebuilt(&r, |c| {
        c.hourly_alert_usd = 0.000_005;
        c.daily_budget_usd = None;
        c.runaway_factor = 2.0;
    });
    (r, a)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_raised_hourly_line_ends_the_mark_at_a_restart() {
    let (r, a) = hour_rig();
    let sink = sink!(r);
    let now = theseus_protocol::now_unix_ms();
    assert!(sink.admit(&a, 10, now).unwrap().is_some(), "enters");
    assert!(
        sink.admit(&a, 1, now).unwrap().is_some(),
        "refused in the mark"
    );
    // The restart: the same store, a raised line.
    let raised = rebuilt(&r, |c| {
        c.hourly_alert_usd = 0.000_005;
        c.daily_budget_usd = None;
        c.runaway_factor = 2.0;
        c.hourly_alert_usd = 0.001;
    });
    assert_eq!(
        sink.admit(&raised, 10, now).unwrap(),
        None,
        "the same group runs"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_raised_factor_ends_the_mark_at_a_restart() {
    let (r, a) = hour_rig();
    let sink = sink!(r);
    let now = theseus_protocol::now_unix_ms();
    assert!(sink.admit(&a, 10, now).unwrap().is_some());
    let raised = rebuilt(&r, |c| {
        c.hourly_alert_usd = 0.000_005;
        c.daily_budget_usd = None;
        c.runaway_factor = 100.0;
    });
    assert_eq!(sink.admit(&raised, 10, now).unwrap(), None);
    // Observing agrees: no mark holds.
    assert!(sink.observe(&raised, now).unwrap().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lowered_line_keeps_the_mark() {
    let (r, a) = hour_rig();
    let sink = sink!(r);
    let now = theseus_protocol::now_unix_ms();
    assert!(sink.admit(&a, 10, now).unwrap().is_some());
    let lowered = rebuilt(&r, |c| {
        c.daily_budget_usd = None;
        c.runaway_factor = 2.0;
        c.hourly_alert_usd = 0.000_001;
    });
    assert!(
        sink.admit(&lowered, 1, now).unwrap().is_some(),
        "still refused"
    );
    assert!(runaway::current(&r.core.store, &lowered, now).is_some());
    assert_eq!(rows(&r.core, "aws.runaway").len(), 1, "no second row");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_removed_daily_line_ends_its_mark() {
    let r = rig(calls(serde_json::json!({"argv": ["true"]})));
    let a = rebuilt(&r, |c| {
        c.hourly_alert_usd = 100.0;
        c.daily_budget_usd = Some(DAY_LINE as u32);
        c.runaway_factor = 2.0;
    });
    let sink = sink!(r);
    let now = theseus_protocol::now_unix_ms();
    let why = sink.admit(&a, 2_000_000, now).unwrap().expect("refused");
    assert!(why.contains("daily_budget_usd"), "{why}");
    let without = rebuilt(&r, |c| {
        c.hourly_alert_usd = 100.0;
        c.daily_budget_usd = None;
        c.runaway_factor = 2.0;
    });
    assert!(runaway::current(&r.core.store, &without, now).is_none());
    assert_eq!(sink.admit(&without, 1, now).unwrap(), None);
}
