//! `[routing]` (M5 step 25e): Jev's model per interaction mode. The route
//! pack (`route.v2` since theseus-3okf; `route.v1` before it) asks, at
//! `inbound`, which mode a person's message needs, and the turn runs on that
//! mode's first usable profile. It acts only while `[judge]` is on, and
//! `mode = "shadow"` (or `[judge.packs."route.v2"] mode = "shadow"`) records
//! the verdict and routes nothing. Every key has a default, so a sparse note
//! holds only what differs.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::PackMode;

/// The profile name that means the usable profile cheapest for a short
/// turn at catalog prices (reserved: no profile may take it).
pub const CHEAPEST: &str = "cheapest";

/// `[routing]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingConfig {
    /// On by default: acts only while `[judge]` is on.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// `live` routes; `shadow` records the verdict and routes nothing.
    #[serde(default = "live")]
    pub mode: RoutingMode,
    /// How long the turn waits for the verdict after its first compile ends.
    #[serde(default = "max_wait_ms")]
    pub max_wait_ms: u64,
    /// The exchanges a detour's request (`trivial`, `quick`) carries before
    /// the message.
    #[serde(default = "trivial_context_turns")]
    pub trivial_context_turns: u32,
    /// A switch of the session's profile at a compile of this many estimated
    /// tokens or more waits for a second turn in a row that agrees: above
    /// it, the cache a switch leaves cold costs more.
    #[serde(default = "cold_switch_tokens")]
    pub cold_switch_tokens: u64,
    /// A verdict routes only at this confidence or more (detours included),
    /// unless its mode sets its own bar (`[routing.modes.<mode>]`).
    #[serde(default = "switch_confidence")]
    pub switch_confidence: f64,
    /// Each mode's profiles, in order: the first usable wins.
    #[serde(default)]
    pub modes: RoutingModes,
    /// The owner's corrections of routing (theseus-q31l).
    #[serde(default)]
    pub corrections: CorrectionsConfig,
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

/// `[routing] mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Live,
    Shadow,
}

/// `[routing.modes.<mode>]`, one per mode the route pack answers (`other`
/// is routed as `chat`; `quick` is route.v2's, theseus-3okf).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingModes {
    #[serde(default = "trivial")]
    pub trivial: ModeProfiles,
    #[serde(default = "quick")]
    pub quick: ModeProfiles,
    #[serde(default)]
    pub chat: ModeProfiles,
    #[serde(default = "sophisticated")]
    pub sophisticated: ModeProfiles,
    #[serde(default = "deep_coding")]
    pub deep_coding: ModeProfiles,
    #[serde(default = "routine_coding")]
    pub routine_coding: ModeProfiles,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModeProfiles {
    /// Profile names (or `cheapest`), in order; empty: the session's own.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// The confidence this mode's verdict needs to route (theseus-6n5j).
    /// Unset: `[routing] switch_confidence`, except `trivial`, whose bar is
    /// [`TRIVIAL_CONFIDENCE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switch_confidence: Option<f64>,
}

/// `trivial`'s bar when its table sets none (theseus-6n5j, the owner's
/// "about 40% for trivial only", 2026-10-04): a wrong trivial call is cheap,
/// since a detour carries only the last `trivial_context_turns` exchanges to
/// the trivial profile, for that turn alone.
pub const TRIVIAL_CONFIDENCE: f64 = 0.4;

/// The modes, in the pack's order.
pub const MODES: [&str; 6] = [
    "trivial",
    "quick",
    "chat",
    "sophisticated",
    "deep_coding",
    "routine_coding",
];

/// The modes whose turn is a detour (theseus-3okf; `trivial` alone before
/// it): that turn alone runs on the mode's profile, with the last
/// `trivial_context_turns` exchanges, and the session stays where it was, its
/// cache warm. A late verdict of one of them applies to no message
/// (`routing::carries`). Every other mode switches the session.
pub const DETOURS: [&str; 2] = ["trivial", "quick"];

