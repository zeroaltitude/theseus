//! `[project]` (theseus-wadu): the budget for a repository's own
//! `AGENTS.md` and `CLAUDE.md`, which land in the session's block
//! (`docs/design/project-context.md` §6). Every key has a default, so an
//! owner's config names none unless it differs.
//!
//! Not `[projects]`, which is the task graph's long-term work: these are
//! two sections, one per row, and TOML keeps them apart.

use serde::{Deserialize, Serialize};

use crate::project_context::{MAX_TOKENS, MAX_TOKENS_RANGE};

/// `[project]`, in the shape of the persona level's existing cap
/// (`[context] persona_max_tokens`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    /// The most tokens a repository's own instruction files may take in the
    /// session's block, all of them together, estimated as bytes over
    /// `task_graph::view::BYTES_PER_TOKEN`. Absent: [`MAX_TOKENS`] (8,000);
    /// 500 to 64,000.
    ///
    /// It is not caution for its own sake: this repository's own largest
    /// instruction file is over 400,000 bytes, and what the per-file cap
    /// leaves of it would still be over twenty thousand tokens in every
    /// turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
}

impl ProjectConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The level's budget, in tokens.
    pub fn max_tokens(&self) -> u64 {
        self.max_tokens.unwrap_or(MAX_TOKENS)
    }

    /// The level's budget, in bytes of the sections it carries.
    pub fn budget_bytes(&self) -> usize {
        let bytes = self
            .max_tokens()
            .saturating_mul(crate::task_graph::view::BYTES_PER_TOKEN);
        usize::try_from(bytes).unwrap_or(usize::MAX)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(n) = self.max_tokens.filter(|n| !MAX_TOKENS_RANGE.contains(n)) {
            anyhow::bail!(
                "project.max_tokens = {n} is out of range: {} to {}",
                MAX_TOKENS_RANGE.start(),
                MAX_TOKENS_RANGE.end()
            );
        }
        Ok(())
    }
}
