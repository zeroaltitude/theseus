//! `[tools.defer]` (theseus-7m7w.2): which tool families a request loads.
//!
//! Every model call reads the whole tool set, and most places use a few
//! families of it. A family a place does not need is declared deferred
//! (`defer_loading` on Anthropic's API, where the model finds it by search) or
//! left out with a `tools_find` to ask for it (anywhere else). The loaded set
//! per place kind starts from `tools_defer::defaults` and is learned from the
//! ledger's use: a family used in at least `load_share` of a place kind's turns
//! over the last `window_days` loads, and one unused for `hide_after_days`
//! hides. It is recomputed only where the conversation recompiles anyway.

use serde::{Deserialize, Serialize};

/// `[tools.defer]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeferConfig {
    /// Off: every offered tool loads, as before.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Families that always load, in every place and whatever the ledger says.
    #[serde(default = "always_loaded")]
    pub always_loaded: Vec<String>,
    /// A family used in at least this share of a place kind's turns over the
    /// window loads there.
    #[serde(default = "load_share")]
    pub load_share: f64,
    /// The days of ledger the share is counted over.
    #[serde(default = "window_days")]
    pub window_days: u32,
    /// A family loaded by default and unused this many days hides.
    #[serde(default = "hide_after_days")]
    pub hide_after_days: u32,
    /// The hard ceiling on a first request that admitted families raise, as a
    /// multiple of the place kind's own first-request budget (theseus-xlri5):
    /// a family the message names, or Jev judges relevant, loads past the
    /// budget by exactly its own size, while the whole stays at or under
    /// `budget × admit_ceiling`. 2.0 is the smallest round multiple that
    /// admits the largest family, the tasks' (about 6,000 tokens), beside a cli
    /// detour's 7,300-token base with room for an AWS family and the
    /// terminals (7,300 + 6,000 + 1,271 + 1,264 = 15,835, under 16,000); at
    /// 1.5 (12,000) the tasks could never load at the cli. A multiple keeps
    /// one set of numbers, the keel's budgets. It is a ceiling, not a target.
    /// 1.0 is the cap as it was: the rollback. It is an ordinary prompt's
    /// (theseus-d9je7, `turn::tier`); a small one (`trivial`) takes
    /// `admit_ceiling_small`, a deep one (`sophisticated`, `deep_coding`,
    /// `review`) `admit_ceiling_deep`. The three alike are the rollback.
    #[serde(default = "admit_ceiling")]
    pub admit_ceiling: f64,
    #[serde(default = "admit_ceiling_small")]
    pub admit_ceiling_small: f64,
    #[serde(default = "admit_ceiling_deep")]
    pub admit_ceiling_deep: f64,
    /// Jev judges each hidden family's relevance to the prompt in focus, once
    /// a prompt (`families.v1`); off, only the families a message names are
    /// admitted: the rollback.
    #[serde(default = "judge_families")]
    pub judge_families: bool,
    /// The most a turn waits for that judgment, counted from its start at the
    /// inbound point (as `[routing] max_wait_ms`); a later answer is dropped.
    #[serde(default = "judge_wait_ms")]
    pub judge_wait_ms: u64,
}

fn enabled() -> bool {
    true
}

fn always_loaded() -> Vec<String> {
    vec!["files".into(), "commands".into()]
}

fn load_share() -> f64 {
    0.10
}

fn window_days() -> u32 {
    14
}

fn hide_after_days() -> u32 {
    14
}

fn admit_ceiling() -> f64 {
    2.0
}

fn admit_ceiling_small() -> f64 {
    1.0
}

/// The owner asked for more generous than the audit's 2.0 (theseus-d9je7).
fn admit_ceiling_deep() -> f64 {
    3.0
}

fn judge_families() -> bool {
    true
}

fn judge_wait_ms() -> u64 {
    200
}

impl Default for DeferConfig {
    fn default() -> Self {
        Self {
            enabled: enabled(),
            always_loaded: always_loaded(),
            load_share: load_share(),
            window_days: window_days(),
            hide_after_days: hide_after_days(),
            admit_ceiling: admit_ceiling(),
            admit_ceiling_small: admit_ceiling_small(),
            admit_ceiling_deep: admit_ceiling_deep(),
            judge_families: judge_families(),
            judge_wait_ms: judge_wait_ms(),
        }
    }
}

impl DeferConfig {
    /// A message for the first thing wrong with the section, if any.
    pub fn check(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.load_share) {
            return Err(format!(
                "[tools.defer] load_share = {} must be between 0 and 1",
                self.load_share
            ));
        }
        if self.window_days == 0 || self.hide_after_days == 0 {
            return Err("[tools.defer] window_days and hide_after_days must be at least 1".into());
        }
        for (key, v) in [
            ("admit_ceiling", self.admit_ceiling),
            ("admit_ceiling_small", self.admit_ceiling_small),
            ("admit_ceiling_deep", self.admit_ceiling_deep),
        ] {
            if !(1.0..=3.0).contains(&v) {
                return Err(format!(
                    "[tools.defer] {key} = {v} must be between 1.0 (the cap as it was) and 3.0"
                ));
            }
        }
        if self.judge_wait_ms > 2_000 {
            return Err(format!(
                "[tools.defer] judge_wait_ms = {} must be at most 2000",
                self.judge_wait_ms
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[tools.defer]` is the default, and a note may change a
    /// key without naming the others.
    #[test]
    fn the_templates_defer_section_is_the_default() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.tools.defer, DeferConfig::default());
        assert!(cfg.tools.defer.enabled);
        assert_eq!(cfg.tools.defer.always_loaded, ["files", "commands"]);
        assert_eq!(cfg.tools.defer.admit_ceiling, 2.0);
        assert_eq!(cfg.tools.defer.admit_ceiling_small, 1.0);
        assert_eq!(cfg.tools.defer.admit_ceiling_deep, 3.0);
        assert!(cfg.tools.defer.judge_families);
        assert_eq!(cfg.tools.defer.judge_wait_ms, 200);
    }

    /// theseus-xlri5: the ceiling is a multiple from 1.0 (the cap as it was) to 3.0, and the
    /// judgment's wait at most two seconds.
    #[test]
    fn a_ceiling_or_a_wait_out_of_bounds_is_refused() {
        for admit_ceiling in [0.5, 3.5] {
            let bad = DeferConfig {
                admit_ceiling,
                ..Default::default()
            };
            assert!(bad.check().unwrap_err().contains("admit_ceiling"));
            let bad = DeferConfig {
                admit_ceiling_deep: admit_ceiling,
                ..Default::default()
            };
            assert!(bad.check().unwrap_err().contains("admit_ceiling_deep"));
        }
        let bad = DeferConfig {
            judge_wait_ms: 5_000,
            ..Default::default()
        };
        assert!(bad.check().unwrap_err().contains("judge_wait_ms"));
        for admit_ceiling in [1.0, 3.0] {
            let ok = DeferConfig {
                admit_ceiling,
                judge_wait_ms: 2_000,
                ..Default::default()
            };
            assert!(ok.check().is_ok());
        }
    }

    #[test]
    fn a_share_outside_zero_to_one_is_refused() {
        let bad = DeferConfig {
            load_share: 1.5,
            ..Default::default()
        };
        assert!(bad.check().is_err());
        let bad = DeferConfig {
            window_days: 0,
            ..Default::default()
        };
        assert!(bad.check().is_err());
        assert!(DeferConfig::default().check().is_ok());
    }
}
