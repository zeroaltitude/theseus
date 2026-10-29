//! `[routing]` (M5 step 25e): Jev's model per task class. The route
//! pack (`route.v4` since theseus-p8dy; `route.v3`, `route.v2` and `route.v1` before it)
//! asks, at `inbound`, which task class a person's message needs, and the turn runs
//! on that class's first usable profile; since route.v3 it also asks the
//! effort the reply needs, which the turn's model runs at, within
//! `effort_bounds`. It acts only while `[judge]` is on, and
//! `rung = "shadow"` (or `[judge.packs."route.v4"] rung = "shadow"`) records
//! the verdict and routes nothing. Every key has a default, so a sparse note
//! holds only what differs.

use std::borrow::Cow;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::{Effort, PackMode};

pub use super::routing_checkpoints::TaskCheckpoints;

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
    /// `live` routes; `shadow` records the verdict and routes nothing: the route pack's rung.
    #[serde(default = "live", rename = "rung")]
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
    /// unless its mode sets its own bar (`[routing.classes.<class>]`).
    #[serde(default = "switch_confidence")]
    pub switch_confidence: f64,
    /// The efforts Jev's answer may set, lowest and highest, inclusive
    /// (route.v3, theseus-qe3v): an answer outside them is clamped to the
    /// nearer one. The full range by default.
    #[serde(default = "effort_bounds")]
    pub effort_bounds: [Effort; 2],
    /// Jev's answer of `xhigh` or `max` applies only when its confidence is
    /// strictly above this (theseus-p8dy; 0.40 since the owner's 2026-10-22
    /// "reduce required confidence"); at or below, the turn runs at `high`.
    #[serde(default = "high_effort_confidence")]
    pub high_effort_confidence: f64,
    /// An effort answer whose band is under the question's confirm bar still
    /// applies at this confidence or more (theseus-7pm8t): an effort level is
    /// not held to the model switch's bar. At most `high_effort_confidence`.
    #[serde(default = "effort_confidence")]
    pub effort_confidence: f64,
    /// The dear tier's context guard (theseus-p8dy): at a compile of more
    /// estimated tokens than this, a `sophisticated` or `review` verdict skips
    /// a profile whose input price is above the next usable one's in the
    /// mode's list.
    #[serde(default = "dear_tier_max_tokens")]
    pub dear_tier_max_tokens: u64,
    /// Each task class's profiles, in order: the first usable wins.
    #[serde(default)]
    pub classes: RoutingClasses,
    /// The owner's corrections of routing (theseus-q31l).
    #[serde(default)]
    pub corrections: CorrectionsConfig,
    /// A running task's model ranges (theseus-ypnxt.9).
    #[serde(default)]
    pub task_checkpoints: TaskCheckpoints,
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

