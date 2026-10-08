//! The daemon's day ceiling (theseus-kp20; `day_ceiling.rs`): a provider
//! call's reservation holds on the day and is refused past the ceiling with
//! nothing written; a settle spends the real cost; a late cost is booked in
//! full past it; a task's cost counts once; an AWS hand's never; two calls
//! racing at the edge cannot both pass; a transaction that fails lets its
//! hold go; and the day turns at local midnight.

use std::sync::{Arc, Barrier};

use serde_json::json;
use theseus_protocol::LedgerKind;

use crate::day_ceiling::{DayCeiling, Seed};
use crate::kernel::*;
use crate::tests::*;
use crate::types::*;

/// A world whose day ceiling is `ceiling` micros, in a zone three hours east
/// of UTC with no changes, so a test can put a call either side of its
/// midnight.
fn ceiling_world(ceiling: Micros) -> World {
    world_with(KernelConfig {
        daily_ceiling_micros: ceiling,
        zone: crate::repeat::TimeZone::posix("XST-3").unwrap(),
        ..KernelConfig::default()
    })
}

fn plan(w: &World, g: &TurnGuard, tool: &str, reserve: Micros) -> anyhow::Result<Action> {
    w.kernel
        .plan_action(g, &proposal(tool), RetryClass::SafeToRepeat, None, reserve)
}

fn refused(r: &anyhow::Result<Action>) -> Option<crate::day_ceiling::Reached> {
    match r.as_ref().err()?.downcast_ref::<KernelError>()? {
        KernelError::DayCeiling(r) => Some((**r).clone()),
        _ => None,
    }
}

fn today(w: &World) -> crate::day_ceiling::Today {
    let c = w.kernel.day_ceiling();
    c.today(c.now())
}

fn settle(w: &World, a: &Action, cost: Option<Micros>) {
    w.kernel
        .accept_completion(&completion(&a.correlation_id, Outcome::Succeeded, cost))
        .unwrap();
}

/// A reservation holds its amount on the day until it settles, and the call
/// whose reservation would pass the ceiling is refused, with nothing
/// written; its settle spends the real cost and frees the rest.
#[test]
fn a_reservation_holds_the_day_and_one_past_the_ceiling_is_refused_unwritten() {
    let w = ceiling_world(1_000);
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 600);
    assert_eq!((today(&w).spent, today(&w).held), (0, 600));
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let r = plan(&w, &g, PROVIDER_TOOL, 401);
    let reached = refused(&r).expect("refused by the day ceiling");
    assert_eq!(
        (reached.limit, reached.spent, reached.held, reached.needed),
        (1_000, 0, 600, 401)
    );
    assert_eq!(
        w.kernel.store().stats().unwrap().frames_appended,
        frames,
        "a refusal writes nothing"
    );
    let words = r.unwrap_err().to_string();
    assert!(
        words.starts_with("today's spend reached the $0.001 daily ceiling")
            && words.contains("`[kernel] daily_spend_ceiling_usd`")
            && words.contains("wait until midnight"),
        "{words}"
    );
    // The execution's budget never saw it.
    let b = w.kernel.execution(&e.id).unwrap().unwrap().budget;
    assert_eq!(b.reserved_micros, 600);
    // The call that fits is still made, and holds.
    let b2 = plan(&w, &g, PROVIDER_TOOL, 400).unwrap();
    assert_eq!(today(&w).held, 1_000);
    // A settle spends the real cost and frees the rest of its hold.
    settle(&w, &a, Some(250));
    assert_eq!((today(&w).spent, today(&w).held), (250, 400));
    w.kernel
        .authorize(&b2.correlation_id, &proposal(PROVIDER_TOOL), None)
        .unwrap();
    w.kernel.dispatch(&b2.correlation_id, None).unwrap();
    settle(&w, &b2, Some(100));
    assert_eq!((today(&w).spent, today(&w).held), (350, 0));
    assert!(today(&w).reached(), "a call was refused today");
}

