//! `context.census` (theseus-7m7w.1): the next request of a session, or a fresh session's first request for a
//! kind of place, compiled and counted part by part down to each tool's schema, and never sent. A read: it
//! compiles once, writes nothing, and calls no model (`exact` calls the provider's token counter, which is
//! not a model call).
//!
//! Every count is at the model's figures (the compiler's own estimate), apportioned so the parts add up to
//! the total exactly: a tool's description, its schema and the rest of its definition are its tokens, the
//! tools are the request's tools, and the system, the tools and the messages (with images) are the total.

use serde::{Deserialize, Serialize};

/// The kinds of place a fresh session's first request is counted for.
pub const PLACES: [&str; 4] = ["cli", "dm", "task", "harness"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextCensusParams {
    /// The session whose next request is counted; none counts a fresh session's first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// With no session: `cli`, `dm`, `task` or `harness` (default `cli`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    /// Also count with the provider's `count_tokens`, when a key resolves.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub exact: bool,
}

/// One section of a system block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CensusPart {
    /// `persona`, `assembly`, `precedence`, `tools note`, `profile`; `standing orders`; a context file's path;
    /// a category's id; `joins` (the blank lines between sections); `directory`.
    pub name: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
}

/// One system block (`header`, `context`, `session`): its sections, and the framing the provider counts for
/// the block.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CensusBlock {
    pub block: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// The sections' tokens and the framing.
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub framing: u64,
    pub parts: Vec<CensusPart>,
}

/// One tool as the request carries it, with its use.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CensusTool {
    /// The name the model sees.
    pub name: String,
    /// The definition as JSON, in bytes: what `tokens` counts.
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub description_bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub schema_bytes: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub description_tokens: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub schema_tokens: u64,
    /// The whole definition's: the description, the schema and the name's and keys' own.
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    /// Calls planned in the census's window, across the store's sessions.
    #[cfg_attr(test, ts(type = "number"))]
    pub calls: u64,
    /// The last of them, unix ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub last_call_ms: Option<u64>,
    /// Sent with `defer_loading` (theseus-7m7w.2): its definition is in the
    /// request, and the estimate counts one constant for every deferred tool,
    /// on the first one's line.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deferred: bool,
}

/// The provider's own counts of the same request, by `count_tokens`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CensusExact {
    #[cfg_attr(test, ts(type = "number"))]
    pub total: u64,
    /// What the system, the tools and the messages each add to the same request's smallest form.
    #[cfg_attr(test, ts(type = "number"))]
    pub system: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub tools: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub messages: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextCensusResult {
    /// The session counted, or none for a fresh session's first request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// `cli`, `dm`, `task`, `harness`, or `session`.
    pub place: String,
    /// The place class the request is built for (`private`, `shared`).
    pub class: String,
    pub profile: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub window: Option<u64>,
    /// How the request is made: `fresh` (no history), `append` or `recompile` (a session's next).
    pub compile: String,
    pub system: Vec<CensusBlock>,
    #[cfg_attr(test, ts(type = "number"))]
    pub system_tokens: u64,
    pub tools: Vec<CensusTool>,
    #[cfg_attr(test, ts(type = "number"))]
    pub tools_tokens: u64,
    /// The conversation's messages, and what a recalled note in them comes to (a part of them).
    #[cfg_attr(test, ts(type = "number"))]
    pub messages_tokens: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub recall_tokens: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub images_tokens: u64,
    /// The system, the tools, the messages and the images: the compile's estimate.
    #[cfg_attr(test, ts(type = "number"))]
    pub total_tokens: u64,
    /// The compile's own figure, which counts the provider's tokens of what a session's last request repeated;
    /// the same as `total_tokens` for a fresh session.
    #[cfg_attr(test, ts(type = "number"))]
    pub compile_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub exact: Option<CensusExact>,
    /// Why `exact` is absent when it was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub exact_error: Option<String>,
    /// The window the tools' use was counted in: unix ms from, and days.
    #[cfg_attr(test, ts(type = "number"))]
    pub use_since_ms: u64,
    pub use_days: u32,
    /// Why the use is absent (the index is being built).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub use_unread: Option<String>,
    /// What the census could not say, or said another way.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    pub ms: f64,
}

impl ContextCensusResult {
    /// The parts' tokens, which are the total: the system, the tools, the messages and the images.
    pub fn parts_sum(&self) -> u64 {
        self.system_tokens + self.tools_tokens + self.messages_tokens + self.images_tokens
    }
}
