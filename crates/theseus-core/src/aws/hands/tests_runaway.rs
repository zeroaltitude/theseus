//! Runaway mode across a restart (theseus-6hkx): its mark yields to a
//! config that gives its line more room, and keeps a lowered line's.

use super::runaway::{self, Sink};
use super::tests_hands::{calls, group_of, play_since, record, rig, rows, state_of, turn, until};
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

/// One Lambda hand whose invocation ran `init` before its child did (a cold
/// start), settled through the real poller: what the hour's meter counts for
/// it, and whether the brake (a line of 200 micro-dollars: 100 at a factor
/// of 2) is reached (theseus-63wzi).
async fn settled_after_init(init: std::time::Duration) -> (u64, bool) {
    let r = rig(calls(serde_json::json!({"argv": ["true"]})));
    turn(&r.core, "run one hand").await;
    let a = rebuilt(&r, |c| {
        c.hourly_alert_usd = 0.000_1;
        c.daily_budget_usd = None;
        c.runaway_factor = 2.0;
    });
    let g = group_of(&r.core);
    let spec = r.state.invoked().remove(0);
    r.core.poll_hands_after_serving();
    // The real hand role, its clock begun `init` before its child ran.
    play_since(&spec, super::hand::Began::now().earlier_by(init)).await;
    until("the hand to settle", || {
        state_of(&r.core, &spec.correlation_id).is_settled()
    })
    .await;
    assert_eq!(record(&r.core, &g).hands.len(), 1);
    let now = theseus_protocol::now_unix_ms();
    let micros =
        runaway::spend_since(&r.core.kernel, &r.core.store, &a.id, now - 3_600_000).unwrap();
    let fired = sink!(r).observe(&a, now).unwrap().is_some();
    (micros, fired)
}

/// A cold start's init is billed with the invocation, so it is in what a
/// hand settles at, and the brake reads it: the same job, thirteen seconds
/// of init before it, reaches the line a bare child's run does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_cold_starts_init_is_in_what_the_brake_reads() {
    let (warm, warm_fired) = settled_after_init(std::time::Duration::ZERO).await;
    let (cold, cold_fired) = settled_after_init(std::time::Duration::from_secs(13)).await;
    // 2048 MB for 13 s at Lambda's rate is about 433 micro-dollars.
    assert!(
        (400..=500).contains(&cold),
        "13 s of init at 2 GB: {cold} micro-dollars"
    );
    assert!(warm < 50, "a bare child's run: {warm} micro-dollars");
    assert!(cold_fired, "the brake fires on the whole invocation");
    assert!(!warm_fired, "and not on a bare child's run");
}
