//! Consolidation (M6 step 31b; design `m6-memory.md` §2.7). The nightly
//! synthesis writer is retired (the owner, 2026-11-21): no recall arm that
//! ran live admitted a synthesis, so it only spent. What stays:
//!
//! - **Reading.** A synthesis a store holds (`node::Body::Synthesis`, kept in
//!   the memory's harness session with a `derived_from` edge to each source)
//!   still decodes, and `info`, `publish`, a retraction and history read it:
//!   [`CitationCheck`] and [`Stage`] are its stored fields. The harness
//!   session stays out of recall ([`MEMORY_SESSION`]).
//! - **The night.** [`tender`] runs the dream's [`guidance`] at
//!   `[memory] consolidate_hour`, under `synth_profile` and
//!   `synth_limit_usd_per_day`, which the dream reads.
//!
//! In the books (spec P8) a synthesis is an encyclopedia entry, by topic:
//! never an SOP (the operator's alone) nor a recipe (promoted after
//! repeated success). No book is built here.

pub mod guidance;
pub mod tender;
#[cfg(test)]
mod tests_guidance;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page, Store as _};

use crate::config::memory::SUMMARY_SESSION;
use crate::fact::synthesis::SCOPE;
use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::session::SessionRecord;

/// The META key naming the memory's harness session.
pub const MEMORY_SESSION: &str = "memory.session";
/// The META key of the nightly run's last time.
pub const LAST_RUN: &str = "memory.consolidated";

/// A synthesis's citation check (`citation.v1`, design §2.7): Jev's verdict
/// on each sentence and source it cites, after the deterministic checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum CitationCheck {
    /// No verdict: the judge is off, its pack is off, or Jev did not answer
    /// (`why`). Never promoted.
    Unchecked { why: String },
    /// Every sentence's every cited source supports it: the least of Jev's
    /// probabilities, at 0.5 or above.
    Supported {
        least: f64,
        judgment: String,
        /// The pack's rung when it answered (`shadow`, `live`).
        mode: String,
    },
}

impl CitationCheck {
    pub fn as_str(&self) -> &'static str {
        match self {
            CitationCheck::Unchecked { .. } => "unchecked",
            CitationCheck::Supported { .. } => "supported",
        }
    }

    pub fn checked(&self) -> bool {
        matches!(self, CitationCheck::Supported { .. })
    }
}

/// Where a synthesis stands. No arm puts one before a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Unchecked: kept, and admitted by no arm.
    Shadow,
    /// Checked and supported (the retired `+synthesis` arm admitted these).
    Arm,
}

impl Stage {
    pub fn of(check: &CitationCheck) -> Self {
        if check.checked() {
            Stage::Arm
        } else {
            Stage::Shadow
        }
    }
}

impl Core {
    /// The rows of `kind` kept under consolidation's scope, oldest first,
    /// through the pages (or the scope, while the index's shape is built);
    /// with `since_ms`, only those from then. The dream counts the day's
    /// spend from them, and the morning note and the persona's pace read
    /// theirs.
    pub(crate) fn synthesis_rows(
        &self,
        kind: LedgerKind,
        since_ms: Option<u64>,
    ) -> anyhow::Result<Vec<LedgerRow>> {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_kind(kind.as_str())],
            since_ms,
            limit: usize::MAX / 2,
            ..Page::default()
        };
        let records = match self.store.inner().page(&page)? {
            Some(out) => out.records,
            None => self
                .store
                .scope_after(SCOPE, 0)?
                .into_iter()
                .filter(|r| r.kind == kinds::LEDGER)
                .collect(),
        };
        Ok(records
            .iter()
            .filter_map(|r| r.decode::<LedgerRow>().ok())
            .filter(|r| r.kind == kind.as_str())
            .filter(|r| since_ms.is_none_or(|s| r.at_unix_ms >= s))
            .collect())
    }

    /// The memory's harness session, where the retired writer kept its
    /// syntheses: found by its META key, as the ladder's is. Never compiled:
    /// no turn runs there, and its place reads private (no target), so a
    /// shared place never draws on it.
    pub fn memory_session(&self) -> anyhow::Result<Option<String>> {
        self.store.get_meta::<String>(MEMORY_SESSION)
    }

    /// `synth_profile`'s rule over `sessions`: the configured profile, or with
    /// `"session"` the one profile every session last used (the dream's
    /// guidance asks it of a category's sessions).
    pub(crate) fn session_profile(
        &self,
        sessions: &[&str],
    ) -> Result<String, (&'static str, String)> {
        // Unset reads as `session` here, for a caller that has no better word.
        // The dream's guidance has: it runs `DREAM_MODEL` when the key is unset
        // and calls this only for a set one (theseus-xdi65).
        let word = self
            .runner
            .memory
            .cfg()
            .synth_profile
            .clone()
            .unwrap_or_else(|| SUMMARY_SESSION.to_string());
        if word != SUMMARY_SESSION {
            return Ok(word);
        }
        let mut seen = BTreeSet::new();
        for session_id in sessions {
            let last = self
                .store
                .get_session::<SessionRecord>(session_id)
                .ok()
                .flatten()
                .and_then(|r| r.last_target);
            match last {
                Some(t) => {
                    seen.insert(t.profile);
                }
                None => {
                    return Err((
                        "no_profile",
                        format!("{session_id} has used no profile yet"),
                    ))
                }
            }
        }
        match seen.len() {
            1 => Ok(seen.into_iter().next().unwrap_or_default()),
            _ => Err((
                "profiles_disagree",
                format!(
                    "the sources' sessions last used {}: synth_profile = \"session\" waits",
                    seen.into_iter().collect::<Vec<_>>().join(", ")
                ),
            )),
        }
    }
}
