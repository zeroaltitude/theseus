//! Consolidation (M6 step 31b; design `m6-memory.md` §2.7): off every turn,
//! the nodes recall admits together become one short cited synthesis per
//! cluster, kept as a `Synthesis` node (`node::Body::Synthesis`) in the
//! memory's harness session, with a `derived_from` edge to each source.
//!
//! In the books (spec P8) a synthesis is an encyclopedia entry, by topic:
//! never an SOP (the operator's alone) nor a recipe (promoted after
//! repeated success). No book is built here.

pub mod run;
pub mod tender;

use serde::{Deserialize, Serialize};

/// A synthesis's citation check (`citation.v1`, design §2.7): Jev's verdict
/// on each sentence and source it cites, after the deterministic checks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum CitationCheck {
    /// No verdict: the judge is off, its pack is off, or Jev did not answer
    /// (`why`). Never promoted, and no arm admits it.
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

/// Where a synthesis stands. It is never shown in shadow: only the
/// `+synthesis` arm puts a checked one before a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Unchecked: kept, and admitted by no arm.
    Shadow,
    /// Checked and supported: the `+synthesis` arm may admit it.
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
