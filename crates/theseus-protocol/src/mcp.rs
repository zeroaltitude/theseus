//! The MCP client's surfaces (M7 §2.1, step 36b): health's `mcp[]`, and
//! `mcp.list` and `mcp.restart` (`theseus mcp`, `theseus mcp restart`). Each
//! name carries `Mcp`, since the web apps' types share one namespace.

use serde::{Deserialize, Serialize};

/// One configured MCP server, as the board has it now: health's `mcp[]`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpServerStatus {
    pub name: String,
    /// `stdio` or `http`.
    pub transport: String,
    /// `stopped` (not started yet), `starting`, `ready`, `exited`,
    /// `restarting`, `failed`, or `disabled`.
    pub state: String,
    /// A stdio server's process (and process group).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pid: Option<u32>,
    /// The tools offered now: the live list, or the stored one until the
    /// server lists them.
    pub tools: u64,
    pub prompts: u64,
    /// Whether the tools offered are the stored list (`mcp.tools.<server>`),
    /// the server not having listed them since the start.
    #[serde(default)]
    pub stored: bool,
    /// Calls and failed calls since the daemon started.
    pub calls: u64,
    pub errors: u64,
    /// Crashes since the last start that stayed up, or the last restart.
    #[serde(default)]
    pub crashes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_error: Option<String>,
    /// When it last became ready, in ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub started_at_ms: Option<u64>,
    /// The negotiated protocol revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub protocol: Option<String>,
    /// The digest of the tool list offered.
    #[serde(default)]
    pub digest: String,
    /// `read` entries the server does not list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unknown_read: Vec<String>,
}

/// One MCP tool, as the model is offered it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpToolInfo {
    pub server: String,
    /// The tool's own name, at its server.
    pub tool: String,
    /// `mcp:<server>/<tool>`: what the gate and `[policy.mcp]` name.
    pub name: String,
    /// `mcp__<server>__<tool>`: what the model calls.
    pub wire_name: String,
    /// `read` or `run`.
    pub class: String,
    pub posture: String,
    /// The setting the posture comes from.
    pub setting: String,
    /// The server's own hints (`readOnlyHint`, `destructiveHint`,
    /// `idempotentHint`, `openWorldHint`), shown, never loosening.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hints: Vec<String>,
    /// The description the model reads, whole (capped at 2,000 characters).
    pub description: String,
    pub calls: u64,
}

/// `mcp.list`: every configured server and the tools offered.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpListResult {
    pub servers: Vec<McpServerStatus>,
    pub tools: Vec<McpToolInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpRestartParams {
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpRestartResult {
    pub name: String,
    /// The state it was in when asked.
    pub was: String,
}
