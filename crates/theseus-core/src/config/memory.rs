//! `[memory]` (M6 §2.14, step 30a): recall, live on `baseline` by default
//! (theseus-is8t: the memory is what sets Theseus apart, so a fresh install
//! has it; it needs no Jev, and with no index it answers nothing and costs
//! nothing; the place rule keeps a shared place to its own sessions).
//! `off` turns it off. `shadow` asks
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
    Off,
    /// Recall runs and is recorded, and nothing reaches the model.
    Shadow,
    /// A sticky share of sessions get `arm` in front of the model; the rest
    /// run `none`, with `baseline` in shadow.
    Canary,
    /// Every session gets `arm` in front of the model.
    #[default]
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
    /// pauses until the next recompile. Long sessions are fine (the owner,
    /// theseus-d9je7): it limits the tail, not a turn's pack.
    #[serde(default = "default_session_cap")]
    pub session_recall_cap_tokens: u64,
    /// The pack's tokens: an ordinary prompt's (theseus-d9je7, `turn::tier`).
    #[serde(default = "default_budget")]
    pub recall_budget_tokens: u64,
    /// The pack's items: an ordinary prompt's.
    #[serde(default = "default_max_items")]
    pub recall_max_items: usize,
    /// A deep prompt's pack (`sophisticated`, `deep_coding`, `review`), in
    /// tokens and items (theseus-d9je7).
    #[serde(default = "default_deep_tokens")]
    pub deep_recall_tokens: u64,
    #[serde(default = "default_deep_items")]
    pub deep_recall_items: usize,
    /// How long a turn's recall waits for the index. The turn never waits for
    /// it past its model call's answer and this.
    #[serde(default = "default_deadline_ms")]
    pub recall_deadline_ms: u64,
    /// The most word pieces of a turn's new text the vector source embeds
    /// (theseus-zo1y): an embedding's time grows with its tokens, and a
    /// recall's whole query (about 115) missed the deadline. 0: uncut.
    #[serde(default = "default_vector_tokens")]
    pub recall_vector_tokens: u64,
    /// How long a recall in front of the model waits for Jev's live rerank
    /// (32d), from the rerank's start, its state's build included. Past it,
    /// recall's own order stands, and the answer is recorded `late`.
    #[serde(default = "default_rerank_wait_ms")]
    pub rerank_wait_ms: u64,
    /// The relevance floor (theseus-55hlu): a candidate whose fused score is
    /// under this share of the best its sources allow (every answering
    /// source's top rank) is not admitted, and a pack of none brings no
    /// `Recall` node. 0 admits all.
    #[serde(default = "default_recall_floor")]
    pub recall_floor: f64,
    /// The subject bar (theseus-55hlu): the share of that best a candidate
    /// about another subject of the same kind as the question's must reach
    /// (another host, path or crate than the one the question names). 0
    /// is no bar.
    #[serde(default)]
    pub recall_subject_bar: f64,
    /// Whether external text (a fetched page, a listed program's output) may
    /// be recalled.
    #[serde(default)]
    pub include_external: bool,
    /// The profile that summarizes what the ring would drop (step 30c):
    /// compaction. A configured key is a pin (route.v10, theseus-ej6bj):
    /// `session` is the turn's own profile, provider and model, so the range
    /// goes nowhere the session's turns don't; a key of `[profiles]` sends it
    /// to that profile's provider, which may not be the session's; `off`
    /// keeps the ring. Unset: Jev chooses the model for each summary.
    #[serde(default)]
    pub summary_profile: Option<String>,
    /// The tokens of the recall section an assembled prefix carries (30c):
    /// a task's first compile, and a compaction.
    #[serde(default = "default_assembled_budget")]
    pub assembled_budget_tokens: u64,
    /// The profile consolidation writes its syntheses with (31b). A
    /// configured key is a pin (route.v10, theseus-ej6bj): `session` is the
    /// profile every source's session last used (the owner's call at 30c: no
    /// second provider reads a session's text by default; a cluster whose
    /// sources disagree waits), and a key of `[profiles]` is the operator's
    /// choice. Unset: Jev chooses the model for each cluster, from US options
    /// only (a synthesis reads the owner's material).
    #[serde(default)]
    pub synth_profile: Option<String>,
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
    3_000
}
fn default_recall_floor() -> f64 {
    0.0
}
fn default_max_items() -> usize {
    8
}
fn default_deep_tokens() -> u64 {
    8_000
}
fn default_deep_items() -> usize {
    16
}
fn default_deadline_ms() -> u64 {
    250
}
/// About 90 ms an embedding on one thread in the scale plan's spike, against
/// about 350 at 115 word pieces.
fn default_vector_tokens() -> u64 {
    32
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
    64_000
}
fn default_assembled_budget() -> u64 {
    4_000
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
/// what an unset key meant until Jev chose the summary's model (the owner's
/// call at 30c's join, 2026-10-04: no second provider reads a session's text
/// by default), and the pin that keeps it.
pub const SUMMARY_SESSION: &str = "session";

/// A key that is set to nothing: an error (an unset key is `None`).
fn blank(p: Option<&str>) -> bool {
    p.is_some_and(|p| p.trim().is_empty())
}

/// What `[memory] summary_profile` says to do (route.v10, theseus-ej6bj).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Summary<'a> {
    /// `off`: the ring drops leading turns.
    Off,
    /// A configured key (a pin): a key of `[profiles]`, or `session`.
    Pin(&'a str),
    /// Unset: Jev chooses the model for each summary.
    Jev,
}

/// The longest a recall may wait for the index.
pub const MAX_RECALL_DEADLINE_MS: u64 = 5_000;
/// The most word pieces a query's one window holds (the model's 512, less
/// its markers and prefix).
pub const MAX_RECALL_VECTOR_TOKENS: u64 = 500;
/// The longest a recall may wait for Jev's live rerank: its call's own
/// deadline (`judge::rerank::DEADLINE`).
pub const MAX_RERANK_WAIT_MS: u64 = 600;
/// The most items a pack may hold: the index's hits for a turn.
pub const MAX_RECALL_ITEMS: usize = 40;

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            mode: MemoryMode::default(),
            arm: MemoryArm::default(),
            canary_fraction: default_canary_fraction(),
            experiment: default_experiment(),
            session_recall_cap_tokens: default_session_cap(),
            recall_budget_tokens: default_budget(),
            recall_max_items: default_max_items(),
            deep_recall_tokens: default_deep_tokens(),
            deep_recall_items: default_deep_items(),
            recall_deadline_ms: default_deadline_ms(),
            recall_vector_tokens: default_vector_tokens(),
            rerank_wait_ms: default_rerank_wait_ms(),
            recall_floor: default_recall_floor(),
            recall_subject_bar: 0.0,
            include_external: false,
            summary_profile: None,
            assembled_budget_tokens: default_assembled_budget(),
            synth_profile: None,
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

    /// What compaction does for its summary: nothing (`off`), the pinned
    /// profile (a key, or `session`), or ask Jev (the key is unset).
    pub fn summary(&self) -> Summary<'_> {
        match self.summary_profile.as_deref() {
            None => Summary::Jev,
            Some(SUMMARY_OFF) => Summary::Off,
            Some(p) => Summary::Pin(p),
        }
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
        for (key, tokens) in [
            ("recall_budget_tokens", self.recall_budget_tokens),
            ("deep_recall_tokens", self.deep_recall_tokens),
        ] {
            if self.session_recall_cap_tokens < tokens {
                bail!(
                    "memory.session_recall_cap_tokens = {} is under {key} = {tokens}: no \
                     recall would fit",
                    self.session_recall_cap_tokens,
                );
            }
            if tokens == 0 {
                bail!("memory.{key} = 0: a pack with no tokens admits nothing");
            }
        }
        for (key, items) in [
            ("recall_max_items", self.recall_max_items),
            ("deep_recall_items", self.deep_recall_items),
        ] {
            if !(1..=MAX_RECALL_ITEMS).contains(&items) {
                bail!("memory.{key} = {items} is outside 1 to {MAX_RECALL_ITEMS}");
            }
        }
        if blank(self.summary_profile.as_deref()) {
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
        if self.recall_vector_tokens > MAX_RECALL_VECTOR_TOKENS {
            bail!(
                "memory.recall_vector_tokens = {} is over {MAX_RECALL_VECTOR_TOKENS}: a query's \
                 one window holds no more",
                self.recall_vector_tokens
            );
        }
        if blank(self.synth_profile.as_deref()) {
            bail!("memory.synth_profile is empty: name a profile, or \"session\"");
        }
        if !(0.0..=100.0).contains(&self.synth_limit_usd_per_day) {
            bail!(
                "memory.synth_limit_usd_per_day = {} is outside 0 to 100",
                self.synth_limit_usd_per_day
            );
        }
        for (key, v) in [
            ("recall_floor", self.recall_floor),
            ("recall_subject_bar", self.recall_subject_bar),
        ] {
            if !(0.0..=1.0).contains(&v) {
                bail!(
                    "memory.{key} = {v} is outside 0 to 1: it is a share of the best fused \
                     score the sources allow"
                );
            }
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
            floor: theseus_memory::Floor::default(),
            persona: None,
            category: None,
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
/// `example_template_uncommented_still_parses`): live, with §2.4's
/// defaults, which is what it says as written, with its lines commented
/// (theseus-is8t).
#[cfg(test)]
pub(crate) fn the_templates_memory_section(m: &MemoryConfig) {
    assert_eq!(m.mode, MemoryMode::Live);
    // Un-commented, the two profile lines pin `session`, the word an unset key
    // meant before Jev chose (theseus-ej6bj); everything else is the default.
    let pinned = MemoryConfig {
        summary_profile: Some(SUMMARY_SESSION.into()),
        synth_profile: Some(SUMMARY_SESSION.into()),
        ..MemoryConfig::default()
    };
    assert_eq!(*m, pinned);
    // As written, with its lines commented, the template is the default.
    assert_eq!(crate::Config::example().memory, MemoryConfig::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> Result<crate::Config> {
        let base = crate::Config::EXAMPLE_TOML;
        Ok(crate::Config::from_toml(&format!("{base}\n{toml}"))?)
    }

    /// Live on `baseline` by default (theseus-is8t); `off` turns it off,
    /// and shadow, canary, and live each turn it on; the limits are checked.
    #[test]
    fn memory_is_live_by_default_and_off_turns_it_off() {
        let example = crate::Config::example().memory;
        assert!(example.on());
        assert_eq!(
            (example.mode, example.arm),
            (MemoryMode::Live, MemoryArm::Baseline)
        );
        assert!(!parse("[memory]\nmode = \"off\"\n").unwrap().memory.on());
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
        assert_eq!(crate::Config::example().memory.recall_vector_tokens, 32);
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
            "deep_recall_tokens = 0",
            "deep_recall_tokens = 70000",
            "deep_recall_items = 0",
            "deep_recall_items = 41",
            "recall_deadline_ms = 0",
            "recall_deadline_ms = 5001",
            "recall_vector_tokens = 501",
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
            "recall_floor = 1.1",
            "recall_floor = -0.1",
            "recall_subject_bar = 1.5",
        ] {
            let cfg = parse(&format!("[memory]\nmode = \"shadow\"\n{bad}\n")).unwrap();
            assert!(cfg.validate().is_err(), "{bad}");
        }
    }

    /// The relevance floor and the subject bar are shares (theseus-55hlu): a
    /// note says nothing, the floor is the documented default, the bar is
    /// off, and a share is read as written; the template documents both.
    #[test]
    fn the_floor_and_the_subject_bar_are_shares_of_the_best_score() {
        let m = crate::Config::example().memory;
        assert_eq!(m.recall_floor, default_recall_floor());
        assert_eq!(
            m.recall_subject_bar, 0.0,
            "the bar is off unless measured to help"
        );
        let cfg = parse("[memory]\nrecall_floor = 0.25\nrecall_subject_bar = 0.6\n").unwrap();
        assert_eq!(
            (cfg.memory.recall_floor, cfg.memory.recall_subject_bar),
            (0.25, 0.6)
        );
        cfg.validate().unwrap();
        for key in ["recall_floor", "recall_subject_bar"] {
            assert!(
                crate::Config::EXAMPLE_TOML.contains(&format!("# {key} = ")),
                "{key}"
            );
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
