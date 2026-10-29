//! A red that was attempted and not acted on is not spent (theseus-8p1oi): a
//! halt after the blame, a revert held with no card, and a revert that errors
//! each leave the red for the next pass, because the watch's memory tells
//! "acted on" from "attempted". A revert held with a card is the owner's and
//! stays settled. Each test runs two passes over one memory, with the world
//! put right between them.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::fetch_retry::flaky_git;
use super::*;

/// A prober that halts the switch the first time it is asked (a spend
/// ceiling tripping during the blame's model-free probes stands for the real
/// thing), then answers as main's gate does.
struct Halts {
    core: Arc<Core>,
    done: AtomicBool,
}

impl Prober for Halts {
    fn red_at<'a>(&'a self, p: &'a Place, commit: &'a str, benches: bool) -> ProbeFut<'a> {
        if !self.done.swap(true, Ordering::SeqCst) {
            halt(&self.core);
        }
        MainsGate.red_at(p, commit, benches)
    }
}

fn halt(core: &Core) {
    core.self_halt(&SelfHaltParams::default(), &Answerer::from("the CLI"))
        .unwrap();
}

fn resume(core: &Core) {
    core.self_resume(&SelfResumeParams::default(), &Answerer::from("the CLI"))
        .unwrap();
}

/// A self-join that broke main, and the red it left.
fn broken_join(r: &Rig) -> String {
    let bad = r.land("trim-mast", &[("broken.txt", "oops\n")]);
    r.joined("trim-mast", &bad);
    r.red("gate", &bad);
    bad
}

/// KEY (path 2): a halt that lands after the blame and before the revert
/// stops the pass with the red unreverted; once resumed, the next pass, over
/// the same memory, reverts it.
#[tokio::test]
async fn a_halt_after_the_blame_does_not_spend_the_red() {
    let r = rig(SelfMode::Act);
    let bad = broken_join(&r);
    let mut seams = r.seams();
    seams.prober = Arc::new(Halts {
        core: r.core.clone(),
        done: AtomicBool::new(false),
    });
    let mut known = Known::new();
    let first = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
    assert!(first.reverted.is_empty(), "{first:?}");
    assert!(first.stopped[0].1.contains("halted"), "{first:?}");
    assert_eq!(r.upstream_main(), bad, "nothing reached main");
    assert_eq!(
        known.attempted().count(),
        1,
        "attempted, not settled: {known:?}"
    );
    resume(&r.core);
    let second = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
    assert_eq!(
        second.reverted,
        [(bad.clone(), "gate".to_string())],
        "the halted red was acted on once the halt lifted: {second:?}"
    );
    assert!(!r.file_on_main("broken.txt"));
    assert_eq!(known.attempted().count(), 0, "acted on now: {known:?}");
}

/// KEY (path 3): a revert held with no card (a halt at its gate phase) is
/// not acted on, so the next pass reverts it.
#[tokio::test]
async fn a_revert_held_without_a_card_is_retried() {
    let r = rig(SelfMode::Act);
    let bad = broken_join(&r);
    let (core, once) = (r.core.clone(), AtomicBool::new(false));
    let mut seams = r.seams();
    seams.on_phase = Some(Arc::new(move |phase: &str| {
        if phase == "gate" && !once.swap(true, Ordering::SeqCst) {
            halt(&core);
        }
    }));
    let mut known = Known::new();
    let first = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
    assert_eq!(
        first.stopped,
        [(bad.clone(), "gate".to_string())],
        "{first:?}"
    );
    assert_eq!(r.upstream_main(), bad);
    assert!(
        r.posts().is_empty(),
        "a halt posts no card: {:?}",
        r.posts()
    );
    resume(&r.core);
    let second = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
    assert_eq!(second.reverted.len(), 1, "{second:?}");
    assert!(!r.file_on_main("broken.txt"));
    assert_eq!(r.rows("self.reverted").len(), 1);
}

/// KEY (path 1): a revert that errors (its own fetch fails) does not spend
/// the red either, and the retry reverts once, not twice.
#[tokio::test]
async fn a_revert_that_errors_is_retried_once() {
    let r = rig(SelfMode::Act);
    let flaky = flaky_git(&r);
    let bad = broken_join(&r);
    // The pass's own fetch and the blame's probes answer; the revert's fetch
    // is the one that fails.
    let mut seams = r.seams();
    let (arm, once) = (flaky.flag.clone(), AtomicBool::new(false));
    seams.on_phase = Some(Arc::new(move |phase: &str| {
        if phase == "revert" && !once.swap(true, Ordering::SeqCst) {
            std::fs::write(&arm, "").unwrap();
        }
    }));
    let mut known = Known::new();
    let first = r.core.self_revert_pass(&seams, &mut known).await;
    let e = first.expect_err("the revert's fetch failed, so the pass did");
    assert!(format!("{e:#}").contains("stand-in"), "{e:#}");
    assert_eq!(r.upstream_main(), bad);
    let second = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
    assert_eq!(second.reverted.len(), 1, "{second:?}");
    assert_eq!(r.rows("self.reverted").len(), 1);
    let third = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
    assert!(third.reverted.is_empty(), "never twice: {third:?}");
    assert_eq!(r.rows("self.reverted").len(), 1);
}

/// A revert that already landed on main, its row never written (a crash
/// between the push and the row), is not made twice: the retry finds the
/// commit that says it undoes the join, and hands the owner a card instead.
#[tokio::test]
async fn a_revert_already_on_main_is_never_made_twice() {
    let r = rig(SelfMode::Act);
    let bad = broken_join(&r);
    // The world lands the revert, as a self-revert's push would, and the row
    // is never written.
    let w = &r.work;
    git(w, &["checkout", "-q", "main"]);
    git(w, &["pull", "-q", "--ff-only"]);
    git(w, &["rm", "-q", "broken.txt"]);
    git(
        w,
        &[
            "commit",
            "-q",
            "-m",
            &format!("Revert it\n\nA self-revert: undoes the self-join {bad}."),
        ],
    );
    git(w, &["push", "-q", "origin", "main"]);
    let landed = r.upstream_main();
    let pass = r.pass().await;
    assert!(pass.reverted.is_empty(), "{pass:?}");
    assert_eq!(r.upstream_main(), landed, "no second revert");
    assert_eq!(r.posts().len(), 1, "{:?}", r.posts());
    assert!(r.posts()[0].contains("already reverted"), "{:?}", r.posts());
}

/// A red whose revert keeps stopping is tried a few times and then said, once,
/// as given up on: it does not hold the gate every pass for good.
#[tokio::test]
async fn a_red_whose_revert_keeps_stopping_is_given_up_on() {
    let r = rig(SelfMode::Act);
    broken_join(&r);
    let core = r.core.clone();
    let mut seams = r.seams();
    seams.on_phase = Some(Arc::new(move |phase: &str| {
        if phase == "gate" {
            halt(&core);
        }
    }));
    let mut known = Known::new();
    let (mut stops, mut gave_up) = (0, 0);
    for _ in 0..6 {
        resume(&r.core);
        let pass = r.core.self_revert_pass(&seams, &mut known).await.unwrap();
        stops += pass.stopped.len();
        gave_up += pass
            .left
            .iter()
            .filter(|(_, w)| w.contains("gave up"))
            .count();
    }
    assert_eq!(stops, 3, "three tries, then no more");
    assert_eq!(gave_up, 1, "said once");
}
