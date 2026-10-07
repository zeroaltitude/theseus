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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Read,
    Write,
    /// A directory a program runs in.
    Exec,
}

/// A path a call will touch, already resolved against the context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Resource {
    pub path: PathBuf,
    pub access: Access,
}

/// What a tool does: read, write, or run a program. The tools' type
/// (`theseus_tools` re-exports it), here since a plan carries a call's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ToolClass {
    /// Reads files or repository state; changes nothing.
    Read,
    /// Changes files.
    Write,
    /// Runs a program.
    Run,
}

impl ToolClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ToolClass::Read => "read",
            ToolClass::Write => "write",
            ToolClass::Run => "run",
        }
    }
}

/// An AWS call's plan (AWS design §3.9): the account and region it goes to,
/// the operation, and what the catalog says of it. The gate's
/// `[policy.aws]` reads it, and the surfaces show it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct AwsPlan {
    pub account: String,
    pub region: String,
    /// The service as the catalog names it (`cloudformation`).
    pub service: String,
    /// `DescribeStacks`.
    pub operation: String,
    /// Charged per request, or it starts something metered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cost_bearing: bool,
    /// The names, ids, and ARNs its input gives (an S3 bucket and prefix).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    /// The guardrail the call hits, as the floor's confirm names it (AWS
    /// design §3.6): the gate asks at every posture. The gate's input, held
    /// in memory: its record keeps the verdict (`floor`, and this in its
    /// reason), so a node's layout is unchanged.
    #[serde(skip)]
    pub guardrail: Option<String>,
    /// It deletes a stateful resource or a stack (§3.9's approve list): the
    /// gate asks. Held in memory, as `guardrail`.
    #[serde(skip)]
    pub destructive: bool,
    /// An AWS session mint, in the words the approval names it with (its role
    /// or target, and that the keys stay held): the gate asks at every
    /// posture (theseus-a3s3). Held in memory, as `guardrail`.
    #[serde(skip)]
    pub session_mint: Option<String>,
}

/// What a call will do, before it does it: the gate reads this.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Plan {
    pub resources: Vec<Resource>,
    /// For `proc.run`: the exact argv.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub argv: Option<Vec<String>>,
    /// For a `proc.run` of `steps` (theseus-7gir.3): each step's exact argv,
    /// in the order they run. Its summary names each step's directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub steps: Option<Vec<Vec<String>>>,
    /// For a network tool: the URL it asks for (`http.fetch`'s, or the
    /// request `web.search` makes). The gate judges its host (DD5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub url: Option<String>,
    /// One line for humans ("edit src/main.rs (1 occurrence)").
    pub summary: String,
    /// This call's class, when it is the call's and not the tool's (an
    /// `aws.call` is a read or a write by its operation): the gate, the
    /// external-text hold, and the reads that run together read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub class: Option<ToolClass>,
    /// For an AWS call (AWS design §3.9): its account, region, and operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub aws: Option<AwsPlan>,
    /// A change to what only the operator decides (a task's objective or
    /// acceptance, or abandoning it: 39a's layer 1), as the card names it:
    /// the gate asks at every posture, as the floor does. Held in memory, as
    /// `AwsPlan::guardrail`: the record keeps the verdict in its reason.
    #[serde(skip)]
    pub authority: Option<String>,
}

/// A proposed tool call as the model (or a test) states it: what a
/// confirmation binds, by its digest (`theseus_kernel::gate`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Proposal {
    pub tool: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resource: Option<String>,
    /// Policy context the confirm is bound to (binding revision, role, channel).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub policy_context: Value,
}

/// The structured notice a `notify` posture posts: to the session's channel
/// (Discord, web UI, CLI) and to the ledger (`tool.notified`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct GateDecision {
    /// `open`, `notify`, or `approve`. A record from before postures has
    /// `mode` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub posture: Option<String>,
    /// A record from before postures: its band (`auto`, `confirm`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub mode: Option<String>,
    pub reason: String,
    /// The notice a `notify` posture posts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub notify: Option<Notice>,
    /// The floor asked: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. No setting makes it run unasked.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub floor: bool,
    /// What the secret broker gives the call, by name: `gh got GH_TOKEN`
    /// (theseus-dcy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub granted: Option<String>,
    /// The call waits (or is notified) because its session read external
    /// text (theseus-9bp): what it read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub external: Option<ExternalText>,
    /// A job's class (M4 17b): `l1` when it runs in the sandbox, and then the
    /// proposal a confirm binds names it too. Absent: L0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub class: Option<String>,
}

/// What the gate did with the call.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct GateResult {
    /// `allow` (it runs), `needs_confirm` (it waits for `by`), or `deny`
    /// (its input failed validation, and `reason` says how; it never runs).
    pub gate: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by: Option<String>,
}

/// The gate's record of one tool call: its input's validation, the policy's
/// verdict, the plan it judged, and the proposal a confirmation binds.
///
/// Every record before this type was a JSON map with its keys sorted, and a
/// node keeps writing it so (`canonical`): a stored record decodes and
/// encodes again byte for byte.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