/// `[routing.classes.<class>]`, one per mode the route pack answers (`other`
/// is routed as `chat`; `quick` is route.v2's, theseus-3okf; `run_tool` and
/// `commentary` are route.v9's, theseus-gezlj).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoutingClasses {
    /// route.v9's `run_tool` (theseus-gezlj): a command, script or tool to run
    /// and its result to report. A detour, and one that carries the tools.
    #[serde(default = "run_tool")]
    pub run_tool: ClassProfiles,
    /// route.v9's `commentary` (theseus-gezlj): a remark that asks for nothing.
    /// A detour that carries `tools_find` alone, as `trivial`'s does.
    #[serde(default = "commentary")]
    pub commentary: ClassProfiles,
    #[serde(default = "trivial")]
    pub trivial: ClassProfiles,
    #[serde(default = "quick")]
    pub quick: ClassProfiles,
    #[serde(default)]
    pub chat: ClassProfiles,
    #[serde(default = "sophisticated")]
    pub sophisticated: ClassProfiles,
    #[serde(default = "deep_coding")]
    pub deep_coding: ClassProfiles,
    #[serde(default = "routine_coding")]
    pub routine_coding: ClassProfiles,
    /// route.v4's `review` (theseus-p8dy): verification of work already made.
    #[serde(default = "review")]
    pub review: ClassProfiles,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassProfiles {
    /// Profile names (or `cheapest`), in order; empty: the session's own.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// The confidence this mode's verdict needs to route (theseus-6n5j).
    /// Unset: `[routing] switch_confidence`, except `trivial`, whose bar is
    /// [`TRIVIAL_CONFIDENCE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub switch_confidence: Option<f64>,
    /// Restricted (China-origin or unknown-origin) profiles this class may
    /// use, only for an ask that an open request can be built for
    /// (theseus-a1wvq.4). `trivial` alone takes any; they never enter by
    /// `profiles`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cn_profiles: Vec<String>,
    /// The entry of `profiles` the `cn_profiles` go right after; unset, they
    /// go first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cn_after: Option<String>,
    /// A restricted profile that drafts this class's coding asks in a tree of
    /// its own, never applied until a `final` profile has reviewed it
    /// (theseus-a1wvq.13): `routine_coding` and `deep_coding` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cn_draft: Option<String>,
    /// The Claude profiles that review and finish every draft: the first
    /// usable runs. Required with `cn_draft`, refused without it.
    #[serde(default, rename = "final", skip_serializing_if = "Vec::is_empty")]
    pub final_review: Vec<String>,
    /// A restricted profile that reads the draft's diff for bugs before the
    /// `final` review opens, on open content only; its findings go to that
    /// review and nowhere else (theseus-a1wvq.17). Refused without `cn_draft`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_pass: Option<String>,
}

/// `trivial`'s bar when its table sets none (theseus-6n5j, the owner's
/// "about 40% for trivial only", 2026-10-04): a wrong trivial call is cheap,
/// since a detour carries only the last `trivial_context_turns` exchanges to
/// the trivial profile, for that turn alone.
pub const TRIVIAL_CONFIDENCE: f64 = 0.4;

/// The classes, in the pack's order (route.v9's `run_tool` and `commentary`
/// first, as the pack lists them).
pub const MODES: [&str; 9] = [
    "run_tool",
    "commentary",
    "trivial",
    "quick",
    "chat",
    "sophisticated",
    "deep_coding",
    "routine_coding",
    "review",
];

/// The classes whose turn is a detour (theseus-3okf; `trivial` alone before
/// it; `run_tool` and `commentary` since route.v9, theseus-gezlj): that turn
/// alone runs on the mode's profile, with the last `trivial_context_turns`
/// exchanges, and the session stays where it was, its cache warm. A late
/// verdict of one of them applies to no message (`routing::carries`). Every
/// other mode switches the session: a run-the-script turn must not move the
/// session's profile and leave its cache cold for one command.
pub const DETOURS: [&str; 4] = ["trivial", "quick", "run_tool", "commentary"];

/// Whether `mode`'s turn is a detour ([`DETOURS`]).
pub fn is_detour(mode: &str) -> bool {
    DETOURS.contains(&mode)
}

/// The detours whose request carries `tools_find` and what its message names,
/// not the whole tool set (theseus-d9je7; `commentary` since theseus-gezlj): a
/// greeting or a remark asks for no tool. `run_tool` is not one of them, since
/// the real tools are what its turn is for, and neither is `quick`.
pub const BARE_DETOURS: [&str; 2] = ["trivial", "commentary"];

/// Whether `mode`'s detour carries `tools_find` alone ([`BARE_DETOURS`]).
pub fn is_bare_detour(mode: &str) -> bool {
    BARE_DETOURS.contains(&mode)
}

/// The detours whose request carries the turn's recall (theseus-pa1xx): a
/// quick question is often about what was said or decided before, which only
/// recall holds. A trivial one (a greeting, a thanks) carries none
/// (theseus-n7nc).
pub const RECALLING_DETOURS: [&str; 1] = ["quick"];

