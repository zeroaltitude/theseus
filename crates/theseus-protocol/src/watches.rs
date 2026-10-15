//! Watches (theseus-f2r2b; the horizon report's §5.1): waiting on the world
//! with no model. A watch is a check (a command, a fetch, a file) run on a
//! timer until a condition holds, and then one thing happens: a notice, a
//! turn of the conversation that set it, or a task. The wire shapes of the
//! owner's `watch.create` and `watch.list`; the model's `watch` tool takes the
//! same fields.

use serde::{Deserialize, Serialize};

/// What a watch looks at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WatchCheck {
    /// A command, run in L1 as `proc.run` runs one; its output is the value.
    Run {
        argv: Vec<String>,
        /// A JSON path into the output (`.state`, `.checks[0].status`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        json: Option<String>,
    },
    /// An HTTP GET; the body is the value.
    Fetch {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        json: Option<String>,
    },
    /// A file: its text is the value, and `exists` asks whether it is there.
    File { path: String },
    /// A reply (theseus-rlnbf): a new message in `place` (a bound place's name,
    /// `#ops`, or its target) that `from` wrote (a user id, `discord:<id>`, or a
    /// name) and whose text has `words` (text, or `/pattern/`). Nothing polls:
    /// the Discord binding tells the core of each message as it arrives.
    Reply {
        place: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        words: Option<String>,
    },
}

/// What happens when the condition holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchThen {
    /// `notice` (the default), `turn` or `task`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// With `task`: what it is to do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub brief: Option<String>,
}

/// `watch.create`: a watch on the owner's word, with no model turn. The
/// owner types the command, so no approval stands in front of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchCreateParams {
    /// What it waits for, as a question: `PR 123 merged?`.
    pub title: String,
    pub check: WatchCheck,
    /// The condition: `exit 0`, `equals X`, `contains X`, `matches RE`,
    /// `changes`, `above N`, `below N`, `exists`. A `reply` check has none.
    #[serde(default)]
    pub until: String,
    /// How often it checks: `1m` at the most often, `5m` by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub every: Option<String>,
    /// It gives up after this long: `3d`; 30 days at the most, and by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub for_at_most: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub then: Option<WatchThen>,
    /// The conversation it lives in; none is the owner's Home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Where a bound transport says it came from (the place rule's input).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<crate::DiscordOrigin>,
}

/// An open watch, as `watch.list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchInfo {
    pub wake_id: String,
    pub short: String,
    pub session_id: String,
    pub execution_id: String,
    pub title: String,
    /// `run`, `fetch`, `file` or `reply`.
    pub kind: String,
    /// The command, the URL or the path, as people read it.
    pub target: String,
    pub until: String,
    pub every: String,
    pub then: String,
    pub set_at_ms: u64,
    /// It gives up at this time.
    pub expires_at_ms: u64,
    /// The last value it saw; none before its first check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub value: Option<String>,
    /// Checks made, as the last frame counted them.
    pub checks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_checked_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub next_check_ms: Option<u64>,
    /// Why its last checks failed, when they did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub failing: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchCreateResult {
    pub watch: WatchInfo,
    /// It lives in the owner's Home conversation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub home: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchListParams {
    /// Only this conversation's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WatchListResult {
    pub watches: Vec<WatchInfo>,
}