/// A cost that lands after its call (speech, `book_spend`) is booked in
/// full, past the ceiling too: a real cost is never hidden. So is a late
/// settle's cost above its reservation.
#[test]
fn a_late_real_cost_is_booked_in_full_past_the_ceiling() {
    let w = ceiling_world(1_000);
    let (_, e, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 900);
    // The provider billed more than the reservation.
    settle(&w, &a, Some(1_200));
    assert_eq!(today(&w).spent, 1_200);
    w.kernel
        .book_spend(&e.id, 300, LedgerKind::SpeechSynthesized, json!({}))
        .unwrap();
    assert_eq!(today(&w).spent, 1_500);
    assert!(refused(&plan(&w, &g, PROVIDER_TOOL, 1)).is_some());
}

/// A task's provider call counts once: its cost carried to its parent's
/// budget adds nothing to the day. The task's carve (a reservation of the
/// parent's) is no model call, and an AWS hand's reservation is not model
/// spend: neither holds the day.
#[test]
fn a_tasks_cost_counts_once_and_a_carve_or_a_hand_never() {
    let w = ceiling_world(100_000);
    let (_, parent, g) = running(&w);
    let call = new_id("act");
    let t = w
        .kernel
        .open_task(&g, &call, 30_000, None, false, |_| Ok(vec![]))
        .unwrap();
    assert_eq!(today(&w).held, 0, "a carve is not a model call");
    let tg = w.kernel.admit(&t.task.id).unwrap();
    let a = dispatched(&w, &tg, PROVIDER_TOOL, 10_000);
    assert_eq!(today(&w).held, 10_000);
    settle(&w, &a, Some(4_000));
    let p = w.kernel.execution(&parent.id).unwrap().unwrap();
    assert_eq!(
        p.budget.spent_micros, 4_000,
        "the parent's spend includes it"
    );
    assert_eq!(
        (today(&w).spent, today(&w).held),
        (4_000, 0),
        "counted once"
    );
    // An AWS hand's reservation is not model spend.
    let h = dispatched(&w, &g, "aws.hand", 50_000);
    assert_eq!(today(&w).held, 0);
    settle(&w, &h, Some(50_000));
    assert_eq!(today(&w).spent, 4_000);
}

/// Two provider calls of two sessions racing at the edge: one passes and
/// holds, the other is refused, whatever the order.
#[test]
fn two_reservations_racing_at_the_edge_cannot_both_pass() {
    for _ in 0..20 {
        let w = ceiling_world(1_000);
        let (_, _, g1) = running(&w);
        let (_, _, g2) = running(&w);
        let gate = Barrier::new(2);
        let (r1, r2) = std::thread::scope(|s| {
            let a = s.spawn(|| {
                gate.wait();
                plan(&w, &g1, PROVIDER_TOOL, 600).is_ok()
            });
            let b = s.spawn(|| {
                gate.wait();
                plan(&w, &g2, PROVIDER_TOOL, 600).is_ok()
            });
            (a.join().unwrap(), b.join().unwrap())
        });
        assert!(r1 ^ r2, "exactly one passes: {r1} {r2}");
        assert_eq!(today(&w).held, 600);
    }
}

/// A transaction that fails after its plan held the day writes nothing, and
/// lets the hold go.
#[test]
fn a_failed_transaction_lets_its_hold_go() {
    let w = ceiling_world(1_000);
    let (_, e, g) = running(&w);
    let r: anyhow::Result<()> = w.kernel.frame(&[&e.id], |k| {
        k.plan_action(
            &g,
            &proposal(PROVIDER_TOOL),
            RetryClass::SafeToRepeat,
            None,
            700,
        )?;
        assert_eq!(today(&w).held, 700, "held inside");
        anyhow::bail!("a later step failed")
    });
    assert!(r.is_err());
    assert_eq!(today(&w).held, 0, "the hold went with the transaction");
    // Its settle inside a transaction lands once the frame is written.
    let a = plan(&w, &g, PROVIDER_TOOL, 700).unwrap();
    w.kernel
        .frame(&[&e.id], |k| {
            k.authorize(&a.correlation_id, &proposal(PROVIDER_TOOL), None)?;
            k.dispatch(&a.correlation_id, None)?;
            k.accept_completion(&completion(
                &a.correlation_id,
                Outcome::Succeeded,
                Some(300),
            ))?;
            assert_eq!(today(&w).spent, 0, "not before its frame");
            Ok(())
        })
        .unwrap();
    assert_eq!((today(&w).spent, today(&w).held), (300, 0));
}

