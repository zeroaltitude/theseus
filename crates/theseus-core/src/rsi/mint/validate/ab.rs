//! The model A/B replay (§2.5 tier 3, §3.1): at most `[self.mint] replay_turns`
//! check-half whole turns, each run twice with a model, **arm A** with the
//! tool set the turn had and **arm B** with the minted tool loaded beside it.
//! Each arm is a whole-turn replay under the existing caps (`whole_loops`,
//! `whole_seconds`, `whole_usd`), and **each is reserved on `[self.budget]`
//! before it is run**, and held to `[self.mint] usd` for the one mint: a
//! refusal stops the replay where it stands, and what ran is judged.
//!
//! **Pass:** B's facts match the recording as often as A's (no loss of
//! success); B saves at least 15% of the tokens **or** of the wall time; and
//! at least 2 model decisions (model calls) are removed per occurrence, on
//! average. **If the model never calls the tool in arm B**, the motif is real
//! but the tool's shape or description is wrong: it goes back to the backlog
//! with that evidence, and a model is never told to use it.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::rsi::budget::{Budget, Reserved};

/// The share of tokens or wall time B must save.
pub const MIN_SAVING: f64 = 0.15;
/// The model calls B must remove from each occurrence, on average.
pub const MIN_DECISIONS_REMOVED: f64 = 2.0;

/// Which arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    /// The recorded tool set.
    A,
    /// The same, plus the minted tool.
    B,
}

/// What one arm of one turn did.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArmFacts {
    /// Its facts match the recording's: the same paths changed, the same exit
    /// codes, no worse a last test result.
    pub matched: bool,
    pub tokens: u64,
    pub wall_ms: u64,
    /// Model calls it made.
    pub decisions: u32,
    /// The minted tool was called (always false in arm A).
    pub called: bool,
    pub usd: f64,
}

/// Runs one arm of one turn with a model. `left_usd` is the most it may spend.
pub trait Arms {
    /// `turn` is the check-half occurrence's id.
    fn run(
        &self,
        turn: &str,
        arm: Arm,
        left_usd: f64,
    ) -> impl std::future::Future<Output = Result<ArmFacts>>;
}

/// The A/B replay's result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AbReport {
    /// Turns replayed in both arms.
    pub turns: usize,
    pub a_matched: usize,
    pub b_matched: usize,
    /// B's saving over A, as shares: tokens and wall time.
    pub token_saving: f64,
    pub time_saving: f64,
    /// Model calls removed per occurrence, on average.
    pub decisions_removed: f64,
    /// Turns in which B called the tool.
    pub b_called: usize,
    /// The replay stopped before its last turn: the refusal's words.
    pub stopped: Option<String>,
    pub spent_usd: f64,
    pub pass: bool,
    pub why: String,
    /// Set when the model never called the tool: what to put in the backlog.
    pub back_to_backlog: Option<String>,
}

