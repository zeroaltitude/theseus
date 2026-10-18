//! The A/B replay's rules and its money, with a stand-in model (`Scripted`):
//! a pass, a fail for each rule, the tool that is never called, and the
//! reservation on `[self.budget]` before every arm.

use std::sync::Mutex;

use anyhow::Result;
use theseus_protocol::rsi::SelfMode;

use super::ab::{ab_replay, judge, Arm, ArmFacts, Arms, Caps};
use super::tests_dry::block_on;
use crate::config::self_improve::SelfBudgetConfig;
use crate::rsi::budget::Budget;
use crate::store::Store;

fn arm(tokens: u64, ms: u64, decisions: u32, called: bool) -> ArmFacts {
    ArmFacts {
        matched: true,
        tokens,
        wall_ms: ms,
        decisions,
        called,
        usd: 0.10,
    }
}

/// The recorded tool set: seven model decisions, 100,000 tokens, 60 s.
fn a() -> ArmFacts {
    arm(100_000, 60_000, 7, false)
}

/// The stand-in model: arm B calls the tool and goes straight to the answer.
struct Scripted {
    a: ArmFacts,
    b: ArmFacts,
    log: Mutex<Vec<(String, Arm)>>,
}

impl Scripted {
    fn new(a: ArmFacts, b: ArmFacts) -> Self {
        Self {
            a,
            b,
            log: Mutex::new(vec![]),
        }
    }
    fn calls(&self) -> usize {
        self.log.lock().unwrap().len()
    }
}

impl Arms for Scripted {
    async fn run(&self, turn: &str, arm: Arm, _left: f64) -> Result<ArmFacts> {
        self.log.lock().unwrap().push((turn.into(), arm));
        Ok(if arm == Arm::A { self.a } else { self.b })
    }
}

fn turns(n: usize, b: ArmFacts) -> Vec<(ArmFacts, ArmFacts)> {
    vec![(a(), b); n]
}

#[test]
fn a_tool_that_saves_tokens_time_and_decisions_passes() {
    let r = judge(&turns(4, arm(55_000, 25_000, 3, true)));
    assert!(r.pass, "{r:?}");
    assert_eq!(
        (r.turns, r.a_matched, r.b_matched, r.b_called),
        (4, 4, 4, 4)
    );
    assert!((r.token_saving - 0.45).abs() < 1e-9 && r.decisions_removed == 4.0);
    assert!(r.back_to_backlog.is_none());
}

#[test]
fn saving_time_alone_is_enough_and_so_is_saving_tokens_alone() {
    assert!(judge(&turns(3, arm(100_000, 30_000, 4, true))).pass);
    assert!(judge(&turns(3, arm(70_000, 60_000, 4, true))).pass);
}

#[test]
fn no_saving_fails() {
    let r = judge(&turns(3, arm(95_000, 55_000, 3, true)));
    assert!(!r.pass && r.why.contains("neither is 15%"), "{r:?}");
}

#[test]
fn too_few_decisions_removed_fails_even_when_the_tokens_fall() {
    // Six decisions instead of seven: one removed, under the two it needs.
    let r = judge(&turns(3, arm(50_000, 20_000, 6, true)));
    assert!(!r.pass && r.why.contains("decisions"), "{r:?}");
    assert!(judge(&turns(3, arm(50_000, 20_000, 5, true))).pass);
}

#[test]
fn a_loss_of_success_fails() {
    let mut worse = arm(50_000, 20_000, 3, true);
    worse.matched = false;
    let r = judge(&turns(4, worse));
    assert!(!r.pass && r.why.contains("matched"), "{r:?}");
    // B matching as often as A is enough: both lost the same turn.
    let mut both = turns(4, arm(50_000, 20_000, 3, true));
    both[0].0.matched = false;
    both[0].1.matched = false;
    assert!(judge(&both).pass);
}

#[test]
fn a_tool_the_model_never_calls_goes_back_to_the_backlog_with_that_evidence() {
    let r = judge(&turns(5, arm(100_000, 60_000, 7, false)));
    assert!(!r.pass && r.b_called == 0);
    let why = r.back_to_backlog.expect("evidence for the backlog");
    assert!(
        why.contains("never called it") && why.contains("5 replayed turns"),
        "{why}"
    );
    assert!(
        !why.to_lowercase().contains("tell the model"),
        "a model is never told to use it"
    );
}

#[test]
fn no_turns_cannot_pass() {
    let r = judge(&[]);
    assert!(!r.pass && r.why.contains("no turn"));
}

fn rig(day: f64) -> (tempfile::TempDir, Budget) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let cfg = SelfBudgetConfig {
        branch_usd: 10.0,
        day_usd: day,
    };
    (dir, Budget::new(SelfMode::Act, &cfg, store))
}

fn ids(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("t{i}")).collect()
}

#[test]
fn every_arm_reserves_on_the_self_budget_first_and_the_day_line_stops_it() {
    // $0.35 a day, $0.30 an arm: the first arm fits and settles at $0.10, and
    // the second asks for $0.30 more than the day has left.
    let (_d, b) = rig(0.35);
    let model = Scripted::new(a(), arm(50_000, 20_000, 3, true));
    let caps = Caps {
        mint_usd: 2.0,
        arm_usd: 0.30,
        turns: 5,
    };
    let r = block_on(ab_replay(&model, &b, "branch", &ids(5), &caps)).unwrap();
    assert_eq!(
        model.calls(),
        1,
        "the refused reservation ran nothing: {r:?}"
    );
    let why = r.stopped.clone().expect("stopped at the day line");
    assert!(why.contains("day line"), "{why}");
    assert!(!r.pass, "{r:?}");
    // The one arm that ran was settled at what it cost, not held at its worst case.
    assert!((r.spent_usd - 0.10).abs() < 1e-9, "{r:?}");
}

#[test]
fn the_mints_own_dollars_stop_it_and_a_turn_limit_caps_the_turns() {
    let (_d, b) = rig(30.0);
    let model = Scripted::new(a(), arm(50_000, 20_000, 3, true));
    // $0.25 for the mint, $0.10 an arm run: two arms, then a third is asked
    // for the $0.05 that is left and the fourth for nothing.
    let caps = Caps {
        mint_usd: 0.25,
        arm_usd: 0.50,
        turns: 5,
    };
    let r = block_on(ab_replay(&model, &b, "branch", &ids(5), &caps)).unwrap();
    assert!(
        r.stopped
            .as_deref()
            .is_some_and(|s| s.contains("[self.mint] usd")),
        "{r:?}"
    );
    let caps = Caps {
        mint_usd: 9.0,
        arm_usd: 0.50,
        turns: 3,
    };
    let model = Scripted::new(a(), arm(50_000, 20_000, 3, true));
    let r = block_on(ab_replay(&model, &b, "branch", &ids(5), &caps)).unwrap();
    assert_eq!((r.turns, model.calls()), (3, 6), "{r:?}");
    assert!(r.pass && r.stopped.is_none(), "{r:?}");
}

#[test]
fn with_the_self_mode_off_no_arm_runs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let off = Budget::new(
        SelfMode::Off,
        &SelfBudgetConfig {
            branch_usd: 10.0,
            day_usd: 30.0,
        },
        store,
    );
    let model = Scripted::new(a(), arm(50_000, 20_000, 3, true));
    let caps = Caps {
        mint_usd: 2.0,
        arm_usd: 0.30,
        turns: 5,
    };
    let r = block_on(ab_replay(&model, &off, "branch", &ids(3), &caps)).unwrap();
    assert_eq!(model.calls(), 0);
    assert!(r.stopped.is_some_and(|s| s.contains("off")));
}
