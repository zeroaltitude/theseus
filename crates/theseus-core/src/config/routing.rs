//! `[routing]` (M5 step 25e): the switch's few knobs. Jev chooses the model and
//! its effort (route.v10, theseus-h66v4); this section holds what the daemon
//! keeps of routing around those answers: whether it acts, how long a turn waits
//! for the verdict, the exchanges a `short` context carries, the cache's one
//! hysteresis (`cold_switch_tokens`), the owner's corrections and a task's
//! checkpoint triggers. It acts only while `[judge]` is on, and `rung = "shadow"`
//! (or `[judge.packs."route.v10"] rung = "shadow"`) records the verdict and
//! routes nothing. Every key has a default, so a sparse note holds only what
//! differs.
//!
//! The keys of the routing that Jev's choice replaced (a table of profiles per
//! class, the confidence bars, the effort bounds and bars, the dear tier's
//! guard, a task's switch cap) still load: they warn once ([`RoutingConfig::retired`]),
//! do nothing, and are never written back, for one release.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::PackMode;

pub use super::routing_checkpoints::TaskCheckpoints;

/// `[routing]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingConfig {
    /// On by default: acts only while `[judge]` is on.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// `live` routes; `shadow` records the verdict and routes nothing: the route pack's rung.
    #[serde(default = "live", rename = "rung")]
    pub mode: RoutingMode,
    /// How long the turn waits for the verdict after its first compile ends.
    #[serde(default = "max_wait_ms")]
    pub max_wait_ms: u64,
    /// The exchanges a `short` context's request carries before the message.
    #[serde(default = "trivial_context_turns")]
    pub trivial_context_turns: u32,
    /// A switch of the session's profile at a compile of this many estimated
    /// tokens or more waits for a second turn in a row that agrees: above
    /// it, the cache a switch leaves cold costs more.
    #[serde(default = "cold_switch_tokens")]
    pub cold_switch_tokens: u64,
    /// The owner's corrections of routing (theseus-q31l).
    #[serde(default)]
    pub corrections: CorrectionsConfig,
    /// A running task's model ranges (theseus-ypnxt.9).
    #[serde(default)]
    pub task_checkpoints: TaskCheckpoints,
    /// The keys Jev's choice retired (route.v10, theseus-h66v4): read, warned
    /// of ([`Self::retired`]), ignored and never written back. `classes` is
    /// the table of profiles per class, with its `cn_*`, `final` and
    /// `first_pass` keys.
    #[serde(default, skip_serializing)]
    switch_confidence: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    effort_bounds: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    high_effort_confidence: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    effort_confidence: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    effort_floor_confidence: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    dear_tier_max_tokens: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    classes: Option<toml::Table>,
}

/// `[routing.corrections]` (theseus-q31l): the owner's corrections in a
/// private place ("that should have been on fable", a reaction, a press),
/// and the live layer that steers a close message the same way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrectionsConfig {
    /// Read the owner's words as a correction, and let the layer steer.
    /// Off: words are messages and the layer steers nothing; `route.correct`
    /// (a reaction, a press, the CLI) still labels and switches.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// A new message whose content words share at least this much with a
    /// corrected one's (Jaccard) runs where the owner said.
    #[serde(default = "similarity")]
    pub similarity: f64,
    /// The layer's bound: the oldest out past it.
    #[serde(default = "max_entries")]
    pub max_entries: u32,
}

impl Default for CorrectionsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            similarity: similarity(),
            max_entries: max_entries(),
        }
    }
}

/// `[routing] rung`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Live,
    Shadow,
}

fn yes() -> bool {
    true
}
fn live() -> RoutingMode {
    RoutingMode::Live
}
fn max_wait_ms() -> u64 {
    200
}
fn trivial_context_turns() -> u32 {
    2
}
fn cold_switch_tokens() -> u64 {
    30_000
}
fn similarity() -> f64 {
    0.5
}
fn max_entries() -> u32 {
    64
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: live(),
            max_wait_ms: max_wait_ms(),
            trivial_context_turns: trivial_context_turns(),
            cold_switch_tokens: cold_switch_tokens(),
            corrections: CorrectionsConfig::default(),
            task_checkpoints: TaskCheckpoints::default(),
            switch_confidence: None,
            effort_bounds: None,
            high_effort_confidence: None,
            effort_confidence: None,
            effort_floor_confidence: None,
            dear_tier_max_tokens: None,
            classes: None,
        }
    }
}

impl RoutingConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The checks as the config loads.
    pub fn validate(&self) -> Result<()> {
        if self.max_wait_ms > 5_000 {
            anyhow::bail!(
                "routing.max_wait_ms must be at most 5000: the turn waits that long for its verdict"
            );
        }
        self.task_checkpoints.validate()?;
        let c = &self.corrections;
        if !(c.similarity > 0.0 && c.similarity <= 1.0) {
            anyhow::bail!("routing.corrections.similarity must be above 0 and at most 1");
        }
        if !(1..=4096).contains(&c.max_entries) {
            anyhow::bail!("routing.corrections.max_entries must be 1 to 4096");
        }
        Ok(())
    }

    /// The retired keys this section sets, by the name the config gives
    /// them: what [`crate::Config::parse`] warns of, once.
    pub fn retired(&self) -> Vec<String> {
        let keys = [
            ("switch_confidence", self.switch_confidence.is_some()),
            ("effort_bounds", self.effort_bounds.is_some()),
            (
                "high_effort_confidence",
                self.high_effort_confidence.is_some(),
            ),
            ("effort_confidence", self.effort_confidence.is_some()),
            (
                "effort_floor_confidence",
                self.effort_floor_confidence.is_some(),
            ),
            ("dear_tier_max_tokens", self.dear_tier_max_tokens.is_some()),
        ];
        let mut out: Vec<String> = keys
            .iter()
            .filter(|(_, set)| *set)
            .map(|(k, _)| format!("routing.{k}"))
            .collect();
        if let Some(classes) = &self.classes {
            out.extend(classes.keys().map(|c| format!("routing.classes.{c}")));
            if classes.is_empty() {
                out.push("routing.classes".into());
            }
        }
        if self.task_checkpoints.max_switches.is_some() {
            out.push("routing.task_checkpoints.max_switches".into());
        }
        out
    }

    /// What the route pack may do: the judge's mode for it, lowered by this
    /// section (off when disabled, shadow when `mode = "shadow"`).
    pub fn pack_mode(&self, judge: PackMode) -> PackMode {
        if !self.enabled {
            return PackMode::Off;
        }
        match self.mode {
            RoutingMode::Live => judge,
            RoutingMode::Shadow => judge.min(PackMode::Shadow),
        }
    }
}

