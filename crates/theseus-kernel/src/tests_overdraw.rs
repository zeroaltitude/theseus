//! A spend limit that notifies (theseus-usei; `overdraw.rs`): a provider
//! call past the limit is planned and reserved, never refused; every other
//! reservation, a pinned limit's, and a task of a pinned parent still refuse;
//! a task's carve is what it asked for; with notify off, today's refusal.

use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

fn notifying(spend_limit_micros: Micros) -> KernelConfig {
    KernelConfig {
        spend_limit_notify: true,
        ..limited(spend_limit_micros)
    }
}

fn plan(w: &World, g: &TurnGuard, tool: &str, reserve: Micros) -> anyhow::Result<Action> {
    w.kernel
        .plan_action(g, &proposal(tool), RetryClass::SafeToRepeat, None, reserve)
}

fn over_budget(r: anyhow::Result<Action>) -> bool {
    matches!(
        r.err()
            .as_ref()
            .and_then(|e| e.downcast_ref::<KernelError>()),
        Some(KernelError::OverBudget { .. })
    )
}

/// The config's limit notifies: a provider call that reserves twice the limit
/// goes on, reserved and recorded in full (its `action.planned` row), and the
/// spend may pass the limit; an AWS hand's reservation over it still refuses.
#[test]
fn a_provider_call_past_a_notifying_limit_is_reserved_and_goes_on() {
    let w = world_with(notifying(1_000_000));
    let (s, e, g) = following(&w);
    assert!(w.kernel.overdraws(&e));
    let a = plan(&w, &g, PROVIDER_TOOL, 2_000_000).unwrap();
    assert_eq!(a.reserved_micros, 2_000_000);
    let b = w.kernel.execution(&e.id).unwrap().unwrap().budget;
    assert_eq!((b.reserved_micros, b.available()), (2_000_000, 0));
    w.kernel
        .authorize(&a.correlation_id, &proposal(PROVIDER_TOOL), None)
        .unwrap();
    w.kernel.dispatch(&a.correlation_id, None).unwrap();
    w.kernel
        .accept_completion(&completion(
            &a.correlation_id,
            Outcome::Succeeded,
            Some(1_500_000),
        ))
        .unwrap();
    let b = w.kernel.execution(&e.id).unwrap().unwrap().budget;
    assert_eq!((b.spent_micros, b.reserved_micros), (1_500_000, 0));
    // Past the limit, the next call goes on too.
    plan(&w, &g, PROVIDER_TOOL, 2_000_000).unwrap();
    // AWS spend still restricts (`[aws]` is managed apart).
    assert!(over_budget(plan(&w, &g, "aws.hand", 10)));
    let planned = rows(&w, &s, "action.planned");
    assert_eq!(
        planned[0]["reserved_usd"], 2.0,
        "the reservation is recorded"
    );
}

/// With notify off (`spend_limit_mode = "ask"`), the same call is refused, as
/// before, and so is a provider call under a pinned limit (an MCP client's,
/// a place's ceiling) even when the config's notifies.
#[test]
fn ask_mode_and_a_pinned_limit_still_refuse() {
    let w = world_with(limited(1_000_000));
    let (_, e, g) = following(&w);
    assert!(!w.kernel.overdraws(&e));
    assert!(over_budget(plan(&w, &g, PROVIDER_TOOL, 2_000_000)));

    let w = world_with(notifying(1_000_000));
    let (_, e, g) = running(&w);
    assert!(e.budget.pinned, "opened with its own limit");
    assert!(!w.kernel.overdraws(&e));
    assert!(over_budget(plan(&w, &g, PROVIDER_TOOL, 2_000_000)));
}

/// Under a notifying limit a task's carve is what it asked for, up to the
/// parent's whole limit, even past what the parent has left, and the task
/// notifies as its parent does; a parent whose limit is pinned carves as
/// before.
#[test]
fn a_tasks_carve_under_a_notifying_limit_is_what_it_asked_for() {
    let w = world_with(notifying(100_000));
    let (_, parent, g) = following(&w);
    let carve = |want: Micros| {
        w.kernel
            .open_task(&g, &new_id("act"), want, None, false, |_| Ok(vec![]))
            .unwrap()
    };
    let first = carve(80_000);
    assert_eq!(first.task.budget.limit_micros, 80_000);
    assert!(w.kernel.overdraws(&first.task));
    let second = carve(50_000);
    assert_eq!(
        (second.available_before, second.task.budget.limit_micros),
        (20_000, 50_000),
        "past what the parent had left"
    );
    let third = carve(500_000);
    assert_eq!(
        (third.available_before, third.task.budget.limit_micros),
        (0, 100_000),
        "a parent past its limit still carves, up to its whole limit"
    );
    let b = w.kernel.execution(&parent.id).unwrap().unwrap().budget;
    assert_eq!((b.reserved_micros, b.available()), (230_000, 0));

    let w = world_with(notifying(100_000));
    let (_, _, g) = running(&w);
    let t = w
        .kernel
        .open_task(&g, &new_id("act"), 500_000, None, false, |_| Ok(vec![]))
        .unwrap();
    assert_eq!(t.task.budget.limit_micros, 100_000, "capped, as before");
    assert!(!w.kernel.overdraws(&t.task));
}