/// Whether `mode`'s turn is a detour ([`DETOURS`]).
pub fn is_detour(mode: &str) -> bool {
    DETOURS.contains(&mode)
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
fn switch_confidence() -> f64 {
    0.6
}
fn similarity() -> f64 {
    0.5
}
fn max_entries() -> u32 {
    64
}
fn list(names: &[&str]) -> ModeProfiles {
    ModeProfiles {
        profiles: names.iter().map(|s| (*s).to_string()).collect(),
        switch_confidence: None,
    }
}
/// Haiku 5.5 first, then GLM-5.3 Flash, then the cheapest usable profile, so
/// a config without either still detours as it did before (theseus-3okf).
fn trivial() -> ModeProfiles {
    list(&["haiku", "glm", CHEAPEST])
}
/// Haiku 5.5 alone: a config without it runs a quick message on the
/// session's own profile, as it did while `chat` held them.
fn quick() -> ModeProfiles {
    list(&["haiku"])
}
fn sophisticated() -> ModeProfiles {
    list(&["opus", "fable"])
}
fn deep_coding() -> ModeProfiles {
    list(&["opus", "sonnet"])
}
/// Haiku 5.5 at high effort, then Sonnet 5.5 (theseus-3okf): a config
/// without `haikuhi` runs routine programming on `sonnet`, or the session's
/// own.
fn routine_coding() -> ModeProfiles {
    list(&["haikuhi", "sonnet"])
}

impl Default for RoutingModes {
    fn default() -> Self {
        Self {
            trivial: trivial(),
            quick: quick(),
            chat: ModeProfiles::default(),
            sophisticated: sophisticated(),
            deep_coding: deep_coding(),
            routine_coding: routine_coding(),
        }
    }
}

impl RoutingModes {
    /// A mode's table; `other` and any unknown mode are `chat`'s.
    pub fn table(&self, mode: &str) -> &ModeProfiles {
        match mode {
            "trivial" => &self.trivial,
            "quick" => &self.quick,
            "sophisticated" => &self.sophisticated,
            "deep_coding" => &self.deep_coding,
            "routine_coding" => &self.routine_coding,
            _ => &self.chat,
        }
    }

    /// A mode's profiles; `other` and any unknown mode are `chat`'s.
    pub fn of(&self, mode: &str) -> &[String] {
        &self.table(mode).profiles
    }
}

impl Default for RoutingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: live(),
            max_wait_ms: max_wait_ms(),
            trivial_context_turns: trivial_context_turns(),
            cold_switch_tokens: cold_switch_tokens(),
            switch_confidence: switch_confidence(),
            modes: RoutingModes::default(),
            corrections: CorrectionsConfig::default(),
        }
    }
}

