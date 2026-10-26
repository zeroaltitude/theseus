//! `context.explain` (theseus-7n3e): what a turn sees, part by part, with
//! token counts, for the cockpit's context explorer. The system block's text
//! is in no record (a compilation keeps its digest), so the core builds the
//! parts as the session's next turn would (`request_spec` and the compile
//! walk, the compiler's own inputs), without compiling or calling a model,
//! and says whether they are still the bytes the asked turn saw: their
//! digest against the one its compilation recorded. A read: it writes
//! nothing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::import::ImportedEpisode;
use crate::memory::RecallManifest;
use crate::{CompilationInfo, ContextCompiled};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextExplainParams {
    pub session_id: String,
    /// The turn; default the session's latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
}

/// One part of a request, in the request's order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextPart {
    /// `header` (the system's first block), `context` (a context file in its
    /// second), `guidance` (a category's section after the files), `tools`,
    /// `recall` (an assembled prefix's recall section), or `conversation`.
    pub block: String,
    /// `persona`, `assembly`, `precedence`, `tools note`, `profile`; a
    /// file's path; a category's id (`preamble` for the guidance's first
    /// section); a recalled source's header.
    pub name: String,
    /// Its UTF-8 length; `0` on a detour's parts, whose row records tokens only.
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes: u64,
    /// Its tokens at the model's figures (`TokenRates`), as the compiler
    /// estimates a request; the conversation's are the rest of the turn's
    /// estimate.
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
    /// Against the turn's compilation: `same` (the digest it recorded),
    /// `changed`, or `new` (it carried no such part). Absent when there is
    /// no turn to compare, or the part has no digest of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub then: Option<String>,
    /// Its version (a guidance's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub version: Option<u32>,
    /// Its text, to a private place only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// What else to know: a file missing or cut, a shared place's file
    /// carried as its header alone, the tools' names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub note: Option<String>,
}

/// A turn of the session, as its compiles recorded it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextTurn {
    pub turn_id: String,
    /// Its first compile's time.
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub loops: u32,
    /// Its last loop's estimate.
    #[cfg_attr(test, ts(type = "number"))]
    pub est_tokens: u64,
    pub compilation_id: String,
    /// The turn detoured (`context.detoured`): it compiled no context.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub detoured: bool,
}

/// One part of a detour's request: its block and name, and its tokens.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DetourPart {
    /// `header`, `session`, `tools`, `persona`, `standing`, `recall`,
    /// `project`, `exchanges` (the last exchanges) or `message`.
    pub block: String,
    pub name: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub tokens: u64,
}

/// A detour's request, as `context.detoured` records it (names and counts, no
/// text): the compile of a turn the router sent to a cheaper profile is never
/// stored, so this row is what `context.explain` reads for it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextDetoured {
    pub session_id: String,
    pub turn_id: String,
    #[serde(rename = "loop")]
    pub loop_index: u32,
    /// What it ran on: the routed profile and its model.
    pub profile: String,
    pub model: String,
    /// `trivial`, `quick`, `run_tool` or `commentary`.
    pub kind: String,
    /// The place class its turn speaks in (`private`, `shared`).
    pub class: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub est_tokens: u64,
    pub parts: Vec<DetourPart>,
    /// The tool families the request loaded.
    pub families: Vec<String>,
    /// Whether the request carried the turn's own recall; rows from before
    /// this field say nothing (a quick detour did, a trivial one did not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub recall: Option<bool>,
}

/// How a turn ran when it detoured: on `profile`, in a request of its own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextDetour {
    pub kind: String,
    pub profile: String,
    pub model: String,
    /// As [`ContextDetoured::recall`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub recall: Option<bool>,
}

/// Where a recalled node came from: its session's place by name, and for an
/// imported one, its episode's labels.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextSource {
    pub place: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub imported: Option<ImportedEpisode>,
}

/// `context.explain`'s answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextExplainResult {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    pub kind: String,
    /// The place class its turns speak in (`private`, `shared`), and the
    /// place by name.
    pub class: String,
    pub place: String,
    /// An imported session: its episode. It takes no turn, so it has no
    /// parts; it reaches a model only as recall's testimony, from a private
    /// place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub imported: Option<ImportedEpisode>,
    /// The session's turns with compiles, newest first (the newest 50).
    pub turns: Vec<ContextTurn>,
    /// The turn explained, and its last loop's compile.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub compiled: Option<ContextCompiled>,
    /// That compile's compilation, or the session's current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub compilation: Option<CompilationInfo>,
    /// The turn detoured: it ran on a cheaper profile, in a request built
    /// outside the session's compilation, so `compiled` and `compilation`
    /// are absent and `parts` are that request's. Absent for any other turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub detour: Option<ContextDetour>,
    /// What the turn ran on (the session's last target), and its window.
    pub profile: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub window: Option<u64>,
    /// The request's parts, as the next turn would build them now.
    pub parts: Vec<ContextPart>,
    /// The system blocks' digest now, and the turn's compilation's.
    pub digest_now: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest_then: Option<String>,
    /// The system blocks are the bytes that turn saw (the digests agree).
    /// Absent with no turn to compare.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unchanged: Option<bool>,
    /// Why the parts could not be built, when they could not (a profile no
    /// longer configured).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub unbuilt: Option<String>,
    /// The turn's recalls (its `recall.shadow` manifests, each admitted
    /// node's excerpt filled to a private place), and each recalled
    /// session's place.
    pub recalls: Vec<RecallManifest>,
    /// When each admitted node was written, in unix ms, by node id: a native
    /// node's creation, an imported message's own time. Absent from a
    /// daemon before `theseus why` (theseus-ighf).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[cfg_attr(test, ts(optional, type = "Record<string, number>"))]
    pub noted: BTreeMap<String, u64>,
    pub sources: BTreeMap<String, ContextSource>,
    /// Why the text was left out: a place that is not private reads sizes
    /// and digests alone. Absent when it was given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub withheld: Option<String>,
    pub ms: f64,
}
