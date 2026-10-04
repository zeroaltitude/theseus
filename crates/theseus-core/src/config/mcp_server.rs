//! `[mcp_server]` (step 41b, M7 §2.5): Theseus's own MCP server at `/mcp`
//! on 127.0.0.1, behind one static key. Off by default. The listener is
//! `theseusd`'s (`mcp.rs`), over `theseus-mcp`'s `server` module; what an
//! MCP-opened session may do is the core's (`crate::mcp_server`).

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use crate::policy::Posture;

/// `[mcp_server]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    /// Off by default: nothing listens.
    #[serde(default)]
    pub enabled: bool,
    /// Its port on 127.0.0.1 (the reachability rule: loopback only). 0 lets
    /// the kernel pick one, for tests; health says which.
    #[serde(default = "default_port")]
    pub port: u16,
    /// The `[secrets]` entry holding the one static key, at least 16 bytes.
    /// Only the daemon reads it.
    #[serde(default = "default_key_secret")]
    pub key_secret: String,
    /// The least posture of every acting call in a session an MCP client
    /// opened: `approve` (the default) makes each one wait for the operator.
    /// A call whose posture is stricter keeps it; a read keeps its own.
    #[serde(default = "default_floor")]
    pub posture_floor: Posture,
    /// Each MCP-opened session's spend limit, in US dollars.
    #[serde(default = "default_spend_limit")]
    pub spend_limit_usd: f64,
    /// Requests a minute over every client (one key, one principal, one
    /// budget); past it, 429 with `Retry-After`.
    #[serde(default = "default_rpm")]
    pub requests_per_minute: u32,
}

fn default_port() -> u16 {
    7434
}
fn default_key_secret() -> String {
    "mcp_server_key".into()
}
fn default_floor() -> Posture {
    Posture::Approve
}
fn default_spend_limit() -> f64 {
    5.0
}
fn default_rpm() -> u32 {
    60
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_port(),
            key_secret: default_key_secret(),
            posture_floor: default_floor(),
            spend_limit_usd: default_spend_limit(),
            requests_per_minute: default_rpm(),
        }
    }
}

impl McpServerConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

impl crate::config::Config {
    /// `[mcp_server]`: a key that `[secrets]` names when it is on, a limit
    /// above zero, and at least one request a minute.
    pub(crate) fn validate_mcp_server(&self) -> Result<()> {
        let m = &self.mcp_server;
        if m.key_secret.trim().is_empty() {
            bail!("mcp_server.key_secret is empty");
        }
        if m.enabled && !self.secrets.contains_key(&m.key_secret) {
            bail!(
                "mcp_server.key_secret = {:?} names no [secrets] entry: add one holding the key \
                 (at least 16 bytes)",
                m.key_secret
            );
        }
        if !(m.spend_limit_usd.is_finite() && m.spend_limit_usd > 0.0) {
            bail!(
                "mcp_server.spend_limit_usd = {} must be above zero",
                m.spend_limit_usd
            );
        }
        if m.requests_per_minute == 0 {
            bail!("mcp_server.requests_per_minute must be at least 1");
        }
        Ok(())
    }
}
