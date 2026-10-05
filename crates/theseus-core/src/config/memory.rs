//! `[memory]` (M6 §2.14, step 30a): recall, off by default. `shadow` asks
//! the index on a turn's first loop, filters and packs what it answers, and
//! records what would have been admitted in a `recall.shadow` row: it writes
//! no frame, and the model's request is the same byte for byte. `canary` and
//! `live` (step 30b) put recall in front of the model: `live` for every
//! session, `canary` for the share `canary_fraction` of sessions a hash of
//! the session and `experiment` picks, sticky, while the others (the
//! control) run `none` live with `baseline` in shadow.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// What recall does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryMode {
    /// No recall.
    #[default]
    Off,
    /// Recall runs and is recorded, and nothing reaches the model.
    Shadow,
    /// A sticky share of sessions get `arm` in front of the model; the rest
    /// run `none`, with `baseline` in shadow.
    Canary,
    /// Every session gets `arm` in front of the model.
    Live,
}

impl MemoryMode {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryMode::Off => "off",
            MemoryMode::Shadow => "shadow",
            MemoryMode::Canary => "canary",
            MemoryMode::Live => "live",
        }
    }
}

/// The arms this build has (§2.9): `none`, today's compiler; `bm25`, the
/// index's BM25 and entities alone (34b); `baseline`, the fused pipeline;
/// `+retention` (32a), `baseline` ranked by FSRS-6 retention too;
/// `+activation` (32b), `baseline` with spreading activation as one more
/// ranked source; and `+synthesis` (31b), `baseline` with consolidation's
/// checked syntheses as candidates. Later steps add theirs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryArm {
    None,
    Bm25,
    #[default]
    Baseline,
    #[serde(rename = "+retention")]
    Retention,
    #[serde(rename = "+activation")]
    Activation,
    /// `baseline` with consolidation's checked syntheses as candidates (31b).
    #[serde(rename = "+synthesis")]
    Synthesis,
}

