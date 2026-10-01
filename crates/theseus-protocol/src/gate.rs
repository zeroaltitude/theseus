//! The gate's record of a tool call (§3.9, theseus-0g4): what a `tool_call`
//! node keeps, `tool.proposed` shows, and `session.history` hands to clients.
//! The types it holds are the tools' and the kernel's own: `theseus_tools`
//! re-exports `Plan`, `Resource`, and `Access`, and `theseus_kernel::gate`
//! re-exports `Proposal`, so each shape has one definition.

use std::path::PathBuf;

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::ExternalText;

/// How a call touches a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
    /// A directory a program runs in.
    Exec,
}

/// A path a call will touch, already resolved against the context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub path: PathBuf,
    pub access: Access,
}

/// What a call will do, before it does it: the gate reads this.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub resources: Vec<Resource>,
    /// For `proc.run`: the exact argv.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Vec<String>>,
    /// For a network tool: the URL it asks for (`http.fetch`'s, or the
    /// request `web.search` makes). The gate judges its host (DD5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// One line for humans ("edit src/main.rs (1 occurrence)").
    pub summary: String,
}

/// A proposed tool call as the model (or a test) states it: what a
/// confirmation binds, by its digest (`theseus_kernel::gate`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    pub tool: String,
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Policy context the confirm is bound to (binding revision, role, channel).
    #[serde(default)]
    pub policy_context: Value,
}

/// The structured notice a `notify` posture posts: to the session's channel
/// (Discord, web UI, CLI) and to the ledger (`tool.notified`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// `notify`.
    pub kind: String,
    /// The setting that chose the posture, e.g. `enforcement = notify`.
    pub setting: String,
    /// The gate's reason, e.g. `proc.run — notify (enforcement = notify)`.
    pub rule: String,
}

/// The policy's verdict on a call, as the gate records it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateDecision {
    /// `open`, `notify`, or `approve`. A record from before postures has
    /// `mode` instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posture: Option<String>,
    /// A record from before postures: its band (`auto`, `confirm`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub reason: String,
    /// The notice a `notify` posture posts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<Notice>,
    /// The floor asked: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. No setting makes it run unasked.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    /// What the secret broker gives the call, by name: `gh got GH_TOKEN`
    /// (theseus-dcy).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted: Option<String>,
    /// The call waits (or is notified) because its session read external
    /// text (theseus-9bp): what it read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalText>,
}

/// What the gate did with the call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    /// `allow` (it runs), `needs_confirm` (it waits for `by`), or `deny`
    /// (its input failed validation, and `reason` says how; it never runs).
    pub gate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
}

/// The gate's record of one tool call: its input's validation, the policy's
/// verdict, the plan it judged, and the proposal a confirmation binds.
///
/// Every record before this type was a JSON map with its keys sorted, and a
/// node keeps writing it so (`canonical`): a stored record decodes and
/// encodes again byte for byte.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GateRecord {
    pub result: GateResult,
    /// The input passed the tool's validation.
    pub validated: bool,
    /// The policy's verdict; `null` when the input failed validation.
    pub decision: Option<GateDecision>,
    /// The tool's plan; `null` when the input failed validation.
    pub plan: Option<Plan>,
    pub proposal: Proposal,
}

/// Serialize through a `serde_json::Value`, whose maps keep their keys
/// sorted (serde_json's `preserve_order` stays off): the form a stored gate
/// record has, whatever order its type declares its fields in.
pub fn canonical<T: Serialize, S: Serializer>(v: &T, s: S) -> Result<S::Ok, S::Error> {
    serde_json::to_value(v)
        .map_err(serde::ser::Error::custom)?
        .serialize(s)
}
