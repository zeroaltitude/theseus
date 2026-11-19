//! Keeping a new secret in the vault (issue 26): the owner gives its value in
//! a surface's own form, never as a chat message, and it rides one request to
//! the daemon, which writes it with its own token.
//!
//! **The value is in this crate in one type, [`SecretText`], and it prints as
//! nothing**: a request logged for a failing test, an error or a trace shows
//! `SecretText(..)`. It is never a field of a stored record, a card, a ledger
//! row or a result: the daemon's answer names the vault, the item and the
//! field, and whether it replaced one.

use serde::{Deserialize, Serialize};

use crate::owner_proof::ProofCode;

/// A secret's value as a request carries it.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(transparent)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The text, for the daemon's write alone.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Nothing typed: a form left empty is no value.
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretText(..)")
    }
}

/// `vault.keep.answer`: the owner gives the value of the card a turn's
/// `vault.keep` put to them. The target is the question's own, kept when it was
/// asked; this names only the question.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VaultKeepAnswerParams {
    pub question_id: String,
    pub value: SecretText,
    /// The owner's second proof, for a card that needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub code: Option<ProofCode>,
    /// The shell's `THESEUS_SESSION`: set inside a Theseus job, which is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// `vault.keep`: the owner keeps a secret from their own terminal, with no
/// card and no turn: the target is typed in the command, and the value at a
/// hidden prompt. The one way to replace an item Theseus's own settings read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VaultKeepParams {
    pub vault: String,
    pub title: String,
    pub field: String,
    pub value: SecretText,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub code: Option<ProofCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// What a keep did, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VaultKept {
    pub vault: String,
    pub title: String,
    pub field: String,
    /// An existing value was replaced.
    pub replaced: bool,
    /// The daemon's words for the owner: where it is, and that it reads back.
    pub words: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request printed for a log or a failing test says nothing of the value,
    /// whether it is held alone or in either request.
    #[test]
    fn a_value_prints_as_nothing_and_travels_as_a_plain_string() {
        let value = SecretText::new("INVENTED-VALUE-FOR-TESTS");
        assert_eq!(format!("{value:?}"), "SecretText(..)");
        let answer = VaultKeepAnswerParams {
            question_id: "act_1".into(),
            value: value.clone(),
            code: Some(ProofCode::new("123456")),
            from_job: None,
        };
        let direct = VaultKeepParams {
            vault: "v".into(),
            title: "t".into(),
            field: "f".into(),
            value: value.clone(),
            ..Default::default()
        };
        for shown in [
            format!("{answer:?}"),
            format!("{direct:?}"),
            format!("{answer:#?}"),
        ] {
            assert!(!shown.contains("INVENTED"), "{shown}");
        }
        let wire = serde_json::to_value(&answer).unwrap();
        assert_eq!(wire["value"], "INVENTED-VALUE-FOR-TESTS");
        let back: VaultKeepAnswerParams = serde_json::from_value(wire).unwrap();
        assert_eq!(back, answer);
        assert!(SecretText::new(" \n ").is_blank() && !value.is_blank());
    }
}