impl MemoryArm {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryArm::None => "none",
            MemoryArm::Bm25 => "bm25",
            MemoryArm::Baseline => "baseline",
            MemoryArm::Retention => "+retention",
            MemoryArm::Activation => "+activation",
            MemoryArm::Synthesis => "+synthesis",
        }
    }

    /// Whether the arm's science reads the retention projection (32a).
    pub fn reads_retention(self) -> bool {
        self == MemoryArm::Retention
    }

    /// The index's sources the arm asks for (`index.query`'s `sources`):
    /// none for `none`, which asks nothing; BM25 and entities for `bm25`;
    /// and all three, fused, for `baseline`. A tender without its model
    /// answers `baseline` without vectors, and says so in `skipped`.
    /// `+activation` asks for `baseline`'s: its own source is the core's.
    pub fn sources(self) -> &'static [&'static str] {
        match self {
            MemoryArm::None => &[],
            MemoryArm::Bm25 => &["bm25", "entity"],
            MemoryArm::Baseline
            | MemoryArm::Retention
            | MemoryArm::Activation
            | MemoryArm::Synthesis => &["bm25", "entity", "vector"],
        }
    }

    /// The arm a name names (`memory search --arm`).
    pub fn named(name: &str) -> Option<Self> {
        [
            MemoryArm::None,
            MemoryArm::Bm25,
            MemoryArm::Baseline,
            MemoryArm::Retention,
            MemoryArm::Activation,
            MemoryArm::Synthesis,
        ]
        .into_iter()
        .find(|a| a.as_str() == name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    #[serde(default)]
    pub mode: MemoryMode,
    /// The arm canary and live sessions get.
    #[serde(default)]
    pub arm: MemoryArm,
    /// The share of sessions canary puts on `arm`, 0 to 1.
    #[serde(default = "default_canary_fraction")]
    pub canary_fraction: f64,
    /// The experiment's name: it seeds the sticky assignment, and each
    /// `memory.arm` row names it.
    #[serde(default = "default_experiment")]
    pub experiment: String,
    /// The tokens of recall notes a session's tail may hold; past it, recall
    /// pauses until the next recompile.
    #[serde(default = "default_session_cap")]
    pub session_recall_cap_tokens: u64,
    /// The pack's tokens.
    #[serde(default = "default_budget")]
    pub recall_budget_tokens: u64,
    /// The pack's items.
    #[serde(default = "default_max_items")]
    pub recall_max_items: usize,
    /// How long a turn's recall waits for the index. The turn never waits for
    /// it past its model call's answer and this.
    #[serde(default = "default_deadline_ms")]
    pub recall_deadline_ms: u64,
    /// How long a recall in front of the model waits for Jev's live rerank
    /// (32d), from the rerank's start, its state's build included. Past it,
    /// recall's own order stands, and the answer is recorded `late`.
    #[serde(default = "default_rerank_wait_ms")]
    pub rerank_wait_ms: u64,
    /// Whether external text (a fetched page, a listed program's output) may
    /// be recalled.
    #[serde(default)]
    pub include_external: bool,
    /// The profile that summarizes what the ring would drop (step 30c):
    /// compaction. `session`, the default, is the turn's own profile,
    /// provider and model, so the range goes nowhere the session's turns
    /// don't. A key of `[profiles]` sends it to that profile's provider,
    /// which may not be the session's. `off` keeps the ring.
    #[serde(default = "default_summary_profile")]
    pub summary_profile: String,
    /// The tokens of the recall section an assembled prefix carries (30c):
    /// a task's first compile, and a compaction.
    #[serde(default = "default_assembled_budget")]
    pub assembled_budget_tokens: u64,
    /// The profile consolidation writes its syntheses with (31b).
    /// `session`, the default, is the profile every source's session last
    /// used (the owner's call at 30c: no second provider reads a session's
    /// text by default); a cluster whose sources disagree waits. A key of
    /// `[profiles]` is the operator's choice.
    #[serde(default = "default_synth_profile")]
    pub synth_profile: String,
    /// What consolidation may spend in a local day, read from its rows.
    #[serde(default = "default_synth_limit")]
    pub synth_limit_usd_per_day: f64,
    /// The local hour of the nightly consolidation, 0 to 23.
    #[serde(default = "default_consolidate_hour")]
    pub consolidate_hour: u8,
    /// The heat cache of decoded nodes (step 33), in MB of their records'
    /// bytes; 0 turns it off. The store's read path: it serves with `mode`
    /// off too.
    #[serde(default = "default_node_cache_mb")]
    pub node_cache_mb: u64,
}

fn default_budget() -> u64 {
    1500
}
fn default_max_items() -> usize {
    6
}
fn default_deadline_ms() -> u64 {
    250
}
/// 11 live reranks took p50 115 ms and p95 149 ms (the most 149): 200
/// misses few, and with the index's 250 ms stays inside §2.12's 600 ms.
fn default_rerank_wait_ms() -> u64 {
    200
}
fn default_canary_fraction() -> f64 {
    0.5
}
fn default_experiment() -> String {
    "m6-1".into()
}
fn default_session_cap() -> u64 {
    12_000
}
fn default_summary_profile() -> String {
    SUMMARY_SESSION.into()
}
fn default_assembled_budget() -> u64 {
    4_000
}
fn default_synth_profile() -> String {
    SUMMARY_SESSION.into()
}
fn default_synth_limit() -> f64 {
    0.50
}
fn default_consolidate_hour() -> u8 {
    4
}
fn default_node_cache_mb() -> u64 {
    crate::node_cache::DEFAULT_MB
}

/// The largest heat cache, in MB.
pub const MAX_NODE_CACHE_MB: u64 = 16_384;

/// `summary_profile`'s word for no compaction: the ring drops leading turns.
pub const SUMMARY_OFF: &str = "off";

/// `summary_profile`'s word for the turn's own profile, provider and model:
/// the default until Jev routes the summary (the owner's call at 30c's join,
/// 2026-10-04: no second provider reads a session's text by default).
pub const SUMMARY_SESSION: &str = "session";

/// The longest a recall may wait for the index.
pub const MAX_RECALL_DEADLINE_MS: u64 = 5_000;
/// The longest a recall may wait for Jev's live rerank: its call's own
/// deadline (`judge::rerank::DEADLINE`).
pub const MAX_RERANK_WAIT_MS: u64 = 600;
/// The most items a pack may hold: the index's hits for a turn.
pub const MAX_RECALL_ITEMS: usize = 40;

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            mode: MemoryMode::Off,
            arm: MemoryArm::default(),
            canary_fraction: default_canary_fraction(),
            experiment: default_experiment(),
            session_recall_cap_tokens: default_session_cap(),
            recall_budget_tokens: default_budget(),
            recall_max_items: default_max_items(),
            recall_deadline_ms: default_deadline_ms(),
            rerank_wait_ms: default_rerank_wait_ms(),
            include_external: false,
            summary_profile: default_summary_profile(),
            assembled_budget_tokens: default_assembled_budget(),
            synth_profile: default_synth_profile(),
            synth_limit_usd_per_day: default_synth_limit(),
            consolidate_hour: default_consolidate_hour(),
            node_cache_mb: default_node_cache_mb(),
        }
    }
}

