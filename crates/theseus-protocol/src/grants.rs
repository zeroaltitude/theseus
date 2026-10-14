//! Scoped grants (theseus-drjs): "yes, and don't ask again" for a command's
//! prefix, or for edits inside the project, for the rest of one
//! conversation. A question the tool's posture alone asked carries the
//! offer (`ConfirmRequest.grant_offer`), worded once here; an answer that
//! takes it echoes its pattern (`ActionConfirmParams.grant`), which the
//! daemon checks against what it offered, so a client can never widen it.
//!
//! A grant lives in its session alone and ends with it: `policy.grants`
//! lists the live ones beside the earned grants, and `policy.revoke` ends
//! one by its id (`sgr_…`). It never passes the floor, the approve lists, a
//! place's floor, the external-text hold or an L1 job's own ask: it loosens
//! exactly what an earned grant may, and runs at `notify`, never `open`.

use serde::{Deserialize, Serialize};

use crate::earned::EarnedGrant;

/// The answer option that approves and takes the offer (`work.answer`).
pub const APPROVE_GRANT: &str = "approve_grant";

/// The one scope a grant has today: this conversation.
pub const SCOPE_SESSION: &str = "session";
/// A grant on a command's argv prefix.
pub const KIND_PREFIX: &str = "prefix";
/// A grant on edits under the project directory.
pub const KIND_EDITS: &str = "edits";

/// What a question offers besides a plain yes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct GrantOffer {
    /// `session`: the rest of this conversation.
    pub scope: String,
    /// `prefix` (a command's prefix) or `edits` (edits in the project).
    pub kind: String,
    /// What it covers: the prefix's words (`python3 -m unittest`), or the
    /// project's directory, canonical. An answer echoes it.
    pub pattern: String,
    /// The offer in words, as every surface shows it.
    pub words: String,
}

impl GrantOffer {
    /// The offer for a command's prefix.
    pub fn prefix(prefix: &[String]) -> Self {
        let pattern = prefix.join(" ");
        Self {
            scope: SCOPE_SESSION.into(),
            kind: KIND_PREFIX.into(),
            words: format!("yes, and don't ask again for `{pattern}` in this conversation"),
            pattern,
        }
    }

    /// The offer for edits under the project's directory.
    pub fn edits(root: &str) -> Self {
        Self {
            scope: SCOPE_SESSION.into(),
            kind: KIND_EDITS.into(),
            words: format!("yes, and don't ask again for edits under {root} in this conversation"),
            pattern: root.into(),
        }
    }

    /// The key a surface answers it with: `a` for a command, `e` for edits.
    pub fn key(&self) -> char {
        if self.kind == KIND_EDITS {
            'e'
        } else {
            'a'
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
        if self.kind == KIND_EDITS {
            "Approve + edits in this project for the conversation"
        } else {
            "Approve + this command for the conversation"
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

/// A live grant, as `policy.grants` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct SessionGrant {
    /// `sgr_` and eight hex characters.
    pub id: String,
    pub session_id: String,
    pub scope: String,
    pub kind: String,
    pub pattern: String,
    /// A prefix grant's words, the program by its base name.
    pub prefix: Vec<String>,
    /// A prefix grant's program exactly as the call wrote it: a call of
    /// another path to a program of the same name is not covered.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub program: Option<String>,
    /// An edits grant's directory, canonical.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub root: Option<String>,
    pub words: String,
    /// The question that offered it.
    pub correlation_id: String,
    /// Who granted it, by label, and as the place rule saw them.
    pub by: String,
    pub who: String,
    pub via: String,
    pub at_ms: u64,
}

impl SessionGrant {
    /// Its line in `theseus policy grants`.
    pub fn line(&self) -> String {
        let what = match self.kind.as_str() {
            KIND_EDITS => format!("edits under {}", self.pattern),
            _ => self.pattern.clone(),
        };
        format!(
            "{}: {what} (this conversation, {}; granted by {})",
            self.id, self.session_id, self.by
        )
    }
}

/// `policy.grants`: the live grants, a session's alone when one is named.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyGrantsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyGrantsResult {
    /// The conversations' grants, oldest first.
    pub session: Vec<SessionGrant>,
    /// The earned grants, which hold in every private place.
    pub earned: Vec<EarnedGrant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_offer_says_its_pattern_and_key() {
        let o = GrantOffer::prefix(&["python3".into(), "-m".into(), "unittest".into()]);
        assert_eq!(o.pattern, "python3 -m unittest");
        assert_eq!(o.key(), 'a');
        assert!(o.words.contains("`python3 -m unittest`"));
        let e = GrantOffer::edits("/work/app");
        assert_eq!((e.kind.as_str(), e.key()), (KIND_EDITS, 'e'));
        assert!(e.words.contains("edits under /work/app"));
    }
}