impl RoutingConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The checks as the config loads. A profile a mode names need not be
    /// configured (the defaults name `haiku`, `haikuhi`, `opus` and `fable`,
    /// which a sparse note may lack): one that is not is skipped as unusable.
    pub fn validate(&self, profiles: impl Fn(&str) -> bool) -> Result<()> {
        let c = self.switch_confidence;
        if !(c > 0.0 && c <= 1.0) {
            anyhow::bail!("routing.switch_confidence must be above 0 and at most 1");
        }
        for m in MODES {
            if let Some(c) = self.modes.table(m).switch_confidence {
                if !(c > 0.0 && c <= 1.0) {
                    anyhow::bail!(
                        "routing.modes.{m}.switch_confidence must be above 0 and at most 1"
                    );
                }
            }
        }
        if self.max_wait_ms > 5_000 {
            anyhow::bail!(
                "routing.max_wait_ms must be at most 5000: the turn waits that long for its verdict"
            );
        }
        if profiles(CHEAPEST) {
            anyhow::bail!(
                "[profiles.{CHEAPEST}] takes a reserved name: [routing] uses it for the cheapest usable profile"
            );
        }
        for m in MODES {
            if let Some(p) = self.modes.of(m).iter().find(|p| p.trim().is_empty()) {
                anyhow::bail!("routing.modes.{m}.profiles has an empty name {p:?}");
            }
        }
        let c = &self.corrections;
        if !(c.similarity > 0.0 && c.similarity <= 1.0) {
            anyhow::bail!("routing.corrections.similarity must be above 0 and at most 1");
        }
        if !(1..=4096).contains(&c.max_entries) {
            anyhow::bail!("routing.corrections.max_entries must be 1 to 4096");
        }
        Ok(())
    }

    /// The confidence a verdict of `mode` needs to route: its mode's own
    /// bar, else trivial's [`TRIVIAL_CONFIDENCE`], else the section's
    /// `switch_confidence` (theseus-6n5j).
    pub fn confidence_for(&self, mode: &str) -> f64 {
        let own = self.modes.table(mode).switch_confidence;
        own.unwrap_or(match mode {
            "trivial" => TRIVIAL_CONFIDENCE,
            _ => self.switch_confidence,
        })
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

/// The template's `[routing]`, un-commented: its defaults, and the
/// profiles its modes name configured.
#[cfg(test)]
pub(crate) fn the_templates_routing_section(cfg: &crate::Config) {
    let r = &cfg.routing;
    assert!(r.enabled);
    assert_eq!(r.mode, RoutingMode::Live);
    assert_eq!(
        (r.max_wait_ms, r.trivial_context_turns, r.cold_switch_tokens),
        (200, 2, 30_000)
    );
    assert_eq!(r.switch_confidence, 0.6);
    for m in MODES {
        assert_eq!(r.modes.of(m), RoutingModes::default().of(m), "{m}");
    }
    assert_eq!(r.corrections, CorrectionsConfig::default());
    // Its one commented bar is trivial's, at the default it documents.
    assert_eq!(r.modes.trivial.switch_confidence, Some(TRIVIAL_CONFIDENCE));
    assert_eq!(r.confidence_for("trivial"), TRIVIAL_CONFIDENCE);
    assert_eq!(r.confidence_for("sophisticated"), 0.6);
    for (p, model) in [
        ("opus", "claude-opus-5-5"),
        ("fable", "claude-fable-5-1"),
        ("glm53", "glm-5.3"),
        ("haiku", "claude-haiku-5-5"),
        ("haikuhi", "claude-haiku-5-5"),
    ] {
        assert_eq!(cfg.all_profiles()[p].model, model, "[profiles.{p}]");
    }
    // Every profile a mode names is the template's, but `cheapest`.
    for m in MODES {
        for p in r.modes.of(m).iter().filter(|p| *p != CHEAPEST) {
            assert!(cfg.all_profiles().contains_key(p), "{m}: {p}");
        }
    }
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
        assert_eq!(d.modes.of("trivial"), ["haiku", "glm", "cheapest"]);
        assert_eq!(d.modes.of("quick"), ["haiku"]);
        assert!(d.modes.of("chat").is_empty());
        assert_eq!(d.modes.of("other"), d.modes.of("chat"));
        assert_eq!(d.modes.of("sophisticated"), ["opus", "fable"]);
        assert_eq!(d.modes.of("deep_coding"), ["opus", "sonnet"]);
        assert_eq!(d.modes.of("routine_coding"), ["haikuhi", "sonnet"]);
        d.validate(|_| false).unwrap();
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
    }

    #[test]
    fn one_mode_set_keeps_the_others_defaults() {
        let c = cfg("[modes.chat]\nprofiles = [\"sonnet\"]").unwrap();
        assert_eq!(c.modes.of("chat"), ["sonnet"]);
        assert_eq!(c.modes.of("sophisticated"), ["opus", "fable"]);
        assert!(
            cfg("[modes.poetry]\nprofiles = []").is_err(),
            "no such mode"
        );
        assert!(cfg("wait_ms = 1").is_err(), "an unknown key");
    }

    #[test]
    fn the_checks_refuse_what_cannot_hold() {
        for bad in [
            "switch_confidence = 0.0",
            "switch_confidence = 1.5",
            "max_wait_ms = 9000",
            "[corrections]\nsimilarity = 0.0",
            "[corrections]\nsimilarity = 1.2",
            "[corrections]\nmax_entries = 0",
        ] {
            assert!(cfg(bad).unwrap().validate(|_| false).is_err(), "{bad}");
        }
        let empty = cfg("[modes.trivial]\nprofiles = [\" \"]").unwrap();
        assert!(empty.validate(|_| false).is_err());
        for bad in ["0.0", "1.5", "-0.1"] {
            let c = cfg(&format!("[modes.chat]\nswitch_confidence = {bad}")).unwrap();
            let e = c.validate(|_| false).unwrap_err();
            assert!(
                format!("{e:#}").contains("routing.modes.chat.switch_confidence"),
                "{bad}: {e:#}"
            );
        }
        let e = cfg("").unwrap().validate(|p| p == CHEAPEST).unwrap_err();
        assert!(format!("{e:#}").contains("reserved"));
    }

    /// Each mode's bar (theseus-6n5j): trivial's is 0.4 when its table sets
    /// none, a table naming only its profiles included (the owner's config
    /// names trivial's profiles); every other mode takes the section's; a
    /// mode's own bar wins, the section's raised or lowered.
    #[test]
    fn each_mode_routes_at_its_own_bar_and_trivial_at_four_tenths() {
        let d = cfg("").unwrap();
        assert_eq!(d.confidence_for("trivial"), 0.4);
        for m in [
            "quick",
            "chat",
            "sophisticated",
            "deep_coding",
            "routine_coding",
            "other",
        ] {
            assert_eq!(d.confidence_for(m), 0.6, "{m}");
        }
        let named = cfg("[modes.trivial]\nprofiles = [\"haiku\", \"glm\"]").unwrap();
        assert_eq!(named.confidence_for("trivial"), 0.4);
        let raised = cfg("switch_confidence = 0.8").unwrap();
        assert_eq!(raised.confidence_for("chat"), 0.8, "the section's bar");
        assert_eq!(raised.confidence_for("trivial"), 0.4, "trivial's own");
        let own = cfg(
            "[modes.trivial]\nswitch_confidence = 0.55\n[modes.sophisticated]\nswitch_confidence = 0.75",
        )
        .unwrap();
        assert_eq!(own.confidence_for("trivial"), 0.55);
        assert_eq!(own.confidence_for("sophisticated"), 0.75);
        assert_eq!(own.confidence_for("deep_coding"), 0.6);
        own.validate(|_| false).unwrap();
        // An unset bar is left out when the section is written back.
        let text = toml::to_string(&d).unwrap();
        assert!(
            !text.contains("modes.trivial.switch") && !text.contains("0.4"),
            "{text}"
        );
    }

    /// The detours (theseus-3okf): `trivial` and `quick`, each its own
    /// message's; every other mode, `other` and an unknown one included,
    /// switches. `quick` takes its own table.
    #[test]
    fn trivial_and_quick_are_the_detours() {
        assert_eq!(
            MODES.iter().filter(|m| is_detour(m)).collect::<Vec<_>>(),
            [&"trivial", &"quick"]
        );
        for m in ["chat", "other", "sophisticated", "routine_coding", "poetry"] {
            assert!(!is_detour(m), "{m}");
        }
        let c = cfg("[modes.quick]\nprofiles = [\"glm\"]\nswitch_confidence = 0.7").unwrap();
        assert_eq!(c.modes.of("quick"), ["glm"]);
        assert_eq!(c.confidence_for("quick"), 0.7);
        assert_eq!(c.modes.of("trivial"), ["haiku", "glm", "cheapest"]);
        c.validate(|_| false).unwrap();
    }

    #[test]
    fn the_section_lowers_the_packs_mode_and_never_raises_it() {
        let on = RoutingConfig::default();
        assert_eq!(on.pack_mode(PackMode::Live), PackMode::Live);
        assert_eq!(on.pack_mode(PackMode::Shadow), PackMode::Shadow);
        assert_eq!(on.pack_mode(PackMode::Off), PackMode::Off);
        let shadow = cfg("mode = \"shadow\"").unwrap();
        assert_eq!(shadow.pack_mode(PackMode::Live), PackMode::Shadow);
        let off = cfg("enabled = false").unwrap();
        assert_eq!(off.pack_mode(PackMode::Live), PackMode::Off);
    }
}
