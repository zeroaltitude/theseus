//! `[routing]` (M5 step 25e): the switch's few knobs. Jev chooses the model and
//! its effort (route.v10, theseus-h66v4); this section holds what the daemon
//! keeps of routing around those answers: whether it acts, how long a turn waits
//! for the verdict, the exchanges a `short` context carries, the cache's one
//! hysteresis (`cold_switch_tokens`), the owner's corrections and a task's
//! checkpoint triggers. It acts only while `[judge]` is on, and `rung = "shadow"`
//! (or `[judge.packs."route.v12"] rung = "shadow"`) records the verdict and
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
    /// `rung` was `live` or `shadow` (record the verdict, route nothing),
    /// retired as a second knob beside the route pack's own
    /// `[judge.packs."<pack>"] rung` (issue 44): read, warned of
    /// ([`Self::trimmed`]), ignored and never written back.
    #[serde(default, skip_serializing)]
    rung: Option<toml::Value>,
    /// How long the turn waits for the verdict after its first compile ends.
    #[serde(default = "max_wait_ms")]
    pub max_wait_ms: u64,
    /// A switch of the session's profile waits for a second turn in a row that
    /// agrees once the session carries this many tokens of its own context or
    /// more: above it, the cache a switch leaves cold costs more.
    ///
    /// **The session's own context, not the request's size** (theseus-ataas):
    /// the compile's estimate less the universal prefix — the tool definitions
    /// and the header block, which every session of every place class carries
    /// identically. They are there before a conversation starts and no
    /// conversation adds to them, and counting them put an ordinary turn
    /// within single tokens of this bound, where a few more characters of the
    /// model-facing tools note decided which model answered.
    #[serde(default = "cold_switch_tokens")]
    pub cold_switch_tokens: u64,
    /// A running task's model ranges (theseus-ypnxt.9).
    #[serde(default)]
    pub task_checkpoints: TaskCheckpoints,
    /// The keys Jev's choice retired (route.v10, theseus-h66v4) and the
    /// corrections section (`[routing.corrections]`: reading the owner's words
    /// for a correction, and the layer that steered close messages, are gone;
    /// a press, a reaction and `theseus judge correct` remain): read, warned
    /// of ([`Self::retired`]), ignored and never written back. `classes` is
    /// the table of profiles per class, with its `cn_*`, `final` and
    /// `first_pass` keys. `trivial_context_turns` was the detour's, and
    /// the detour is gone.
    #[serde(default, skip_serializing)]
    corrections: Option<toml::Value>,
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
    trivial_context_turns: Option<toml::Value>,
    #[serde(default, skip_serializing)]
    classes: Option<toml::Table>,
}

fn yes() -> bool {
    true
}
fn max_wait_ms() -> u64 {
    200
}
fn cold_switch_tokens() -> u64 {
    30_000
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rung: None,
            max_wait_ms: max_wait_ms(),
            cold_switch_tokens: cold_switch_tokens(),
            corrections: None,
            task_checkpoints: TaskCheckpoints::default(),
            switch_confidence: None,
            effort_bounds: None,
            high_effort_confidence: None,
            effort_confidence: None,
            effort_floor_confidence: None,
            dear_tier_max_tokens: None,
            trivial_context_turns: None,
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
        Ok(())
    }

    /// The retired keys this section sets, by the name the config gives
    /// them: what [`crate::Config::parse`] warns of, once.
    pub fn retired(&self) -> Vec<String> {
        let keys = [
            ("corrections", self.corrections.is_some()),
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
            (
                "trivial_context_turns",
                self.trivial_context_turns.is_some(),
            ),
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

    /// The retired `rung` said `shadow`: the route pack recorded its verdict
    /// and routed nothing, which it no longer does.
    pub fn was_shadow(&self) -> bool {
        self.rung.as_ref().and_then(toml::Value::as_str) == Some("shadow")
    }

    /// The keys the trim of unused modes retired (issue 44) that this
    /// section sets: what [`crate::Config::parse`] warns of, once.
    pub fn trimmed(&self) -> Vec<String> {
        self.rung.iter().map(|_| "routing.rung".into()).collect()
    }

    /// What the route pack may do: the judge's mode for it, off when this
    /// section is disabled.
    pub fn pack_mode(&self, judge: PackMode) -> PackMode {
        if !self.enabled {
            return PackMode::Off;
        }
        judge
    }
}

/// The template's `[routing]`, un-commented: its defaults.
#[cfg(test)]
pub(crate) fn the_templates_routing_section(cfg: &crate::Config) {
    let r = &cfg.routing;
    assert!(r.enabled);
    assert!(r.trimmed().is_empty(), "the template sets no retired key");
    assert_eq!((r.max_wait_ms, r.cold_switch_tokens), (200, 30_000));
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
                    trivial_context_turns = 2\n\
                    [classes.deep_coding]\nprofiles = [\"sonnet\", \"opus\"]\n\
                    [classes.trivial]\nprofiles = [\"haiku\"]\ncn_draft = \"dsflash\"\n\
                    [task_checkpoints]\nmax_switches = 99\n\
                    [corrections]\nsimilarity = 0.7\n";
        let c = cfg(text).unwrap();
        c.validate().unwrap();
        assert_eq!(
            c.retired(),
            [
                "routing.corrections",
                "routing.switch_confidence",
                "routing.effort_bounds",
                "routing.high_effort_confidence",
                "routing.effort_confidence",
                "routing.effort_floor_confidence",
                "routing.dear_tier_max_tokens",
                "routing.trivial_context_turns",
                "routing.classes.deep_coding",
                "routing.classes.trivial",
                "routing.task_checkpoints.max_switches",
            ]
        );
        let written = toml::to_string(&c).unwrap();
        for gone in [
            "switch_confidence",
            "classes",
            "max_switches",
            "effort_bounds",
        ] {
            assert!(!written.contains(gone), "{gone} written back: {written}");
        }
        assert!(cfg("").unwrap().retired().is_empty());
        assert!(
            cfg("[classes.poetry]\nprofiles = []").unwrap().retired() == ["routing.classes.poetry"],
            "a class of the old table's no longer refused, only retired"
        );
    }

    #[test]
    fn a_wait_past_five_seconds_is_refused() {
        assert!(cfg("max_wait_ms = 5000").unwrap().validate().is_ok());
        let e = cfg("max_wait_ms = 5001").unwrap().validate().unwrap_err();
        assert!(format!("{e:#}").contains("routing.max_wait_ms"));
    }

    /// The corrections section is gone with the reading of the owner's words
    /// and the layer: a settings file that still sets it loads, whatever its
    /// values were (even ones the old bounds refused), is named by
    /// `retired()` and is never written back.
    #[test]
    fn an_old_corrections_section_loads_warns_and_is_never_written_back() {
        let c = cfg("[corrections]\nenabled = false\nsimilarity = 1.5\nmax_entries = 0\n").unwrap();
        c.validate().unwrap();
        assert_eq!(c.retired(), ["routing.corrections"]);
        assert!(!toml::to_string(&c).unwrap().contains("corrections"));
        assert!(cfg("").unwrap().retired().is_empty());
    }

    #[test]
    fn the_section_lowers_the_packs_mode_and_never_raises_it() {
        let on = RoutingConfig::default();
        assert_eq!(on.pack_mode(PackMode::Live), PackMode::Live);
        assert_eq!(on.pack_mode(PackMode::Shadow), PackMode::Shadow);
        assert_eq!(on.pack_mode(PackMode::Off), PackMode::Off);
        let off = cfg("enabled = false").unwrap();
        assert_eq!(off.pack_mode(PackMode::Live), PackMode::Off);
    }
}
