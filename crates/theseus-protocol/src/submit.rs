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
    /// recorded on `session.opened`; it holds nothing since the hold went.
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
    /// The persona the session speaks as (a key of the config's
    /// `[personas]`; theseus-slvl.2): over its place's and the default's.
    /// A task takes its parent's. Absent: the place's, else the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// The session whose job sent this turn (theseus-b5cl): recorded where the
    /// turn opens a session; it holds nothing since the hold went.
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
    /// Run the turn in this thread's current session (theseus-nhew), opened
    /// on its first use. The thread's slug; only the owner, from a private
    /// place, and not with a `session_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub thread: Option<String>,
    /// Who is speaking and where the answer goes, for a `thread` turn from
    /// the Discord binding (theseus-sjzm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub speaking: Option<crate::threads::Speaking>,
    /// The turn whose reply this message replies to (a Discord reply to one
    /// of this process's replies, theseus-rha2): when that turn ran in a
    /// thread, so does this message, certain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub in_reply_to: Option<String>,
    /// Which of its programs the client is, for the surface tag at the head
    /// of the message (theseus-slvl.3): `ask`, `tui`, or `voice`. A label for
    /// the model's register, never an authority: the connection's own
    /// surface decides the rest, and a word this build does not know is
    /// read as none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub via: Option<String>,
    /// A finer name for the surface than the connection's (theseus-slvl.4):
    /// the Discord binding says `voice` for a spoken turn, which the records
    /// scoped to the `voice` surface hold on. The core reads it from a Discord
    /// connection alone; any other value, or any other connection, is the
    /// connection's own surface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub surface: Option<String>,
    /// The bench's clock, in milliseconds (theseus-98bz.3): the router reads
    /// it as the message's arrival, and a session `ask -s` named is placed as
    /// a message typed in its place. Honoured only under `[threads]
    /// bench_clock`, a test hook; any other daemon refuses it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub route_at_ms: Option<u64>,
    /// The turn's time, in seconds from the request's arrival (theseus-yct32):
    /// the model is told the time it has left once a minute, and the progress
    /// check reads the share used (theseus-2uy56). Kept in memory for the
    /// session until it passes. Absent: no deadline, as before.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub deadline_secs: Option<u64>,
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
