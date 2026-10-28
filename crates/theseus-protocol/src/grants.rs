//! A plan question's other yes (theseus-lyxy): "approve the plan, and move
//! this conversation to edits mode". The question carries the offer
//! (`ConfirmRequest.grant_offer`), worded once here; an answer that takes it
//! echoes its pattern (`ActionConfirmParams.grant`), which the daemon checks
//! against what it offered, so a client can never widen it.
//!
//! Scoped grants ("yes, and don't ask again" for a command's prefix, or for
//! edits in the project) and earned autonomy used to share this file and
//! `earned.rs`; neither could fire under `enforcement = notify`, and both are
//! gone. `SessionGrant` stays only as what the edits mode's matching rule
//! reads (`policy::grants::covers`).

use serde::{Deserialize, Serialize};

/// The answer option that approves and takes the offer (`work.answer`).
pub const APPROVE_GRANT: &str = "approve_grant";

/// The one scope an offer has: this conversation.
pub const SCOPE_SESSION: &str = "session";
/// What edits mode covers: edits under the project directory.
pub const KIND_EDITS: &str = "edits";
/// A plan question's other yes: approve the plan and move the conversation
/// to edits mode. The plan's tool sets the mode.
pub const KIND_MODE: &str = "mode";

/// What a question offers besides a plain yes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct GrantOffer {
    /// `session`: the rest of this conversation.
    pub scope: String,
    /// `mode` (a plan's edits mode).
    pub kind: String,
    /// What it covers; an answer echoes it.
    pub pattern: String,
    /// The offer in words, as every surface shows it.
    pub words: String,
}

impl GrantOffer {
    /// A plan question's offer (theseus-lyxy): the plan, and then edits mode.
    pub fn mode_edits() -> Self {
        Self {
            scope: SCOPE_SESSION.into(),
            kind: KIND_MODE.into(),
            pattern: "edits".into(),
            words:
                "yes, and move this conversation to edits mode (file edits here run with a notice)"
                    .into(),
        }
    }

    /// The key a surface answers it with: `e`, for edits mode.
    pub fn key(&self) -> char {
        'e'
    }

    /// Its answer, beside a plain yes: what `work.answer` takes.
    pub fn option(&self) -> crate::work::AnswerOption {
        crate::work::AnswerOption {
            id: APPROVE_GRANT.into(),
            label: self.button().into(),
            style: crate::work::Style::Primary,
            takes_note: false,
            effect: format!(
                "the call runs, and {}",
                self.words.trim_start_matches("yes, and ")
            ),
        }
    }

    /// A button's label (Discord, the cockpit).
    pub fn button(&self) -> &'static str {
        "Approve the plan + edits mode"
    }
}

/// An answer's taking of the offer: its pattern, echoed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct GrantAnswer {
    pub pattern: String,
}

/// What edits mode's matching rule reads: the kind (`edits`) and the
/// project's directory, canonical. It is not on the wire.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionGrant {
    pub kind: String,
    pub root: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plans_offer_says_its_pattern_and_key() {
        let o = GrantOffer::mode_edits();
        assert_eq!((o.kind.as_str(), o.pattern.as_str()), (KIND_MODE, "edits"));
        assert_eq!(o.key(), 'e');
        assert!(o.words.contains("edits mode"));
        assert_eq!(o.option().id, APPROVE_GRANT);
    }
}