impl MemoryConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn on(&self) -> bool {
        self.mode != MemoryMode::Off
    }

    /// The profile compaction summarizes with, unless it is `off`.
    pub fn summary_profile(&self) -> Option<&str> {
        Some(self.summary_profile.as_str()).filter(|p| *p != SUMMARY_OFF)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.canary_fraction) {
            bail!(
                "memory.canary_fraction = {} is outside 0 to 1",
                self.canary_fraction
            );
        }
        if self.experiment.trim().is_empty() {
            bail!("memory.experiment is empty: it names the plan, and seeds each session's arm");
        }
        if self.session_recall_cap_tokens < self.recall_budget_tokens {
            bail!(
                "memory.session_recall_cap_tokens = {} is under recall_budget_tokens = {}: no \
                 recall would fit",
                self.session_recall_cap_tokens,
                self.recall_budget_tokens
            );
        }
        if self.recall_budget_tokens == 0 {
            bail!("memory.recall_budget_tokens = 0: a pack with no tokens admits nothing");
        }
        if !(1..=MAX_RECALL_ITEMS).contains(&self.recall_max_items) {
            bail!(
                "memory.recall_max_items = {} is outside 1 to {MAX_RECALL_ITEMS}",
                self.recall_max_items
            );
        }
        if self.summary_profile.trim().is_empty() {
            bail!("memory.summary_profile is empty: name a profile, or \"off\" to keep the ring");
        }
        if self.assembled_budget_tokens == 0 {
            bail!("memory.assembled_budget_tokens = 0: an assembled recall section with no tokens admits nothing");
        }
        if self.node_cache_mb > MAX_NODE_CACHE_MB {
            bail!(
                "memory.node_cache_mb = {} is over {MAX_NODE_CACHE_MB}: the heat cache holds decoded \
                 nodes in the daemon's memory",
                self.node_cache_mb
            );
        }
        if !(1..=MAX_RECALL_DEADLINE_MS).contains(&self.recall_deadline_ms) {
            bail!(
                "memory.recall_deadline_ms = {} is outside 1 to {MAX_RECALL_DEADLINE_MS}: recall \
                 never holds a turn for long",
                self.recall_deadline_ms
            );
        }
        if self.synth_profile.trim().is_empty() {
            bail!("memory.synth_profile is empty: name a profile, or \"session\"");
        }
        if !(0.0..=100.0).contains(&self.synth_limit_usd_per_day) {
            bail!(
                "memory.synth_limit_usd_per_day = {} is outside 0 to 100",
                self.synth_limit_usd_per_day
            );
        }
        if self.consolidate_hour > 23 {
            bail!(
                "memory.consolidate_hour = {} is not an hour of the day (0 to 23)",
                self.consolidate_hour
            );
        }
        if !(1..=MAX_RERANK_WAIT_MS).contains(&self.rerank_wait_ms) {
            bail!(
                "memory.rerank_wait_ms = {} is outside 1 to {MAX_RERANK_WAIT_MS}: a turn never \
                 waits on Jev's rerank past its call's own deadline",
                self.rerank_wait_ms
            );
        }
        Ok(())
    }

    /// The pack's limits, with §2.4's 400 tokens an item.
    pub fn params(&self) -> theseus_memory::Params {
        theseus_memory::Params {
            budget_tokens: self.recall_budget_tokens,
            max_items: self.recall_max_items,
            item_tokens: 400,
            include_external: self.include_external,
        }
    }
}