/// The day turns at local midnight (the kernel's zone): a call just before
/// it is refused at the ceiling, and one just after starts a new day at
/// zero, yesterday's spend not counted. A reservation made before midnight
/// and settled after counts in the day it settles.
#[test]
fn the_day_turns_at_local_midnight() {
    let w = ceiling_world(1_000);
    // 2026-10-08 23:59:00 at UTC+3 is 20:59:00 UTC.
    let before = 1_791_493_140_000;
    w.clock.set(before);
    let (_, _, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 900);
    settle(&w, &a, Some(900));
    let open = dispatched(&w, &g, PROVIDER_TOOL, 100);
    let r = refused(&plan(&w, &g, PROVIDER_TOOL, 200)).expect("refused before midnight");
    assert_eq!(r.day, "2026-10-08");
    assert_eq!(r.turns_at, "2026-10-09 00:00");
    assert_eq!(r.turns_at_ms, before + 60_000);
    // A minute later it is the 9th: yesterday's 900 does not count, the
    // open call's hold does, and a call fits again.
    w.clock.set(before + 60_000);
    let t = today(&w);
    assert_eq!((t.day.as_str(), t.spent, t.held), ("2026-10-09", 0, 100));
    assert!(!t.reached());
    plan(&w, &g, PROVIDER_TOOL, 800).unwrap();
    // The open call settles today, and counts today.
    settle(&w, &open, Some(50));
    assert_eq!(today(&w).spent, 50);
    // The last millisecond of the 8th was still the 8th.
    let d = DayCeiling::new(1, crate::repeat::TimeZone::posix("XST-3").unwrap(), w.clock);
    assert_eq!(d.day_of(before + 59_999).0, "2026-10-08");
    assert_eq!(d.day_of(before + 60_000).0, "2026-10-09");
}

/// A start's seed (the core reads today's rows back): the day keeps what it
/// spent before the restart, and a day stopped before it is still stopped,
/// with its notice already taken.
#[test]
fn a_seed_keeps_the_days_spend_and_its_notice_across_a_restart() {
    let w = ceiling_world(1_000);
    let (_, _, g) = running(&w);
    let c = w.kernel.day_ceiling();
    c.seed(
        c.now(),
        &Seed {
            spent: 950,
            reached_at_ms: None,
        },
    );
    assert!(refused(&plan(&w, &g, PROVIDER_TOOL, 51)).is_some());
    assert!(c.take_notice(c.now()), "the day's first refusal is noticed");
    assert!(!c.take_notice(c.now()), "once a day");
    let w2 = ceiling_world(1_000);
    let c2 = w2.kernel.day_ceiling();
    c2.seed(
        c2.now(),
        &Seed {
            spent: 1_000,
            reached_at_ms: Some(c2.now()),
        },
    );
    let (_, _, g2) = running(&w2);
    assert!(refused(&plan(&w2, &g2, PROVIDER_TOOL, 1)).is_some());
    assert!(!c2.take_notice(c2.now()), "noticed before the restart");
}

