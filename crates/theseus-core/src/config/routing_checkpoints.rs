//! `[routing.task_checkpoints]` (theseus-ypnxt.9): a running task's model
//! ranges. At a loop boundary a task asks route.v6 about its recent state
//! when it keeps failing, makes no progress, changes phase, or is about to
//! write its report, and the answer moves it up, or back down to its base, by
//! routing's own rules (`routing::checkpoint`). On by default; `enabled = false`
//! is the one switch, and a count of 0 is an error at load, not a silent off.

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The most moves a task may make (up or down) before only a step back down
/// is allowed, as the turn holds one slot for each (`turn::checkpoint_step`):
/// one more than this is the step down past the cap.
pub const MAX_SWITCHES_LIMIT: u32 = 7;

/// `[routing.task_checkpoints]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCheckpoints {
    /// On by default: acts only where `[routing]` acts and the task has no profile pinned.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// This many loops in a row that each had a failed tool result ask a checkpoint.
    #[serde(default = "failing_streak")]
    pub failing_streak: u32,
    /// This many loops in a row with no progress signal ask one.
    #[serde(default = "quiet_loops")]
    pub quiet_loops: u32,
    /// At most one verdict per this many loops, whatever the triggers say.
    #[serde(default = "min_loops_between")]
    pub min_loops_between: u32,
    /// At most this many moves per task, up or down; a step back down to its
    /// base is allowed past it.
    #[serde(default = "max_switches")]
    pub max_switches: u32,
}

fn yes() -> bool {
    true
}
fn failing_streak() -> u32 {
    3
}
fn quiet_loops() -> u32 {
    8
}
fn min_loops_between() -> u32 {
    4
}
fn max_switches() -> u32 {
    2
}

impl Default for TaskCheckpoints {
    fn default() -> Self {
        Self {
            enabled: true,
            failing_streak: failing_streak(),
            quiet_loops: quiet_loops(),
            min_loops_between: min_loops_between(),
            max_switches: max_switches(),
        }
    }
}

impl TaskCheckpoints {
    pub fn validate(&self) -> Result<()> {
        for (key, n) in [
            ("failing_streak", self.failing_streak),
            ("quiet_loops", self.quiet_loops),
            ("min_loops_between", self.min_loops_between),
            ("max_switches", self.max_switches),
        ] {
            if n == 0 {
                anyhow::bail!(
                    "routing.task_checkpoints.{key} must be at least 1 (`enabled = false` turns checkpoints off)"
                );
            }
        }
        if self.max_switches > MAX_SWITCHES_LIMIT {
            anyhow::bail!(
                "routing.task_checkpoints.max_switches must be at most {MAX_SWITCHES_LIMIT}: a turn holds a model slot for each move"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RoutingConfig;

    #[test]
    fn the_defaults_are_on_and_a_sparse_note_holds_none() {
        let r: RoutingConfig = toml::from_str("").unwrap();
        assert_eq!(r.task_checkpoints, TaskCheckpoints::default());
        let c = &r.task_checkpoints;
        assert_eq!((c.enabled, c.failing_streak, c.quiet_loops), (true, 3, 8));
        assert_eq!((c.min_loops_between, c.max_switches), (4, 2));
        assert!(r.validate(|_| false).is_ok());
    }

    #[test]
    fn a_count_of_zero_is_an_error_and_an_unknown_key_is_refused() {
        for key in [
            "failing_streak",
            "quiet_loops",
            "min_loops_between",
            "max_switches",
        ] {
            let r: RoutingConfig =
                toml::from_str(&format!("[task_checkpoints]\n{key} = 0")).unwrap();
            let e = r.validate(|_| false).unwrap_err().to_string();
            assert!(e.contains(key) && e.contains("enabled = false"), "{e}");
        }
        let r: RoutingConfig = toml::from_str("[task_checkpoints]\nenabled = false").unwrap();
        assert!(
            r.validate(|_| false).is_ok(),
            "off is `enabled`, not a zero"
        );
        assert!(toml::from_str::<RoutingConfig>("[task_checkpoints]\nloud = 1").is_err());
        let r: RoutingConfig = toml::from_str("[task_checkpoints]\nmax_switches = 8").unwrap();
        assert!(r.validate(|_| false).is_err(), "the slots bound it");
    }
}