/// Which arm a session runs, and whether recall reaches its model: sticky,
/// from a hash of the session and the experiment (§2.9's minimal
/// assignment, which M5's ladder replaces), so a session keeps its arm across
/// turns and restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Assigned {
    pub arm: MemoryArm,
    /// The arm is in front of the model (canary or live, `baseline`).
    pub live: bool,
}

impl MemoryConfig {
    /// `session_id`'s arm under this config; `None` while memory is off or
    /// in shadow, which assigns nothing.
    pub fn assign(&self, session_id: &str) -> Option<Assigned> {
        let arm = match self.mode {
            MemoryMode::Off | MemoryMode::Shadow => return None,
            MemoryMode::Live => self.arm,
            MemoryMode::Canary if bucket(session_id, &self.experiment) < self.canary_fraction => {
                self.arm
            }
            MemoryMode::Canary => MemoryArm::None,
        };
        Some(Assigned {
            arm,
            live: arm != MemoryArm::None,
        })
    }
}

/// Where `session_id` falls in `[0, 1)` for `experiment`: the first 8 bytes
/// of SHA-256 over both, so every build and every restart agrees.
pub fn bucket(session_id: &str, experiment: &str) -> f64 {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(format!("{experiment}\n{session_id}").as_bytes());
    let n = u64::from_be_bytes(d[..8].try_into().unwrap_or([0; 8]));
    (n >> 11) as f64 / (1u64 << 53) as f64
}

