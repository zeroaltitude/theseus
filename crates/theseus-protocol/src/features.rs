//! Health's first line (theseus-is8t): which of Theseus's killer features
//! are on, in a few words, each part also a field. The core words it from
//! its config (`theseus_core::features`); every surface shows `line`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Features {
    /// The whole line, as `theseus health` prints it first.
    pub line: String,
    /// Recall: its mode and arm (`live (baseline)`), or `off`.
    pub memory: String,
    /// Jev: routing live or in shadow, or `off` and why.
    pub jev: String,
    /// Outbound substitution: the vault's values replaced on what leaves, on
    /// unless `[privacy] redact_outbound = false`.
    pub scrub: String,
    /// The budgets: the daemon's daily ceiling, and what a session's limit does.
    pub budgets: String,
}