/// The background calls' holds: a hold dropped unsettled is let go, a
/// settled one spends its real cost; a judge's hold by amount settles by
/// amount.
#[test]
fn a_background_hold_settles_or_lets_go() {
    let w = ceiling_world(1_000);
    let c: &Arc<DayCeiling> = w.kernel.day_ceiling();
    let h = c.hold_call(c.now(), 700).unwrap();
    assert!(c.hold_call(c.now(), 301).is_err());
    drop(h);
    assert_eq!(today(&w).held, 0);
    let h = c.hold_call(c.now(), 700).unwrap();
    h.settle(650);
    assert_eq!((today(&w).spent, today(&w).held), (650, 0));
    c.hold(c.now(), "judge", 100).unwrap();
    c.hold(c.now(), "judge", 100).unwrap();
    c.settle_part(c.now(), "judge", 100, 30);
    assert_eq!((today(&w).spent, today(&w).held), (680, 100));
    assert!(c.fits(c.now(), 220).is_ok());
    assert!(c.fits(c.now(), 221).is_err());
}

/// The reservation path's cost (theseus-kp20's FAST check), in one run: a
/// provider call's `plan_action` with its reservation (A: it holds the day),
/// then a call of another tool reserving the same (B: no hold, the path as
/// it was), then A again, each timed alone over many calls; and the hold and
/// settle by themselves. A measurement: `--run-ignored only --no-capture`.
#[test]
#[ignore = "a measurement"]
fn the_reservation_path_with_and_without_the_day_hold() {
    use std::time::Instant;
    const N: usize = 500;
    let pct = |v: &mut Vec<u128>, p: usize| {
        v.sort_unstable();
        v[(v.len() * p / 100).min(v.len() - 1)]
    };
    // Each block on a fresh store, so the store's growth is the same for each.
    for (label, tool) in [
        ("A provider call (holds the day)", PROVIDER_TOOL),
        ("B another tool (no hold)", "fs.read"),
        ("A provider call (holds the day)", PROVIDER_TOOL),
    ] {
        let w = world_with(KernelConfig::default());
        let (_, _, g) = running(&w);
        let mut ns = Vec::with_capacity(N);
        for _ in 0..N {
            let t = Instant::now();
            let a = plan(&w, &g, tool, 10).unwrap();
            ns.push(t.elapsed().as_nanos());
            w.kernel
                .authorize(&a.correlation_id, &proposal(tool), None)
                .unwrap();
            w.kernel.dispatch(&a.correlation_id, None).unwrap();
            settle(&w, &a, Some(1));
        }
        println!(
            "{label}: plan_action p50 {} µs, p95 {} µs over {N} calls",
            pct(&mut ns, 50) / 1000,
            pct(&mut ns, 95) / 1000
        );
    }
    let w = world_with(KernelConfig::default());
    let c = w.kernel.day_ceiling();
    let mut ns = Vec::with_capacity(100_000);
    for i in 0..100_000 {
        let id = format!("rsv_{i}");
        let t = Instant::now();
        c.hold(c.now(), &id, 10).unwrap();
        c.settle(c.now(), &id, 10, Some(1));
        ns.push(t.elapsed().as_nanos());
    }
    println!(
        "the hold and its settle alone: p50 {} ns, p95 {} ns over 100,000",
        pct(&mut ns, 50),
        pct(&mut ns, 95)
    );
}

/// A task's provider call meets the same ceiling as its parent's: at it the
/// task's plan is refused and writes nothing (the core then fails the task's
/// turn, `daily_ceiling`, and its report says why), while a call of another
/// tool still plans.
#[test]
fn a_tasks_call_at_the_ceiling_is_refused_too() {
    let w = ceiling_world(10_000);
    let (_, _, g) = running(&w);
    let a = dispatched(&w, &g, PROVIDER_TOOL, 9_000);
    settle(&w, &a, Some(9_000));
    let t = w
        .kernel
        .open_task(&g, &new_id("act"), 5_000, None, false, |_| Ok(vec![]))
        .unwrap();
    let tg = w.kernel.admit(&t.task.id).unwrap();
    let frames = w.kernel.store().stats().unwrap().frames_appended;
    let r = refused(&plan(&w, &tg, PROVIDER_TOOL, 2_000)).expect("refused");
    assert_eq!((r.spent, r.needed), (9_000, 2_000));
    assert_eq!(w.kernel.store().stats().unwrap().frames_appended, frames);
    plan(&w, &tg, "fs.read", 0).unwrap();
}
