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
    /// The directory the session works in (theseus-aab7): an absolute
    /// path, the client's own current directory. Its tools' default `cwd`,
    /// the base of their relative paths, and the session's system block
    /// name it. Absent: the daemon's `[tools] cwd`, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dir: Option<String>,
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
    /// The directory the session works in (theseus-aab7), as
    /// `SessionOpenParams.dir`: the session this turn opens is created
    /// there, and a session named by `session_id` moves there. Absent: a new
    /// session takes the daemon's `[tools] cwd`, and a named one keeps its
    /// own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dir: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// An older peer's bytes, with no `dir`, decode with none, and encode
    /// back without the key; a newer client's carry it both ways.
    #[test]
    fn dir_is_optional_on_the_wire() {
        let old = json!({"input": "hi"});
        let p: TurnSubmitParams = serde_json::from_value(old).unwrap();
        assert_eq!(p.dir, None);
        assert!(serde_json::to_value(&p).unwrap().get("dir").is_none());
        let new = json!({"input": "hi", "dir": "/work/harbour-tides"});
        let p: TurnSubmitParams = serde_json::from_value(new).unwrap();
        assert_eq!(p.dir.as_deref(), Some("/work/harbour-tides"));
        assert_eq!(
            serde_json::to_value(&p).unwrap()["dir"],
            "/work/harbour-tides"
        );
        let o: SessionOpenParams = serde_json::from_value(json!({})).unwrap();
        assert_eq!(o.dir, None);
        let o: SessionOpenParams =
            serde_json::from_value(json!({"dir": "/work/harbour-tides"})).unwrap();
        assert_eq!(
            serde_json::to_value(&o).unwrap()["dir"],
            "/work/harbour-tides"
        );
        let r = crate::TurnSubmitResult::default();
        assert!(serde_json::to_value(&r)
            .unwrap()
            .get("outside_roots")
            .is_none());
    }
}
