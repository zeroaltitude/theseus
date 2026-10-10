//! `tool.list`'s answer: each tool as the gate and the model see it, with the
//! calls it has had since the daemon started (theseus-9dt2 moved it out of
//! `lib.rs`, with the count of its calls whose input was not JSON).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Tightening;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolInfo {
    /// Canonical name (`fs.read`).
    pub name: String,
    /// Name on the wire (`fs_read`).
    pub wire_name: String,
    pub family: String,
    pub description: String,
    /// `read`, `write`, or `run`.
    pub class: String,
    /// `inproc` or `job`.
    pub backend: String,
    /// The posture the gate applies now: `open`, `notify`, or `approve`.
    pub policy: String,
    /// What chose it: a config setting (`enforcement = notify`), or a
    /// tightening (`tightened by discord:zeroaltitude`).
    #[serde(default)]
    pub setting: String,
    /// What the config alone says (theseus-sgh). It differs from `policy`
    /// only while a tightening is stricter.
    #[serde(default)]
    pub config_posture: String,
    #[serde(default)]
    pub config_setting: String,
    /// Someone pressed "should have asked" for this tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tightened: Option<Tightening>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input_schema: Value,
    /// Calls since the daemon started.
    #[serde(default)]
    pub calls: u64,
    /// Calls whose input did not parse as JSON, since the daemon started
    /// (theseus-9dt2): the cost of a tool whose input streams as it is written.
    #[serde(default)]
    pub invalid_json: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolListResult {
    pub tools: Vec<ToolInfo>,
    /// Workspace roots tools may touch.
    pub roots: Vec<String>,
    /// `proc.run` calls over all tool calls (spec §3.23 shell-fallback ratio).
    pub shell_fallback_ratio: f64,
    /// Calls since the daemon started.
    pub calls_total: u64,
}
