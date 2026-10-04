//! Health's `mcp_server` block (step 41b, M7 §2.5): Theseus's own MCP server
//! at `/mcp` on loopback, as the daemon runs it. `HealthResult` is in lib.rs.

use serde::{Deserialize, Serialize};

/// The MCP server (`[mcp_server]`): whether it listens, where, and what it
/// has served and refused since the daemon's image started. Absent while
/// `[mcp_server]` is off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpServerHealth {
    /// `starting` (its key's secret is resolving), `listening`, `stopped`,
    /// or `failed`.
    pub state: String,
    /// The port it listens on, once it does.
    pub port: u16,
    /// MCP sessions open now, and their clients' names, sorted, once each.
    pub sessions: u64,
    #[serde(default)]
    pub clients: Vec<String>,
    /// Conversations opened through it since the start, and the last client
    /// to open one.
    pub opened: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_client: Option<String>,
    /// Tool calls answered, and those whose answer was an error.
    pub calls: u64,
    pub errors: u64,
    /// Requests refused, by why.
    #[serde(default)]
    pub refused: McpServerRefusals,
    /// Why it is not listening, when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<String>,
}

/// The MCP server's refusals, by why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpServerRefusals {
    /// A `Host` that does not name the listener (DNS rebinding).
    pub host: u64,
    /// An `Origin` that is not a loopback page.
    pub origin: u64,
    /// No key, or a wrong one.
    pub key: u64,
    /// Past `requests_per_minute`.
    pub rate: u64,
    /// A connection whose client socket another uid owns.
    pub peer: u64,
}

impl McpServerRefusals {
    pub fn total(&self) -> u64 {
        self.host + self.origin + self.key + self.rate + self.peer
    }
}
