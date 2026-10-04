//! `[judge]` (M5, step 23a; design `m5-judgment.md` §2.15): Jev's judgments,
//! off by default. Enabled, every pack this build wires in judges in shadow:
//! each judgment is made, priced, recorded, and acted on by nothing. The
//! config is a ceiling an agent cannot raise: `max_mode` and a pack's own
//! `mode` lower what a pack may do, and never raise it above what the ladder
//! (step 26a) has given it, which in 23a is shadow for every pack.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// What a pack may do, lowest first: the config takes the lowest of its
/// ceilings and the pack's own mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackMode {
    /// Not called.
    Off,
    /// Called and recorded; nothing acts on it.
    Shadow,
    /// Acts for a share of sessions (step 26a).
    Canary,
    /// Acts everywhere (step 26a).
    Live,
}

impl PackMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PackMode::Off => "off",
            PackMode::Shadow => "shadow",
            PackMode::Canary => "canary",
            PackMode::Live => "live",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeConfig {
    /// Off by default: nothing is judged, and nothing reaches Jev.
    #[serde(default)]
    pub enabled: bool,
    /// The ceiling of every pack: `off` calls nothing, `shadow` acts on
    /// nothing.
    #[serde(default = "live")]
    pub max_mode: PackMode,
    /// The `[secrets]` entry holding the Jev key, read when a judgment is
    /// made, never before serving.
    #[serde(default = "key_secret")]
    pub key_secret: String,
    /// Jev's endpoint. Tests and the bench point it at a fake.
    #[serde(default = "api_base")]
    pub api_base: String,
    /// Calls in flight at once; a shadow judgment with none free is shed.
    #[serde(default = "max_in_flight")]
    pub max_in_flight: usize,
    #[serde(default = "connect_secs")]
    pub connect_secs: u64,
    /// The whole call, connect included.
    #[serde(default = "total_secs")]
    pub total_secs: u64,
    /// What shadow judgments may spend in a local day, in dollars: at the
    /// limit, shadow pauses until midnight (§2.6).
    #[serde(default = "shadow_limit")]
    pub shadow_limit_usd_per_day: f64,
    /// `[judge.packs."<pack>"]`, by the pack's name (`loop.v1`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packs: BTreeMap<String, JudgePackConfig>,
    /// `[judge.signals]`: CONTINUE's candidate signals (M5 25b), computed at
    /// every compile whether or not the judge is on.
    #[serde(default)]
    pub signals: SignalsConfig,
}

/// `[judge.signals]` (design §2.15): where CONTINUE's candidate signals fire.
/// Tests and live checks shorten them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalsConfig {
    /// A new input more than this many minutes after the node before it is
    /// a dormancy gap. 0 fires at any gap.
    #[serde(default = "dormancy_minutes")]
    pub dormancy_minutes: u64,
    /// The tail passing this share of the window fires, and then each
    /// further quarter of it.
    #[serde(default = "tail_band")]
    pub tail_band: f64,
}

impl Default for SignalsConfig {
    fn default() -> Self {
        Self {
            dormancy_minutes: dormancy_minutes(),
            tail_band: tail_band(),
        }
    }
}

fn dormancy_minutes() -> u64 {
    360
}
fn tail_band() -> f64 {
    0.5
}

/// One pack's lines: a lower mode, and its shadow sampling share.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgePackConfig {
    /// A ceiling on the pack's mode: it lowers, never raises.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<PackMode>,
    /// The share of eligible events judged in shadow, 0 to 1 (the pack's
    /// own share when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample: Option<f64>,
}

fn live() -> PackMode {
    PackMode::Live
}
fn key_secret() -> String {
    "jev_api_key".into()
}
fn api_base() -> String {
    theseus_judge::client::DEFAULT_API_BASE.into()
}
fn max_in_flight() -> usize {
    8
}
fn connect_secs() -> u64 {
    2
}
fn total_secs() -> u64 {
    5
}
fn shadow_limit() -> f64 {
    1.0
}

