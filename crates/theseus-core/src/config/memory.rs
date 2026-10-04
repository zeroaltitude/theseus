//! `[memory]` (M6 §2.14, step 30a): recall, off by default. `shadow` asks
//! the index on a turn's first loop, filters and packs what it answers, and
//! records what would have been admitted in a `recall.shadow` row: it writes
//! no frame, and the model's request is the same byte for byte. `canary` and
//! `live`, which put recall in front of the model, come with step 30b.

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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryConfig {
    #[serde(default)]
    pub mode: MemoryMode,
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
    /// Whether external text (a fetched page, a listed program's output) may
    /// be recalled.
    #[serde(default)]
    pub include_external: bool,
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

/// The longest a recall may wait for the index.
pub const MAX_RECALL_DEADLINE_MS: u64 = 5_000;
/// The most items a pack may hold: the index's hits for a turn.
pub const MAX_RECALL_ITEMS: usize = 40;

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            mode: MemoryMode::Off,
            recall_budget_tokens: default_budget(),
            recall_max_items: default_max_items(),
            recall_deadline_ms: default_deadline_ms(),
            include_external: false,
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

    pub(crate) fn validate(&self) -> Result<()> {
        if self.recall_budget_tokens == 0 {
            bail!("memory.recall_budget_tokens = 0: a pack with no tokens admits nothing");
        }
        if !(1..=MAX_RECALL_ITEMS).contains(&self.recall_max_items) {
            bail!(
                "memory.recall_max_items = {} is outside 1 to {MAX_RECALL_ITEMS}",
                self.recall_max_items
            );
        }
        if !(1..=MAX_RECALL_DEADLINE_MS).contains(&self.recall_deadline_ms) {
            bail!(
                "memory.recall_deadline_ms = {} is outside 1 to {MAX_RECALL_DEADLINE_MS}: recall \
                 never holds a turn for long",
                self.recall_deadline_ms
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

    /// Off by default; shadow is the one mode this step accepts; the limits
    /// are checked.
    #[test]
    fn memory_is_off_by_default_and_only_shadow_turns_it_on() {
        assert!(!crate::Config::example().memory.on());
        let cfg = parse("[memory]\nmode = \"shadow\"\n").unwrap();
        assert!(cfg.memory.on());
        cfg.validate().unwrap();
        for mode in ["canary", "live", "on"] {
            let e = parse(&format!("[memory]\nmode = \"{mode}\"\n")).unwrap_err();
            assert!(
                format!("{e:#}").contains("unknown variant"),
                "{mode}: {e:#}"
            );
        }
        assert!(parse("[memory]\nrecall_after = 1\n").is_err());
        for bad in [
            "recall_budget_tokens = 0",
            "recall_max_items = 0",
            "recall_max_items = 41",
            "recall_deadline_ms = 0",
            "recall_deadline_ms = 5001",
        ] {
            let cfg = parse(&format!("[memory]\nmode = \"shadow\"\n{bad}\n")).unwrap();
            assert!(cfg.validate().is_err(), "{bad}");
        }
    }
}
