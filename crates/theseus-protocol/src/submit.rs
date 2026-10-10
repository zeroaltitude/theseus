//! What opens a session and what submits a turn: `session.open`'s and
//! `turn.submit`'s params, moved out of `lib.rs`, which is at its ceiling.

use serde::{Deserialize, Serialize};

use crate::{Attachment, SessionKind};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionOpenParams {
    #[serde(default)]
    pub kind: Option<SessionKind>,
    #[serde(default)]
    pub label: Option<String>,
    /// The session whose job opened this one (`JOB_SESSION_ENV`, theseus-b5cl):
    /// one opened from a session that holds external text holds it too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub opened_from: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnSubmitParams {
    /// Omit to open a fresh conversation session for this turn.
    #[serde(default)]
    pub session_id: Option<String>,
    pub input: String,
    /// Profile for this turn (a configured profile name); default is the live profile.
    #[serde(default)]
    pub profile: Option<String>,
    /// Raw override of the profile's provider for this turn.
    #[serde(default)]
    pub provider: Option<String>,
    /// Raw override of the profile's model for this turn.
    #[serde(default)]
    pub model: Option<String>,
    /// `profile` is carried from the session's last turn (the CLI's pane),
    /// not the owner's choice: routing may still move the turn (M5 25e).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub carried: bool,
    /// Who wrote the input, as a label on the message node (e.g. `discord:zeroaltitude`).
    /// Default: the connection's own label. A label, not an authority: every
    /// local protocol client acts as the operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Files that came with the input, in order (theseus-9g2). With any, the
    /// input may be empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// The surface's message this turn answers (a Discord message id): the
    /// reply's first message is posted as a reply to it (theseus-q4v).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reply_to: Option<String>,
    /// The session whose job sent this turn (theseus-b5cl): the session the
    /// turn opens or names takes its hold of external text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub opened_from: Option<String>,
    /// An MCP server's prompt as the turn's input (M7 36c): the core asks
    /// the server for it, and `input` stays empty. Only a private place's
    /// session may run one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub prompt: Option<crate::mcp::McpPromptRef>,
}
