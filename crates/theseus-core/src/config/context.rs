//! `[context]`: the context files in two levels (theseus-c48), the
//! working window (theseus-3ki0), and the sizes of a turn's look-backs
//! (issue 48).

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
    /// HERE (issue 48): a conversation's newest exchanges that no fold
    /// takes, the window's floor; the fold step stays `ladder::FOLD_STEP`.
    /// Absent: `ladder::WINDOW_FLOOR` (30); 10 to 60. Lowered, each place's
    /// next exchange end folds what is over it; raised, nothing unfolds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub here_exchanges: Option<u64>,
    /// EVERYWHERE (issue 48): the newest exchanges from every place HERE
    /// does not show, laid beside the message. Absent:
    /// [`EVERYWHERE_EXCHANGES`] (15); 5 to 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub everywhere_exchanges: Option<u64>,
    /// The history tool's page, in exchanges (issue 48). Absent:
    /// `view::history::PAGE_EXCHANGES` (100); 20 to 200. The tool's read
    /// bounds stay as they are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_page_exchanges: Option<u64>,
}

/// `persona_max_tokens`' default and bounds.
pub const PERSONA_MAX_TOKENS: u64 = 2000;
pub const PERSONA_MAX_TOKENS_RANGE: std::ops::RangeInclusive<u64> = 200..=8000;

/// The look-backs' bounds, and EVERYWHERE's default (HERE's and the page's
/// are the constants their readers had).
pub const HERE_EXCHANGES_RANGE: std::ops::RangeInclusive<u64> = 10..=60;
pub const EVERYWHERE_EXCHANGES: usize = 15;
pub const EVERYWHERE_EXCHANGES_RANGE: std::ops::RangeInclusive<u64> = 5..=50;
pub const HISTORY_PAGE_EXCHANGES_RANGE: std::ops::RangeInclusive<u64> = 20..=200;

/// The sizes of a turn's look-backs, in exchanges: `[context]`'s, each
/// absent one its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// HERE: the window's floor.
    pub here: usize,
    /// EVERYWHERE: the newest exchanges from every place HERE does not show.
    pub everywhere: usize,
    /// The history tool's page.
    pub page: usize,
}

impl Default for Sizes {
    fn default() -> Self {
        ContextConfig::default().sizes()
    }
}

impl ContextConfig {
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
            && self.default_persona.is_none()
            && self.working_window_tokens.is_none()
            && self.persona_max_tokens.is_none()
            && self.here_exchanges.is_none()
            && self.everywhere_exchanges.is_none()
            && self.history_page_exchanges.is_none()
    }

    /// The look-backs' sizes, each absent one at its default.
    pub fn sizes(&self) -> Sizes {
        let or = |n: Option<u64>, default: usize| n.map_or(default, |n| n as usize);
        Sizes {
            here: or(self.here_exchanges, crate::ladder::WINDOW_FLOOR),
            everywhere: or(self.everywhere_exchanges, EVERYWHERE_EXCHANGES),
            page: or(
                self.history_page_exchanges,
                crate::view::history::PAGE_EXCHANGES,
            ),
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
        let d = Sizes::default();
        for (key, n, range, what, default) in [
            (
                "here_exchanges",
                self.here_exchanges,
                HERE_EXCHANGES_RANGE,
                "the newest exchanges of a conversation kept whole",
                d.here,
            ),
            (
                "everywhere_exchanges",
                self.everywhere_exchanges,
                EVERYWHERE_EXCHANGES_RANGE,
                "the newest exchanges from every other place, laid beside the message",
                d.everywhere,
            ),
            (
                "history_page_exchanges",
                self.history_page_exchanges,
                HISTORY_PAGE_EXCHANGES_RANGE,
                "the exchanges on one page of the history tool",
                d.page,
            ),
        ] {
            if let Some(n) = n.filter(|n| !range.contains(n)) {
                let (low, high) = (range.start(), range.end());
                anyhow::bail!(
                    "context.{key} = {n} is out of range: {low} to {high} ({what}; default {default})"
                );
            }
        }
        Ok(())
    }

    /// The window a request compiles against: the model's (`catalog`),
    /// lowered by `working_window_tokens`. `None`: no lower window is set,
    /// so the compiler reads the catalog's own.
    pub fn working_window(&self, catalog: Option<u64>) -> Option<u64> {
        let w = self.working_window_tokens?;
        Some(catalog.map_or(w, |c| c.min(w)))
    }
}

#[cfg(test)]
mod tests;
