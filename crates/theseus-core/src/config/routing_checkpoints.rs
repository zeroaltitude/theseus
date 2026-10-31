//! `[routing.task_checkpoints]` (theseus-ypnxt.9): a running task's model
//! ranges. At a loop boundary a task asks the route pack about its recent state
//! when it keeps failing, makes no progress, changes phase, or is about to
//! write its report, and the answer moves it up, or back down to its base, by
//! Jev's verdict (`routing::checkpoint`). On by default; `enabled = false`
//! is the one switch, and a count of 0 is an error at load, not a silent off.

use anyhow::Result;
use serde::{Deserialize, Serialize};

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
    /// Retired (route.v10, theseus-h66v4): a task has no cap on its moves.
    /// Read, warned of (`RoutingConfig::retired`), ignored and never written.
    #[serde(default, skip_serializing)]
    pub(crate) max_switches: Option<u32>,
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

impl Default for TaskCheckpoints {
    fn default() -> Self {
        Self {
            enabled: true,
            failing_streak: failing_streak(),
            quiet_loops: quiet_loops(),
            min_loops_between: min_loops_between(),
            max_switches: None,
        }
    }
}

impl TaskCheckpoints {
    pub fn validate(&self) -> Result<()> {
        for (key, n) in [
            ("failing_streak", self.failing_streak),
            ("quiet_loops", self.quiet_loops),
            ("min_loops_between", self.min_loops_between),
        ] {
            if n == 0 {
                anyhow::bail!(
                    "routing.task_checkpoints.{key} must be at least 1 (`enabled = false` turns checkpoints off)"
                );
            }
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
        assert_eq!((c.min_loops_between, c.max_switches), (4, None));
        assert!(r.validate().is_ok());
    }

    #[test]
    fn a_count_of_zero_is_an_error_and_an_unknown_key_is_refused() {
        for key in [
            "failing_streak",
            "quiet_loops",
            "min_loops_between",
        ] {
            let r: RoutingConfig =
                toml::from_str(&format!("[task_checkpoints]\n{key} = 0")).unwrap();
            let e = r.validate().unwrap_err().to_string();
            assert!(e.contains(key) && e.contains("enabled = false"), "{e}");
        }
        let r: RoutingConfig = toml::from_str("[task_checkpoints]\nenabled = false").unwrap();
        assert!(
            r.validate().is_ok(),
            "off is `enabled`, not a zero"
        );
        assert!(toml::from_str::<RoutingConfig>("[task_checkpoints]\nloud = 1").is_err());
        let r: RoutingConfig = toml::from_str("[task_checkpoints]\nmax_switches = 8").unwrap();
        assert!(r.validate().is_ok(), "retired: no slot bounds it any more");
    }
}
