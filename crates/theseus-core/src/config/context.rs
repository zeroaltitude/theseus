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
    /// window. A long instruction's turn clears old results against it, so a
    /// smaller one makes clearing come sooner and cost less.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_window_tokens: Option<u64>,
    /// `kept_mb` bounded each session's render kept across turns. The kept
    /// render is gone (issue 44): read, warned of ([`Self::trimmed`]), ignored
    /// and never written back.
    #[serde(default, skip_serializing)]
    pub(crate) kept_mb: Option<toml::Value>,
    /// The exchange ledger (issue 44, step 1; `crate::exchange`): each
    /// person's message's mark, each turn's exchange and touches, and a
    /// measuring row per private turn, all beside the conversation and none
    /// of it sent to a model. Absent: on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ledger: Option<bool>,
    /// How a request's context is assembled (issue 44; `super::assembly`). Absent: `"view"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembly: Option<super::assembly::AssemblySetting>,
}

/// `persona_max_tokens`' default and bounds.
pub const PERSONA_MAX_TOKENS: u64 = 2000;
pub const PERSONA_MAX_TOKENS_RANGE: std::ops::RangeInclusive<u64> = 200..=8000;

impl ContextConfig {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.default_persona.is_none()
            && self.working_window_tokens.is_none()
            && self.ledger.is_none()
            && self.persona_max_tokens.is_none()
            && self.assembly.is_none()
    }

    /// The one reader of `[context] assembly`: how `place`'s requests are assembled. With no setting,
    /// the view, unless the ledger is off: the view is read from the ledger, so a place with it off
    /// stays on the transcript rather than see no conversation at all (a set `view` with the ledger
    /// off is refused at load).
    pub fn assembly_for(&self, place: &str) -> super::assembly::Assembly {
        match &self.assembly {
            Some(a) => a.for_place(place),
            None if !self.ledger_on() => super::assembly::Assembly::Transcript,
            None => Default::default(),
        }
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
        if !self.ledger_on() && self.assembly.as_ref().is_some_and(|a| a.any_view()) {
            anyhow::bail!(
                "context.assembly sets a place to \"view\" while context.ledger = false: a view is read \
                 from the exchange ledger, so it needs the ledger on. Remove `ledger = false`, or set \
                 assembly back to \"transcript\""
            );
        }
        Ok(())
    }

    /// The keys the cut retired (issue 44) that this section sets: what
    /// [`crate::Config::parse`] warns of, once.
    pub fn trimmed(&self) -> Vec<String> {
        (self.kept_mb.iter())
            .map(|_| "context.kept_mb".into())
            .collect()
    }

    /// Whether the exchange ledger is written.
    pub fn ledger_on(&self) -> bool {
        self.ledger.unwrap_or(true)
    }

    /// The window a request compiles against: the model's (`catalog`),
    /// lowered by `working_window_tokens`. `None`: no lower window is set,
    /// so the compiler reads the catalog's own.
    pub fn working_window(&self, catalog: Option<u64>) -> Option<u64> {
        let w = self.working_window_tokens?;
        Some(catalog.map_or(w, |c| c.min(w)))
    }
}
