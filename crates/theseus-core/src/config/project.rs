//! `[project]` (theseus-wadu): the budget for a repository's own
//! `AGENTS.md` and `CLAUDE.md`, which land in the session's block
//! (`docs/design/project-context.md` §6). Every key has a default, so an
//! owner's config names none unless it differs.
//!
//! Not `[projects]`, which is the task graph's long-term work: these are
//! two sections, one per row, and TOML keeps them apart.

use serde::{Deserialize, Serialize};

use crate::project_context::{MAX_TOKENS, MAX_TOKENS_RANGE};

/// What a compile does when a granted repository's instruction file has
/// changed since the last one that read it (§8, "After a grant").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnChange {
    /// The new text is carried, and the change is named in the block and
    /// ledgered. The design's default, and today's behaviour but for the
    /// saying of it.
    #[default]
    Notice,
    /// The changed files are named and not read until the owner grants the
    /// repository again; the unchanged ones are carried as before.
    Ask,
}

impl OnChange {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notice => "notice",
            Self::Ask => "ask",
        }
    }
}

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
    /// Whether each request's tail carries the date and the git state
    /// (`turn/env_tail.rs`, `docs/design/project-context.md` §4). Absent:
    /// on. Off, a request is what it was before, byte for byte.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_facts: Option<bool>,
    /// What a compile does when a granted repository's instruction file has
    /// changed since the last one that read it (`project_context::change`,
    /// `docs/design/project-context.md` §8). Absent: [`OnChange::Notice`].
    ///
    /// The default is to notify rather than to ask, and the design says why
    /// in as many words: asking on every edit would fire a card many times a
    /// day in the repository being actively edited, and a card that fires
    /// constantly is a card that is clicked through, which is worse security
    /// than a notice that is read. A maintainer taking outside patches turns
    /// it into an ask with this one line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_change: Option<OnChange>,
}

impl ProjectConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// The level's budget, in tokens.
    pub fn max_tokens(&self) -> u64 {
        self.max_tokens.unwrap_or(MAX_TOKENS)
    }

    /// Whether the tail carries the date and the git state.
    pub fn tail_facts(&self) -> bool {
        self.tail_facts.unwrap_or(true)
    }

    /// What a change to a granted repository's file does.
    pub fn on_change(&self) -> OnChange {
        self.on_change.unwrap_or_default()
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