impl Default for JudgeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_mode: live(),
            key_secret: key_secret(),
            api_base: api_base(),
            max_in_flight: max_in_flight(),
            connect_secs: connect_secs(),
            total_secs: total_secs(),
            shadow_limit_usd_per_day: shadow_limit(),
            packs: BTreeMap::new(),
            signals: SignalsConfig::default(),
        }
    }
}

impl JudgeConfig {
    /// The checks as the config loads. Pack names and figures are checked
    /// whether or not the judge is enabled; the key's entry only when it is.
    pub fn validate(&self, secrets: &BTreeMap<String, String>) -> Result<()> {
        if self.enabled && !secrets.contains_key(&self.key_secret) {
            anyhow::bail!(
                "judge.key_secret = {:?} has no matching entry under [secrets]",
                self.key_secret
            );
        }
        if self.max_in_flight == 0 {
            anyhow::bail!("judge.max_in_flight must be at least 1");
        }
        if self.connect_secs == 0 || self.total_secs < self.connect_secs {
            anyhow::bail!(
                "judge.connect_secs must be at least 1, and judge.total_secs at least connect_secs"
            );
        }
        let limit = self.shadow_limit_usd_per_day;
        if !limit.is_finite() || limit < 0.0 {
            anyhow::bail!("judge.shadow_limit_usd_per_day must be zero or more, in dollars");
        }
        let band = self.signals.tail_band;
        if band.is_nan() || band <= 0.0 || band > 1.0 {
            anyhow::bail!(
                "judge.signals.tail_band must be above 0 and at most 1, a share of the window"
            );
        }
        for (name, p) in &self.packs {
            if theseus_judge::pack::by_name(name).is_none() {
                anyhow::bail!(
                    "judge.packs.{name:?} is no pack this build has (packs are named as `loop.v1`)"
                );
            }
            if let Some(s) = p.sample {
                if !(0.0..=1.0).contains(&s) {
                    anyhow::bail!("judge.packs.{name:?}.sample must be between 0 and 1");
                }
            }
        }
        Ok(())
    }

    /// What `pack` may do: the lowest of the ladder's mode for it (`given`),
    /// `max_mode`, and its own line. Off whenever the judge is.
    pub fn mode_of(&self, pack: &str, given: PackMode) -> PackMode {
        if !self.enabled {
            return PackMode::Off;
        }
        let own = self
            .packs
            .get(pack)
            .and_then(|p| p.mode)
            .unwrap_or(PackMode::Live);
        given.min(self.max_mode).min(own)
    }

    /// The share of `pack`'s eligible events judged in shadow: its line's,
    /// else the pack's own.
    pub fn sample_of(&self, pack: &str, own: f64) -> f64 {
        self.packs
            .get(pack)
            .and_then(|p| p.sample)
            .unwrap_or(own)
            .clamp(0.0, 1.0)
    }
}

