//! `[context]`: the context files in two levels (theseus-c48), and the
//! working window (theseus-3ki0).

use serde::{Deserialize, Serialize};

/// `[context]` (theseus-c48): context files in two levels. The system
/// level, `files`, is every session's; the files of the persona in play come
/// after it. Each is compiled into the system block under a header naming it
/// and its level (spec §4.4). Paths are `~/` or absolute; nothing is read at
/// load.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextConfig {
    /// The system level: every session's files, before any persona's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<crate::context_files::ContextEntry>,
    /// The persona in play, a key of `[personas]`. Until Jev chooses one
    /// from its ontology of personas (theseus-8kk, theseus-0j2), this is the
    /// only choice. Absent: no persona, and the system level alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_persona: Option<String>,
    /// The most tokens one persona's soul file may be (theseus-utfnl),
    /// estimated as bytes over `task_graph::view::BYTES_PER_TOKEN`. Absent:
    /// `PERSONA_MAX_TOKENS`; 200 to 8000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona_max_tokens: Option<u64>,
    /// The working window (theseus-3ki0): the tokens a request may take, at
    /// most the model's window in the catalog. Absent: the model's full
    /// window. A long instruction's turn clears old results and compacts
    /// against it, so a smaller one makes both come sooner and cost less.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_window_tokens: Option<u64>,
    /// Each session's render, kept across turns so an append renders only
    /// what was written since (theseus-0l3g; `compiler::kept`): at most this
    /// many MiB across the sessions, the least recently used going first.
    /// Absent: `KEPT_MB`; 0 keeps none, and every compile renders whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept_mb: Option<u64>,
}

/// `persona_max_tokens`' default and bounds.
pub const PERSONA_MAX_TOKENS: u64 = 2000;
pub const PERSONA_MAX_TOKENS_RANGE: std::ops::RangeInclusive<u64> = 200..=8000;

/// `kept_mb`'s default.
pub const KEPT_MB: u64 = 64;

impl ContextConfig {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.default_persona.is_none()
            && self.working_window_tokens.is_none()
            && self.kept_mb.is_none()
            && self.persona_max_tokens.is_none()
    }

    /// A persona's soul may be this many tokens.
    pub fn persona_cap(&self) -> u64 {
        self.persona_max_tokens.unwrap_or(PERSONA_MAX_TOKENS)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(n) = self
            .persona_max_tokens
            .filter(|n| !PERSONA_MAX_TOKENS_RANGE.contains(n))
        {
            anyhow::bail!("context.persona_max_tokens = {n} is out of range: 200 to 8000");
        }
        Ok(())
    }

    /// The kept renders' bound, in bytes.
    pub fn kept_bytes(&self) -> usize {
        (self.kept_mb.unwrap_or(KEPT_MB) as usize).saturating_mul(1 << 20)
    }

    /// The window a request compiles against: the model's (`catalog`),
    /// lowered by `working_window_tokens`. `None`: no lower window is set,
    /// so the compiler reads the catalog's own.
    pub fn working_window(&self, catalog: Option<u64>) -> Option<u64> {
        let w = self.working_window_tokens?;
        Some(catalog.map_or(w, |c| c.min(w)))
    }
}