/// The template's `[routing]`, un-commented: its defaults.
#[cfg(test)]
pub(crate) fn the_templates_routing_section(cfg: &crate::Config) {
    let r = &cfg.routing;
    assert!(r.enabled);
    assert_eq!(r.mode, RoutingMode::Live);
    assert_eq!(
        (r.max_wait_ms, r.trivial_context_turns, r.cold_switch_tokens),
        (200, 2, 30_000)
    );
    assert_eq!(r.corrections, CorrectionsConfig::default());
    assert_eq!(r.task_checkpoints, TaskCheckpoints::default());
    assert!(r.retired().is_empty(), "the template sets no retired key");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Result<RoutingConfig> {
        Ok(toml::from_str::<RoutingConfig>(text)?)
    }

    #[test]
    fn the_defaults_are_the_steps_table() {
        let d = cfg("").unwrap();
        assert_eq!(d, RoutingConfig::default());
        d.validate().unwrap();
        assert_eq!(
            (
                d.corrections.enabled,
                d.corrections.similarity,
                d.corrections.max_entries
            ),
            (true, 0.5, 64)
        );
        let c = cfg("[corrections]\nsimilarity = 0.7").unwrap();
        assert_eq!(
            (c.corrections.similarity, c.corrections.max_entries),
            (0.7, 64)
        );
        assert!(
            cfg("[corrections]\nthreshold = 0.7").is_err(),
            "an unknown key"
        );
        assert!(cfg("wait_ms = 1").is_err(), "an unknown key");
    }

    /// Every key Jev's choice retired (theseus-h66v4, row 6): it loads, is
    /// named by `retired()`, validates whatever its value was, and is never
    /// written back. The live config named three of them before the row.
    #[test]
    fn a_retired_key_loads_warns_and_is_never_written_back() {
        let text = "switch_confidence = 0.0\n\
                    effort_bounds = [\"medium\", \"high\"]\n\
                    high_effort_confidence = 0.40\n\
                    effort_confidence = 0.5\n\
                    effort_floor_confidence = 1.1\n\
                    dear_tier_max_tokens = 0\n\
                    [classes.deep_coding]\nprofiles = [\"sonnethi\", \"opus\"]\n\
                    [classes.trivial]\nprofiles = [\"haiku\"]\ncn_draft = \"dsflash\"\n\
                    [task_checkpoints]\nmax_switches = 99\n";
        let c = cfg(text).unwrap();
        c.validate().unwrap();
        assert_eq!(
            c.retired(),
            [
                "routing.switch_confidence",
                "routing.effort_bounds",
                "routing.high_effort_confidence",
                "routing.effort_confidence",
                "routing.effort_floor_confidence",
                "routing.dear_tier_max_tokens",
                "routing.classes.deep_coding",
                "routing.classes.trivial",
                "routing.task_checkpoints.max_switches",
            ]
        );
        let written = toml::to_string(&c).unwrap();
        for gone in ["switch_confidence", "classes", "max_switches", "effort_bounds"] {
            assert!(!written.contains(gone), "{gone} written back: {written}");
        }
        assert!(cfg("").unwrap().retired().is_empty());
        assert!(
            cfg("[classes.poetry]\nprofiles = []").unwrap().retired()
                == ["routing.classes.poetry"],
            "a class of the old table's no longer refused, only retired"
        );
    }

    #[test]
    fn a_wait_past_five_seconds_is_refused() {
        assert!(cfg("max_wait_ms = 5000").unwrap().validate().is_ok());
        let e = cfg("max_wait_ms = 5001").unwrap().validate().unwrap_err();
        assert!(format!("{e:#}").contains("routing.max_wait_ms"));
    }

    #[test]
    fn the_corrections_bounds_are_checked() {
        for bad in ["similarity = 0.0", "similarity = 1.5", "max_entries = 0", "max_entries = 5000"] {
            let c = cfg(&format!("[corrections]\n{bad}")).unwrap();
            assert!(c.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn the_section_lowers_the_packs_mode_and_never_raises_it() {
        let on = RoutingConfig::default();
        assert_eq!(on.pack_mode(PackMode::Live), PackMode::Live);
        assert_eq!(on.pack_mode(PackMode::Shadow), PackMode::Shadow);
        assert_eq!(on.pack_mode(PackMode::Off), PackMode::Off);
        let shadow = cfg("rung = \"shadow\"").unwrap();
        assert_eq!(shadow.pack_mode(PackMode::Live), PackMode::Shadow);
        let off = cfg("enabled = false").unwrap();
        assert_eq!(off.pack_mode(PackMode::Live), PackMode::Off);
    }
}