/// The template's `[memory]`, un-commented (`config.rs`'s
/// `example_template_uncommented_still_parses`): shadow, with §2.4's
/// defaults, and as written, with its lines commented, off.
#[cfg(test)]
pub(crate) fn the_templates_memory_section(m: &MemoryConfig) {
    assert_eq!(m.mode, MemoryMode::Shadow);
    assert_eq!(
        MemoryConfig {
            mode: MemoryMode::Off,
            ..m.clone()
        },
        MemoryConfig::default()
    );
    assert!(!crate::Config::example().memory.on());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<crate::Config> {
        let base = crate::Config::EXAMPLE_TOML;
        Ok(crate::Config::from_toml(&format!("{base}\n{toml}"))?)
    }

    /// Off by default; shadow, canary, and live turn it on; the limits are
    /// checked.
    #[test]
    fn memory_is_off_by_default_and_its_modes_turn_it_on() {
        assert!(!crate::Config::example().memory.on());
        for mode in ["shadow", "canary", "live"] {
            let cfg = parse(&format!("[memory]\nmode = \"{mode}\"\n")).unwrap();
            assert!(cfg.memory.on(), "{mode}");
            cfg.validate().unwrap();
        }
        for mode in ["on", "full"] {
            let e = parse(&format!("[memory]\nmode = \"{mode}\"\n")).unwrap_err();
            assert!(
                format!("{e:#}").contains("unknown variant"),
                "{mode}: {e:#}"
            );
        }
        assert!(parse("[memory]\nrecall_after = 1\n").is_err());
        assert_eq!(crate::Config::example().memory.rerank_wait_ms, 200);
        assert_eq!(crate::Config::example().memory.node_cache_mb, 64);
        assert!(parse("[memory]\narm = \"+rerank\"\n").is_err());
        for (arm, want) in [
            ("none", MemoryArm::None),
            ("bm25", MemoryArm::Bm25),
            ("baseline", MemoryArm::Baseline),
            ("+retention", MemoryArm::Retention),
            ("+activation", MemoryArm::Activation),
            ("+synthesis", MemoryArm::Synthesis),
        ] {
            let cfg = parse(&format!("[memory]\nmode = \"live\"\narm = \"{arm}\"\n")).unwrap();
            assert_eq!(cfg.memory.arm, want);
            assert_eq!(want.as_str(), arm);
            assert_eq!(MemoryArm::named(arm), Some(want));
        }
        assert!(parse("[memory]\narm = \"activation\"\n").is_err());
        assert_eq!(MemoryArm::named("+rerank"), None);
        for bad in [
            "recall_budget_tokens = 0",
            "recall_max_items = 0",
            "recall_max_items = 41",
            "recall_deadline_ms = 0",
            "recall_deadline_ms = 5001",
            "rerank_wait_ms = 0",
            "rerank_wait_ms = 601",
            "canary_fraction = 1.5",
            "canary_fraction = -0.1",
            "experiment = \" \"",
            "session_recall_cap_tokens = 100",
            "summary_profile = \"\"",
            "assembled_budget_tokens = 0",
            "synth_profile = \"\"",
            "synth_limit_usd_per_day = -1.0",
            "consolidate_hour = 24",
            "node_cache_mb = 16385",
        ] {
            let cfg = parse(&format!("[memory]\nmode = \"shadow\"\n{bad}\n")).unwrap();
            assert!(cfg.validate().is_err(), "{bad}");
        }
    }

    /// The arm is sticky (a session's is the same at every ask), canary's
    /// share follows the fraction, live puts every session on the arm, and
    /// shadow assigns none.
    #[test]
    fn a_sessions_arm_is_sticky_and_canary_takes_its_fraction() {
        let mut m = MemoryConfig {
            mode: MemoryMode::Canary,
            canary_fraction: 0.3,
            ..MemoryConfig::default()
        };
        let ids: Vec<String> = (0..2000).map(|i| format!("ses_heron{i:04}")).collect();
        let live = ids.iter().filter(|s| m.assign(s).unwrap().live).count();
        assert!((500..700).contains(&live), "{live} of 2000 at 0.3");
        for s in &ids[..50] {
            assert_eq!(m.assign(s), m.assign(s), "{s}");
        }
        let control = ids.iter().find(|s| !m.assign(s).unwrap().live).unwrap();
        assert_eq!(m.assign(control).unwrap().arm, MemoryArm::None);
        m.canary_fraction = 1.0;
        assert!(ids.iter().all(|s| m.assign(s).unwrap().live));
        m.canary_fraction = 0.0;
        assert!(ids.iter().all(|s| !m.assign(s).unwrap().live));
        // Another experiment draws again.
        let other = MemoryConfig {
            experiment: "m6-2".into(),
            canary_fraction: 0.3,
            ..m.clone()
        };
        m.canary_fraction = 0.3;
        assert!(ids.iter().any(|s| m.assign(s) != other.assign(s)));
        m.mode = MemoryMode::Live;
        assert!(ids.iter().all(|s| m.assign(s).unwrap().live));
        m.arm = MemoryArm::None;
        assert!(ids.iter().all(|s| !m.assign(s).unwrap().live));
        m.mode = MemoryMode::Shadow;
        assert_eq!(m.assign("ses_heron0001"), None);
    }
}
