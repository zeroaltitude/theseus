//! The owner's second proof (issue 22): a code from the owner's own
//! authenticator app. Wire types and pure words only, like `answer.rs`.
//!
//! **A code is typed into a surface's own form and nowhere else**: a card's
//! form, `theseus answer --code`, a cockpit field. It rides an RPC request to
//! the daemon and is checked there; it is never a message, a content item, a
//! transcript line or a ledger row. [`ProofCode`] is the one type that holds
//! it on the wire, and it prints as nothing.

use serde::{Deserialize, Serialize};

/// A code from the owner's authenticator app, as a request carries it. Its
/// `Debug` says nothing of the digits, so a request printed for a log or a
/// test failure leaks nothing.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(transparent)]
pub struct ProofCode(String);

impl ProofCode {
    pub fn new(code: impl Into<String>) -> Self {
        Self(code.into())
    }

    /// The digits, for the daemon's check alone.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Nothing typed: a form left empty is no code.
    pub fn is_blank(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl std::fmt::Debug for ProofCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProofCode(..)")
    }
}

/// The key in a refusal's `data` that says what the proof needs, so a surface
/// can open its form: `needed`, `wrong`, `used`, `locked`, `not_enrolled`,
/// `unavailable`.
pub const REFUSAL_KEY: &str = "proof";

/// What an answer from one place would be asked for, as a card shows it: the
/// daemon's words, computed for the connection that lists the question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProofStanding {
    /// A code is needed for an answer from here.
    pub code_needed: bool,
    /// No authenticator app is enrolled: the question cannot be approved at
    /// all until one is.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub not_enrolled: bool,
    /// When a proof was given from this place, and when it stops covering
    /// answers from here: set only when the card goes through on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub given_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub until_ms: Option<u64>,
    /// The card's line, in plain words: "second proof: needed, enter the
    /// code" or "second proof: given here 3 hours ago, good for 21 hours more".
    pub line: String,
}

/// `owner.proof.enroll`: begin pairing the owner's app. The CLI sends it from
/// the owner's own shell and nowhere else.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnerProofEnrollParams {
    /// The shell's `THESEUS_SESSION`: set inside a Theseus job, which is
    /// refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// What the app is told, once, to this terminal alone.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnerProofEnrollResult {
    /// The `otpauth://` address an app scans or takes pasted.
    pub uri: String,
    /// The secret, base32, for typing by hand.
    pub secret: String,
}

impl std::fmt::Debug for OwnerProofEnrollResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OwnerProofEnrollResult(..)")
    }
}

/// `owner.proof.confirm` (the app's first code, which keeps the enrolment),
/// `owner.proof.reset` (a valid code takes it away) and `owner.proof.lock`
/// carry the same shape: a code (none for a lock) and where the shell is.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnerProofCodeParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub code: Option<ProofCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub from_job: Option<String>,
}

/// An open window: a place a good code covers for a while.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProofWindow {
    /// `cli`, `web`, `discord:dm`, or `discord:<channel id>`.
    pub place: String,
    pub opened_at_ms: u64,
    pub until_ms: u64,
}

/// `owner.proof.status`'s answer: whether one is enrolled and when, which
/// windows are open, and whether checking is locked. Never the secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnerProofStatus {
    pub enrolled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub enrolled_at_ms: Option<u64>,
    /// Codes are not being checked before this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub locked_until_ms: Option<u64>,
    /// The file is there and cannot be used.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unavailable: bool,
    /// How long a good code covers the place it came from, in hours.
    pub window_hours: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<ProofWindow>,
}

/// What a `confirm`, a `reset` or a `lock` did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct OwnerProofDone {
    /// A lock: how many open windows it closed.
    #[serde(default)]
    pub closed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request printed for a log or a failing test says nothing of the digits.
    #[test]
    fn a_code_prints_as_nothing_and_travels_as_a_plain_string() {
        let code = ProofCode::new("123456");
        assert_eq!(format!("{code:?}"), "ProofCode(..)");
        let params = OwnerProofCodeParams {
            code: Some(code),
            from_job: None,
        };
        assert!(!format!("{params:?}").contains("123456"));
        assert_eq!(
            serde_json::to_string(&params).unwrap(),
            r#"{"code":"123456"}"#
        );
        let back: OwnerProofCodeParams = serde_json::from_str(r#"{"code":"123456"}"#).unwrap();
        assert_eq!(back.code.unwrap().expose(), "123456");
    }

    #[test]
    fn what_the_app_is_told_prints_as_nothing() {
        let r = OwnerProofEnrollResult {
            uri: "otpauth://totp/x?secret=ABC".into(),
            secret: "ABC".into(),
        };
        assert!(!format!("{r:?}").contains("ABC"));
    }
}
