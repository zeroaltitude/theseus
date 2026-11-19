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
    /// Who gave it, as the Discord binding names the presser of a form. Only
    /// that binding's connection may name one; the daemon judges it as it
    /// judges every press (`DiscordOrigin`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Where the binding read the press: the presser's user and channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
    /// The binding's form already found the value one the daemon would refuse, and sends no value: the
    /// request is how the daemon records the refused attempt, by class. The daemon judges who and where
    /// first, takes nothing from `value`, and answers in its own words for the class.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub refused: Option<ValueRefusal>,
}

/// Why a value was refused before it was sent: a closed set, so a row says which and holds nothing typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ValueRefusal {
    /// Nothing was typed.
    Blank,
    /// Too long to be a key, or holding a NUL.
    NotAValue,
}

impl ValueRefusal {
    /// The class a row is written with.
    pub fn class(self) -> &'static str {
        match self {
            ValueRefusal::Blank => "blank",
            ValueRefusal::NotAValue => "not_a_value",
        }
    }
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

/// `vault.keep.waiting`: the first thing `theseus vault keep` asks, before it
/// prompts for anything: which keep cards wait for the owner's terminal and,
/// when there is none, why. The daemon answers in words; the terminal filters
/// nothing of its own. It is also how a run that ends before a value reaches
/// the daemon says so (`ended`), so that every run leaves a row.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VaultKeepWaitingParams {
    /// A card's id, or the end of one, when the terminal was given one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    /// Keep with no card: the vault, item and field typed in the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub vault: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub field: Option<String>,
    /// The shell's `THESEUS_SESSION`: set inside a Theseus job, which is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
    /// Standard input and standard error are both a terminal: the prompts are
    /// seen and the value is typed where only the owner sees it.
    #[serde(default = "yes")]
    pub terminal: bool,
    /// How the run ended before a value was sent, when this reports an end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended: Option<ClientEnd>,
}

fn yes() -> bool {
    true
}

/// How a terminal's run ended without sending a value, as it reports it: a
/// closed set, so a row says what stopped it and holds nothing typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ClientEnd {
    /// Nothing was typed, twice.
    NoValue,
    /// The terminal gave end of input before a line.
    EndOfInput,
    /// The terminal could not be read.
    Unreadable,
    /// The terminal's echo could not be turned off, so nothing was asked.
    NoEcho,
}

impl ClientEnd {
    pub fn name(self) -> &'static str {
        match self {
            ClientEnd::NoValue => "no_value",
            ClientEnd::EndOfInput => "end_of_input",
            ClientEnd::Unreadable => "unreadable",
            ClientEnd::NoEcho => "no_echo",
        }
    }
}

/// One keep the terminal may answer: a card that waits, or the target typed in
/// the command. By name; never a value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KeepCard {
    /// The card's id; none for a keep with no card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question_id: Option<String>,
    pub vault: String,
    pub title: String,
    pub field: String,
    /// `new`, `adds` or `replaces`.
    pub state: String,
    /// A code from the authenticator app is asked with the value.
    pub code_needed: bool,
    /// The daemon's heading: what is kept where, and that the value is typed.
    pub heading: String,
}

/// What `vault.keep.waiting` answers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct VaultKeepWaiting {
    pub cards: Vec<KeepCard>,
    /// When there is no card: why, in plain words (none asked, expired,
    /// declined, answered, kept).
    pub none: String,
    /// When something stops a prompt from being useful (no authenticator app
    /// enrolled, a vault the account cannot write to): said before any prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub blocked: Option<String>,
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
            ..Default::default()
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
