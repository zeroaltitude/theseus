//! Google Workspace's wire types (the Google design, §2.2): `google.auth.begin`
//! and `.finish` (the one-time consent), `google.revoke`, and `google.status`.
//! All four are the CLI's alone. A token, a code and a client secret are in
//! none of them; the paste is in `GoogleAuthFinishParams` and never leaves the
//! daemon's memory.

use serde::{Deserialize, Serialize};

/// `google.auth.begin`: start a consent. `manual` skips the loopback listener
/// and goes straight to the paste.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleAuthBeginParams {
    #[serde(default)]
    pub manual: bool,
}

/// The consent URL, which holds no secret (the client id, the PKCE challenge, a
/// state), and how the redirect will come.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleAuthBeginResult {
    /// Names this consent for `google.auth.finish`.
    pub auth_id: String,
    pub url: String,
    /// The loopback redirect the daemon is listening on; empty with `manual`.
    pub redirect: String,
    pub account: String,
    /// The scopes the consent asks for, by service, in words.
    pub asks: Vec<String>,
    /// How long the listener waits, in seconds, and when the CLI should offer the paste.
    pub expires_secs: u64,
    pub paste_after_secs: u64,
}

/// `google.auth.finish`: wait for the redirect (`pasted` empty), or redeem the
/// address bar the owner pasted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleAuthFinishParams {
    pub auth_id: String,
    /// The address bar of the page that failed to load, from the terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pasted: Option<String>,
    /// With no paste: wait this long for the redirect (seconds), then answer
    /// `waiting` so the CLI can ask for the paste.
    #[serde(default)]
    pub wait_secs: u64,
}

/// What the consent came to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleAuthFinishResult {
    /// `done`, `waiting` (no redirect yet: paste the address), or `refused`.
    pub state: String,
    pub account: String,
    /// The services granted, and the words for each one that was not.
    pub granted: Vec<String>,
    pub not_granted: Vec<String>,
    pub tools_on: Vec<String>,
    /// Where the refresh token was kept: `op`, `file`, or `memory` (the write
    /// was refused: it works until the next restart).
    pub store: String,
    /// Why it was refused, or why the store write was.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// `google.revoke`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleRevokeParams {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleRevokeResult {
    pub revoked_at_google: bool,
    pub cleared: bool,
    /// Anything the owner should know: a store that could not be cleared, or a
    /// revoke Google did not answer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// `google.status`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleStatusParams {}

/// What health and `theseus google status` show.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GoogleStatus {
    pub account: String,
    /// `ok`, `needs_auth`, `checking`, or `off` (no `[google]`).
    pub state: String,
    pub services: Vec<String>,
    /// The scopes Google granted, as it names them.
    pub granted: Vec<String>,
    /// The tools the granted scopes turn on.
    pub tools_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_refresh_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
    pub sends_today: u32,
    pub max_sends_per_day: u32,
    /// Where the refresh token is kept: `op`, `file`, `env`, or `memory`.
    pub store: String,
}