/// The pass rules over the turns both arms finished.
pub fn judge(turns: &[(ArmFacts, ArmFacts)]) -> AbReport {
    let n = turns.len();
    let sum = |f: fn(&ArmFacts) -> f64, side: usize| -> f64 {
        turns
            .iter()
            .map(|t| f(if side == 0 { &t.0 } else { &t.1 }))
            .sum()
    };
    let saving = |f: fn(&ArmFacts) -> f64| -> f64 {
        let a = sum(f, 0);
        if a <= 0.0 {
            0.0
        } else {
            (a - sum(f, 1)) / a
        }
    };
    let (a_matched, b_matched) = (
        turns.iter().filter(|t| t.0.matched).count(),
        turns.iter().filter(|t| t.1.matched).count(),
    );
    let removed = if n == 0 {
        0.0
    } else {
        turns
            .iter()
            .map(|t| f64::from(t.0.decisions) - f64::from(t.1.decisions))
            .sum::<f64>()
            / n as f64
    };
    let b_called = turns.iter().filter(|t| t.1.called).count();
    let (token_saving, time_saving) = (saving(|f| f.tokens as f64), saving(|f| f.wall_ms as f64));
    let mut r = AbReport {
        turns: n,
        a_matched,
        b_matched,
        token_saving,
        time_saving,
        decisions_removed: removed,
        b_called,
        stopped: None,
        spent_usd: sum(|f| f.usd, 0) + sum(|f| f.usd, 1),
        pass: false,
        why: String::new(),
        back_to_backlog: None,
    };
    if n == 0 {
        r.why = "no turn was replayed in both arms".into();
    } else if b_called == 0 {
        r.why = "the model never called the tool in arm B".into();
        r.back_to_backlog = Some(format!(
            "the tool was loaded in arm B of {n} replayed turns and the model never called it: \
             the motif is real, but the tool's shape or its description is wrong"
        ));
    } else if b_matched < a_matched {
        r.why = format!("B matched the recording in {b_matched} of {n} turns, A in {a_matched}");
    } else if token_saving < MIN_SAVING && time_saving < MIN_SAVING {
        r.why = format!(
            "B saved {:.0}% of the tokens and {:.0}% of the time: neither is {:.0}%",
            token_saving * 100.0,
            time_saving * 100.0,
            MIN_SAVING * 100.0
        );
    } else if removed < MIN_DECISIONS_REMOVED {
        r.why = format!("B removed {removed:.1} model decisions an occurrence: {MIN_DECISIONS_REMOVED} are needed");
    } else {
        r.pass = true;
        r.why = format!(
            "B matched {b_matched} of {n}, saved {:.0}% of the tokens and {:.0}% of the time, \
             and removed {removed:.1} decisions an occurrence",
            token_saving * 100.0,
            time_saving * 100.0
        );
    }
    r
}

/// What one mint may spend on arms, and the arm's own cap.
pub struct Caps {
    /// `[self.mint] usd`: this mint's whole replay.
    pub mint_usd: f64,
    /// `[route_bench] whole_usd`: one arm of one turn.
    pub arm_usd: f64,
    /// `[self.mint] replay_turns`.
    pub turns: usize,
}

/// Run the A/B replay over `check` (the check half's turn ids), reserving
/// every arm on `budget` (on the branch `branch`) before it runs.
pub async fn ab_replay<A: Arms>(
    arms: &A,
    budget: &Budget,
    branch: &str,
    check: &[String],
    caps: &Caps,
) -> Result<AbReport> {
    let mut done: Vec<(ArmFacts, ArmFacts)> = Vec::new();
    let (mut spent, mut stopped) = (0.0f64, None);
    'turns: for turn in check.iter().take(caps.turns) {
        let mut pair: Vec<ArmFacts> = Vec::new();
        for arm in [Arm::A, Arm::B] {
            let left = caps.mint_usd - spent;
            if left <= 0.0 {
                stopped = Some(format!("[self.mint] usd (${:.2}) is spent", caps.mint_usd));
                break 'turns;
            }
            let ask = caps.arm_usd.min(left);
            let held = match budget.reserve("mint", Some(branch), ask)? {
                Reserved::Granted(r) => r,
                Reserved::Stopped(s) => {
                    stopped = Some(s.to_string());
                    break 'turns;
                }
            };
            let ran = arms.run(turn, arm, ask).await;
            let usd = ran.as_ref().map_or(ask, |f| f.usd);
            budget.settle(held, usd)?;
            spent += usd;
            match ran {
                Ok(f) => pair.push(f),
                Err(e) => {
                    stopped = Some(format!("{e:#}"));
                    break 'turns;
                }
            }
        }
        done.push((pair[0], pair[1]));
    }
    let mut r = judge(&done);
    r.stopped = stopped;
    r.spent_usd = spent;
    if r.stopped.is_some() && r.pass {
        // A refusal mid-way leaves a smaller sample: it still has to pass its
        // own rules, and says it was cut.
        r.why = format!("{} (stopped early)", r.why);
    }
    Ok(r)
}