/// Whether `mode`'s detour carries the turn's recall ([`RECALLING_DETOURS`]).
pub fn detour_recalls(mode: &str) -> bool {
    RECALLING_DETOURS.contains(&mode)
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
fn effort_bounds() -> [Effort; 2] {
    [Effort::Low, Effort::Max]
}
fn high_effort_confidence() -> f64 {
    0.40
}
fn effort_confidence() -> f64 {
    0.35
}
fn dear_tier_max_tokens() -> u64 {
    200_000
}
fn list(names: &[&str]) -> ClassProfiles {
    ClassProfiles {
        profiles: names.iter().map(|s| (*s).to_string()).collect(),
        ..Default::default()
    }
}
/// Haiku 5.5, then the cheapest usable profile, so a config without Haiku
/// still detours as it did before (theseus-3okf). GLM-5.3 Flash comes back
/// only through `cn_profiles`, for a request that carries open content,
/// behind Haiku (theseus-a1wvq.4).
fn trivial() -> ClassProfiles {
    list(&["haiku", CHEAPEST])
}
/// Haiku 5.5 alone: a config without it runs a quick message on the
/// session's own profile, as it did while `chat` held them.
fn quick() -> ClassProfiles {
    list(&["haiku"])
}
/// Haiku 5.5, then Sonnet 5.5 (theseus-gezlj, theseus-tqomi; the owner,
/// 2026-10-26: "does haiku do tools?"): it runs a command with the real tool
/// set, and a config without either runs the turn on the session's own
/// profile. The China-origin models the owner suggested for such turns are not
/// named here: they wait on `theseus-ouqru`'s content screen for any tool
/// output (`cn_profiles` can express it once it lands).
fn run_tool() -> ClassProfiles {
    list(&["haiku", "sonnet"])
}
/// Haiku 5.5 alone (theseus-gezlj): a remark needs no more than a trivial
/// turn does.
fn commentary() -> ClassProfiles {
    list(&["haiku"])
}
/// Opus 5.5 at high effort, then Opus, then Sonnet 5.5 at high effort
/// (theseus-tqomi; the cn rubric's §7.4; the owner, 2026-10-26: "i'd move to
/// opus high -- i want real thought for comparative analysis"): a config
/// without `opushi` runs on `opus`, and without that on `sonnethi`. The class
/// selects the model; Jev's own `reply_effort` still sets the effort, within
/// `effort_bounds`.
fn sophisticated() -> ClassProfiles {
    list(&["opushi", "opus", "sonnethi"])
}
fn deep_coding() -> ClassProfiles {
    list(&["opus", "sonnet"])
}
/// Haiku 5.5 at high effort, then Sonnet 5.5 (theseus-3okf): a config
/// without `haikuhi` runs routine programming on `sonnet`, or the session's
/// own.
fn routine_coding() -> ClassProfiles {
    list(&["haikuhi", "sonnet"])
}
/// Opus 5.5, then Sonnet 5.5 at high effort (theseus-p8dy, then theseus-tqomi's
/// cn rubric §7.4; the owner, 2026-10-26 07:33): a config without either
/// reviews on the session's own.
fn review() -> ClassProfiles {
    list(&["opus", "sonnethi"])
}

impl Default for RoutingClasses {
    fn default() -> Self {
        Self {
            run_tool: run_tool(),
            commentary: commentary(),
            trivial: trivial(),
            quick: quick(),
            chat: ClassProfiles::default(),
            sophisticated: sophisticated(),
            deep_coding: deep_coding(),
            routine_coding: routine_coding(),
            review: review(),
        }
    }
}

impl RoutingClasses {
    /// A mode's table; `other` and any unknown mode are `chat`'s.
    pub fn table(&self, mode: &str) -> &ClassProfiles {
        match mode {
            "run_tool" => &self.run_tool,
            "commentary" => &self.commentary,
            "trivial" => &self.trivial,
            "quick" => &self.quick,
            "sophisticated" => &self.sophisticated,
            "deep_coding" => &self.deep_coding,
            "routine_coding" => &self.routine_coding,
            "review" => &self.review,
            _ => &self.chat,
        }
    }

    /// A mode's profiles; `other` and any unknown mode are `chat`'s.
    pub fn of(&self, mode: &str) -> &[String] {
        &self.table(mode).profiles
    }

    /// The list a verdict walks: `profiles`, and for a cleared ask the
    /// `cn_profiles` too, first or right after `cn_after`. Borrowed whenever
    /// nothing is added (theseus-a1wvq.4).
    pub fn effective(&self, mode: &str, cleared: bool) -> Cow<'_, [String]> {
        let t = self.table(mode);
        if !cleared || t.cn_profiles.is_empty() {
            return Cow::Borrowed(&t.profiles);
        }
        let at = t
            .cn_after
            .as_ref()
            .and_then(|a| t.profiles.iter().position(|p| p == a))
            .map_or(0, |i| i + 1);
        let mut out = t.profiles.clone();
        out.splice(at..at, t.cn_profiles.iter().cloned());
        Cow::Owned(out)
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
            effort_bounds: effort_bounds(),
            high_effort_confidence: high_effort_confidence(),
            effort_confidence: effort_confidence(),
            dear_tier_max_tokens: dear_tier_max_tokens(),
            classes: RoutingClasses::default(),
            corrections: CorrectionsConfig::default(),
            task_checkpoints: TaskCheckpoints::default(),
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
            if let Some(c) = self.classes.table(m).switch_confidence {
                if !(c > 0.0 && c <= 1.0) {
                    anyhow::bail!(
                        "routing.classes.{m}.switch_confidence must be above 0 and at most 1"
                    );
                }
            }
        }
        if self.effort_bounds[0] > self.effort_bounds[1] {
            anyhow::bail!(
                "routing.effort_bounds is [lowest, highest]: its first is above its second"
            );
        }
        let h = self.high_effort_confidence;
        if !(h > 0.0 && h <= 1.0) {
            anyhow::bail!("routing.high_effort_confidence must be above 0 and at most 1");
        }
        let e = self.effort_confidence;
        if !(e > 0.0 && e <= h) {
            anyhow::bail!(
                "routing.effort_confidence must be above 0 and at most routing.high_effort_confidence"
            );
        }
        if self.dear_tier_max_tokens == 0 {
            anyhow::bail!("routing.dear_tier_max_tokens must be at least 1");
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
            if let Some(p) = self.classes.of(m).iter().find(|p| p.trim().is_empty()) {
                anyhow::bail!("routing.classes.{m}.profiles has an empty name {p:?}");
            }
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

    /// The confidence a verdict of `mode` needs to route: its mode's own
    /// bar, else trivial's [`TRIVIAL_CONFIDENCE`], else the section's
    /// `switch_confidence` (theseus-6n5j).
    pub fn confidence_for(&self, mode: &str) -> f64 {
        let own = self.classes.table(mode).switch_confidence;
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

/// The China-origin guidance is real (theseus-a1wvq.12): a draft for routine coding, the pair deep coding keeps
/// off by default, and the trivial class's GLM after Haiku.
#[cfg(test)]
fn the_templates_china_origin_guidance(cfg: &crate::Config) {
    let classes = &cfg.routing.classes;
    assert_eq!(classes.routine_coding.cn_draft.as_deref(), Some("dsflash"));
    assert_eq!(classes.routine_coding.final_review, ["sonnet"]);
    assert_eq!(classes.deep_coding.cn_draft.as_deref(), Some("dsflashmax"));
    assert_eq!(classes.deep_coding.final_review, ["sonnethi", "opus"]);
    assert_eq!(classes.trivial.cn_profiles, ["glm"]);
}

/// The template's `[routing]`, un-commented: its defaults, and the
/// profiles its classes name configured.
#[cfg(test)]
pub(crate) fn the_templates_routing_section(cfg: &crate::Config) {
    the_templates_china_origin_guidance(cfg);
    let r = &cfg.routing;
    assert!(r.enabled);
    assert_eq!(r.mode, RoutingMode::Live);
    assert_eq!(
        (r.max_wait_ms, r.trivial_context_turns, r.cold_switch_tokens),
        (200, 2, 30_000)
    );
    assert_eq!(r.switch_confidence, 0.6);
    assert_eq!(r.effort_bounds, effort_bounds(), "the full range");
    assert_eq!(r.high_effort_confidence, 0.40);
    assert_eq!(r.effort_confidence, 0.35);
    assert_eq!(r.dear_tier_max_tokens, 200_000);
    for m in MODES {
        assert_eq!(r.classes.of(m), RoutingClasses::default().of(m), "{m}");
    }
    assert_eq!(r.corrections, CorrectionsConfig::default());
    assert_eq!(r.task_checkpoints, TaskCheckpoints::default());
    // Its one commented bar is trivial's, at the default it documents.
    assert_eq!(
        r.classes.trivial.switch_confidence,
        Some(TRIVIAL_CONFIDENCE)
    );
    assert_eq!(r.confidence_for("trivial"), TRIVIAL_CONFIDENCE);
    assert_eq!(r.confidence_for("sophisticated"), 0.6);
    for (p, model) in [
        ("opus", "claude-opus-5-5"),
        ("fable", "claude-fable-5-1"),
        ("glm53", "glm-5.3"),
        ("haiku", "claude-haiku-5-5"),
        ("haikuhi", "claude-haiku-5-5"),
        ("opushi", "claude-opus-5-5"),
    ] {
        assert_eq!(cfg.all_profiles()[p].model, model, "[profiles.{p}]");
    }
    // Every profile a mode names is the template's, but `cheapest`.
    for m in MODES {
        for p in r.classes.of(m).iter().filter(|p| *p != CHEAPEST) {
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
        assert_eq!(d.classes.of("trivial"), ["haiku", "cheapest"]);
        assert_eq!(d.classes.of("quick"), ["haiku"]);
        assert!(d.classes.of("chat").is_empty());
        assert_eq!(d.classes.of("other"), d.classes.of("chat"));
        assert_eq!(d.classes.of("run_tool"), ["haiku", "sonnet"]);
        assert_eq!(d.classes.of("commentary"), ["haiku"]);
        assert_eq!(
            d.classes.of("sophisticated"),
            ["opushi", "opus", "sonnethi"]
        );
        assert_eq!(d.classes.of("deep_coding"), ["opus", "sonnet"]);
        assert_eq!(d.classes.of("routine_coding"), ["haikuhi", "sonnet"]);
        assert_eq!(d.classes.of("review"), ["opus", "sonnethi"]);
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
        let c = cfg("[classes.chat]\nprofiles = [\"sonnet\"]").unwrap();
        assert_eq!(c.classes.of("chat"), ["sonnet"]);
        assert_eq!(
            c.classes.of("sophisticated"),
            ["opushi", "opus", "sonnethi"]
        );
        assert!(
            cfg("[classes.poetry]\nprofiles = []").is_err(),
            "no such mode"
        );
        assert!(cfg("wait_ms = 1").is_err(), "an unknown key");
    }

    #[test]
    fn the_checks_refuse_what_cannot_hold() {
        for bad in [
            "switch_confidence = 0.0",
            "switch_confidence = 1.5",
            "high_effort_confidence = 0.0",
            "high_effort_confidence = 1.1",
            "effort_confidence = 0.0",
            "effort_confidence = 0.5",
            "dear_tier_max_tokens = 0",
            "max_wait_ms = 9000",
            "[corrections]\nsimilarity = 0.0",
            "[corrections]\nsimilarity = 1.2",
            "[corrections]\nmax_entries = 0",
        ] {
            assert!(cfg(bad).unwrap().validate(|_| false).is_err(), "{bad}");
        }
        let empty = cfg("[classes.trivial]\nprofiles = [\" \"]").unwrap();
        assert!(empty.validate(|_| false).is_err());
        for bad in ["0.0", "1.5", "-0.1"] {
            let c = cfg(&format!("[classes.chat]\nswitch_confidence = {bad}")).unwrap();
            let e = c.validate(|_| false).unwrap_err();
            assert!(
                format!("{e:#}").contains("routing.classes.chat.switch_confidence"),
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
            "run_tool",
            "commentary",
            "quick",
            "chat",
            "sophisticated",
            "deep_coding",
            "routine_coding",
            "review",
            "other",
        ] {
            assert_eq!(d.confidence_for(m), 0.6, "{m}");
        }
        let named = cfg("[classes.trivial]\nprofiles = [\"haiku\", \"glm\"]").unwrap();
        assert_eq!(named.confidence_for("trivial"), 0.4);
        let raised = cfg("switch_confidence = 0.8").unwrap();
        assert_eq!(raised.confidence_for("chat"), 0.8, "the section's bar");
        assert_eq!(raised.confidence_for("trivial"), 0.4, "trivial's own");
        let own = cfg(
            "[classes.trivial]\nswitch_confidence = 0.55\n[classes.sophisticated]\nswitch_confidence = 0.75",
        )
        .unwrap();
        assert_eq!(own.confidence_for("trivial"), 0.55);
        assert_eq!(own.confidence_for("sophisticated"), 0.75);
        assert_eq!(own.confidence_for("deep_coding"), 0.6);
        own.validate(|_| false).unwrap();
        // An unset bar is left out when the section is written back.
        let text = toml::to_string(&d).unwrap();
        assert!(
            !text.contains("classes.trivial.switch") && !text.contains("switch_confidence = 0.4"),
            "{text}"
        );
    }

    /// The detours (theseus-3okf; route.v9's two, theseus-gezlj): `trivial`,
    /// `quick`, `run_tool` and `commentary`, each its own message's; every
    /// other mode, `other` and an unknown one included, switches. `quick`
    /// takes its own table.
    #[test]
    fn trivial_quick_run_tool_and_commentary_are_the_detours() {
        assert_eq!(
            MODES.iter().filter(|m| is_detour(m)).collect::<Vec<_>>(),
            [&"run_tool", &"commentary", &"trivial", &"quick"]
        );
        for m in ["chat", "other", "sophisticated", "routine_coding", "poetry"] {
            assert!(!is_detour(m), "{m}");
        }
        let c = cfg("[classes.quick]\nprofiles = [\"glm\"]\nswitch_confidence = 0.7").unwrap();
        assert_eq!(c.classes.of("quick"), ["glm"]);
        assert_eq!(c.confidence_for("quick"), 0.7);
        assert_eq!(c.classes.of("trivial"), ["haiku", "cheapest"]);
        c.validate(|_| false).unwrap();
    }

    /// A quick detour carries the turn's recall, a trivial one none
    /// (theseus-pa1xx); every recalling mode is a detour. A run_tool turn
    /// carries no recall (its message is the command), and neither does a
    /// remark.
    #[test]
    fn a_quick_detour_recalls_and_a_trivial_one_does_not() {
        assert!(detour_recalls("quick"));
        for m in ["trivial", "run_tool", "commentary"] {
            assert!(!detour_recalls(m), "{m}");
        }
        assert!(RECALLING_DETOURS.iter().all(|m| is_detour(m)));
    }

    /// Which detours carry `tools_find` alone (theseus-gezlj): a greeting and
    /// a remark do, a command to run does not, since the real tools are what
    /// its turn is for. Every bare mode is a detour.
    #[test]
    fn a_run_tool_detour_carries_the_real_tools_and_a_remark_carries_none() {
        for m in ["trivial", "commentary"] {
            assert!(is_bare_detour(m), "{m}");
        }
        for m in ["run_tool", "quick", "chat", "sophisticated", "poetry"] {
            assert!(!is_bare_detour(m), "{m}");
        }
        assert!(BARE_DETOURS.iter().all(|m| is_detour(m)));
        assert!(is_detour("run_tool"));
    }

    /// The v9 classes take their own tables and bars, and a table naming
    /// none is Haiku's (theseus-gezlj).
    #[test]
    fn the_v9_classes_take_their_own_tables_and_bars() {
        let c = cfg(
            "[classes.run_tool]\nprofiles = [\"haikuhi\"]\nswitch_confidence = 0.4\n[classes.commentary]\nprofiles = []",
        )
        .unwrap();
        assert_eq!(c.classes.of("run_tool"), ["haikuhi"]);
        assert_eq!(c.confidence_for("run_tool"), 0.4);
        assert!(c.classes.of("commentary").is_empty());
        assert_eq!(c.confidence_for("commentary"), 0.6);
        assert_eq!(c.classes.of("quick"), ["haiku"], "the others are untouched");
        c.validate(|_| false).unwrap();
        let e = cfg("[classes.run_tool]\nswitch_confidence = 0.0").unwrap();
        let e = e.validate(|_| false).unwrap_err();
        assert!(format!("{e:#}").contains("routing.classes.run_tool.switch_confidence"));
    }

    /// `effort_bounds` (route.v3, theseus-qe3v): the full range by default,
    /// any two levels lowest first, and a pair the wrong way round refused.
    #[test]
    fn the_effort_bounds_default_to_the_full_range_and_go_lowest_first() {
        assert_eq!(cfg("").unwrap().effort_bounds, [Effort::Low, Effort::Max]);
        let c = cfg("effort_bounds = [\"medium\", \"high\"]").unwrap();
        assert_eq!(c.effort_bounds, [Effort::Medium, Effort::High]);
        c.validate(|_| false).unwrap();
        cfg("effort_bounds = [\"high\", \"high\"]")
            .unwrap()
            .validate(|_| false)
            .unwrap();
        let e = cfg("effort_bounds = [\"max\", \"low\"]")
            .unwrap()
            .validate(|_| false);
        assert!(format!("{:#}", e.unwrap_err()).contains("effort_bounds"));
        assert!(cfg("effort_bounds = [\"low\"]").is_err(), "two levels");
        assert!(
            cfg("effort_bounds = [\"low\", \"huge\"]").is_err(),
            "levels only"
        );
    }

    /// `high_effort_confidence` (theseus-p8dy): 0.40 by default since
    /// theseus-7pm8t, above 0 and at most 1.
    #[test]
    fn xhigh_and_max_need_confidence_above_a_bar_that_defaults_to_four_tenths() {
        assert_eq!(cfg("").unwrap().high_effort_confidence, 0.40);
        let c = cfg("high_effort_confidence = 0.85").unwrap();
        assert_eq!(c.high_effort_confidence, 0.85);
        c.validate(|_| false).unwrap();
        cfg("high_effort_confidence = 1.0")
            .unwrap()
            .validate(|_| false)
            .unwrap();
    }

    /// `effort_confidence` (theseus-7pm8t): 0.35 by default, above 0 and no
    /// higher than `high_effort_confidence`, and the error says so.
    #[test]
    fn an_unsure_effort_applies_at_its_own_bar_under_the_high_one() {
        let d = cfg("").unwrap();
        assert_eq!(
            (d.effort_confidence, d.high_effort_confidence),
            (0.35, 0.40)
        );
        d.validate(|_| false).unwrap();
        cfg("effort_confidence = 0.40")
            .unwrap()
            .validate(|_| false)
            .unwrap();
        let e = cfg("effort_confidence = 0.5").unwrap().validate(|_| false);
        assert!(format!("{:#}", e.unwrap_err()).contains("at most routing.high_effort_confidence"));
        cfg("effort_confidence = 0.5\nhigh_effort_confidence = 0.6")
            .unwrap()
            .validate(|_| false)
            .unwrap();
    }

    /// `dear_tier_max_tokens` (theseus-p8dy): 200,000 by default, at least 1.
    #[test]
    fn the_dear_tiers_context_guard_defaults_to_two_hundred_thousand_tokens() {
        assert_eq!(cfg("").unwrap().dear_tier_max_tokens, 200_000);
        let c = cfg("dear_tier_max_tokens = 150000").unwrap();
        assert_eq!(c.dear_tier_max_tokens, 150_000);
        c.validate(|_| false).unwrap();
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
