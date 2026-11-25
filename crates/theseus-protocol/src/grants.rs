//! A question's other yes, beside a plain one: a plan's (theseus-lyxy),
//! "approve the plan, and move this conversation to edits mode"; and a held
//! look's (issue 54), "approve, and clear the hold". The question carries the offer
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
/// A look, the settings read or a profile call held after outside text or by
/// the `injection` flag: approve it and clear the hold. Its pattern says which.
pub const KIND_CLEAR: &str = "clear";
/// What a clear's press clears: outside text read since the owner spoke.
pub const CLEAR_OUTSIDE: &str = "outside";
/// What a clear's press clears: the session's `injection` flag.
pub const CLEAR_FLAG: &str = "flag";

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

    /// A held look's offer (issue 54): approve the call and clear the hold
    /// that asked, the flag's when `flag`, else outside text's.
    pub fn clear(flag: bool) -> Self {
        let (pattern, words) = match flag {
            true => (
                CLEAR_FLAG,
                "yes, and clear the injection flag (what it flagged stays in this conversation)",
            ),
            false => (
                CLEAR_OUTSIDE,
                "yes, and stop asking until this conversation reads text from outside again",
            ),
        };
        Self {
            scope: SCOPE_SESSION.into(),
            kind: KIND_CLEAR.into(),
            pattern: pattern.into(),
            words: words.into(),
        }
    }

    /// The key a surface answers it with: `e` for edits mode, `a` for a clear.
    pub fn key(&self) -> char {
        match self.kind.as_str() {
            KIND_CLEAR => 'a',
            _ => 'e',
        }
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
        match self.kind.as_str() {
            KIND_CLEAR => "Approve and clear",
            _ => "Approve the plan + edits mode",
        }
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

    /// A held look's offer clears the hold that asked: its own key, label and
    /// pattern, and an answer that says what the press does.
    #[test]
    fn a_holds_offer_says_what_it_clears() {
        for (flag, pattern) in [(true, CLEAR_FLAG), (false, CLEAR_OUTSIDE)] {
            let o = GrantOffer::clear(flag);
            assert_eq!((o.kind.as_str(), o.pattern.as_str()), (KIND_CLEAR, pattern));
            assert_eq!((o.key(), o.button()), ('a', "Approve and clear"));
            assert_eq!(o.option().id, APPROVE_GRANT);
            assert_eq!(o.option().label, "Approve and clear");
            assert!(o.option().effect.starts_with("the call runs, and "), "{:?}", o.option());
        }
        assert!(GrantOffer::clear(true).words.contains("flag"));
        assert!(GrantOffer::clear(false).words.contains("outside"));
    }
}