/// The template's `[judge]`, un-commented (`config.rs`'s
/// `example_template_uncommented_still_parses`): off, with its key's entry
/// in `[secrets]`, and the commented pack lines real.
#[cfg(test)]
pub(crate) fn the_templates_judge_section(cfg: &crate::Config) {
    let j = &cfg.judge;
    assert!(!j.enabled, "the judge is off by default");
    assert!(cfg.secrets.contains_key(&j.key_secret));
    assert_eq!(j.packs["loop.v1"].mode, Some(PackMode::Off));
    assert_eq!(j.packs["loop.v1"].sample, Some(0.5));
    assert_eq!(j.signals, SignalsConfig::default());
    j.validate(&cfg.secrets).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> Result<JudgeConfig> {
        Ok(toml::from_str::<JudgeConfig>(text)?)
    }

    /// Off by default, and the template's defaults are these.
    #[test]
    fn the_judge_is_off_by_default_and_the_template_says_so() {
        let d = JudgeConfig::default();
        assert!(!d.enabled);
        assert_eq!(d.mode_of("loop.v1", PackMode::Shadow), PackMode::Off);
        let example = crate::Config::example().judge;
        assert!(!example.enabled);
        assert_eq!(example.shadow_limit_usd_per_day, 1.0);
        assert_eq!(example.key_secret, "jev_api_key");
        assert_eq!(
            (example.signals.dormancy_minutes, example.signals.tail_band),
            (360, 0.5)
        );
        let t = crate::Config::EXAMPLE_TOML;
        assert!(t.contains("[judge]") && t.contains("[judge.packs."));
    }

    /// The config lowers a pack's mode and never raises it: a pack line or
    /// `max_mode` of `live` leaves a shadow pack in shadow, and `off` or a
    /// `shadow` ceiling lowers it.
    #[test]
    fn the_config_lowers_a_packs_mode_and_never_raises_it() {
        let on = |extra: &str| cfg(&format!("enabled = true\n{extra}")).unwrap();
        let shadow = PackMode::Shadow;
        assert_eq!(on("").mode_of("loop.v1", shadow), shadow);
        let raise = on("max_mode = \"live\"\n[packs.\"loop.v1\"]\nmode = \"live\"");
        assert_eq!(raise.mode_of("loop.v1", shadow), shadow, "never raised");
        let canary = on("[packs.\"loop.v1\"]\nmode = \"canary\"");
        assert_eq!(canary.mode_of("loop.v1", shadow), shadow);
        let off = on("[packs.\"loop.v1\"]\nmode = \"off\"");
        assert_eq!(off.mode_of("loop.v1", shadow), PackMode::Off);
        assert_eq!(
            off.mode_of("security.v1", shadow),
            shadow,
            "one pack's line"
        );
        let capped = on("max_mode = \"shadow\"");
        assert_eq!(capped.mode_of("loop.v1", PackMode::Live), shadow);
        let none = on("max_mode = \"off\"");
        assert_eq!(none.mode_of("loop.v1", shadow), PackMode::Off);
    }

    /// The loader's checks: an unknown key, a pack this build lacks, a
    /// sample outside 0 to 1, and an enabled judge with no key entry.
    #[test]
    fn the_judges_lines_are_checked_as_the_config_loads() {
        let secrets: BTreeMap<String, String> =
            [("jev_api_key".to_string(), "op://v/i/f".to_string())].into();
        assert!(cfg("enable = true").is_err(), "an unknown key");
        let bad = cfg("[packs.\"nope.v9\"]\nmode = \"off\"").unwrap();
        assert!(format!("{:#}", bad.validate(&secrets).unwrap_err()).contains("nope.v9"));
        let bad = cfg("[packs.\"loop.v1\"]\nsample = 1.5").unwrap();
        assert!(bad.validate(&secrets).is_err());
        let on = cfg("enabled = true").unwrap();
        on.validate(&secrets).unwrap();
        let e = on.validate(&BTreeMap::new()).unwrap_err();
        assert!(format!("{e:#}").contains("jev_api_key"));
        assert!(cfg("shadow_limit_usd_per_day = -1.0")
            .unwrap()
            .validate(&secrets)
            .is_err());
        let short = cfg("[signals]\ndormancy_minutes = 1\ntail_band = 0.25").unwrap();
        short.validate(&secrets).unwrap();
        assert_eq!(short.signals.dormancy_minutes, 1);
        assert!(cfg("[signals]\ndormancy = 1").is_err(), "an unknown key");
        for band in ["0.0", "1.5", "-0.5"] {
            let bad = cfg(&format!("[signals]\ntail_band = {band}")).unwrap();
            assert!(bad.validate(&secrets).is_err(), "{band}");
        }
        let s = cfg("[packs.\"loop.v1\"]\nsample = 0.25").unwrap();
        assert_eq!(s.sample_of("loop.v1", 1.0), 0.25);
        assert_eq!(s.sample_of("security.v1", 1.0), 1.0);
    }
}
