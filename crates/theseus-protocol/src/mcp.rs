//! The MCP client's surfaces (M7 §2.1, step 36b): health's `mcp[]`, and
//! `mcp.list` and `mcp.restart` (`theseus mcp`, `theseus mcp restart`), and
//! the prompts of 36c: `mcp.prompt.list` and the `prompt` that
//! `turn.submit` takes. Each name carries `Mcp`, since the web apps' types
//! share one namespace.

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
    /// Who attached it by asking, and through what: absent for a server the
    /// owner wrote in the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attached_by: Option<String>,
    /// When it was attached by asking, in ms since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attached_at_ms: Option<u64>,
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
    /// Every server's prompts (36c), the stored lists included.
    #[serde(default)]
    pub prompts: Vec<McpPromptInfo>,
}

/// One argument a prompt takes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// One prompt a server lists (36c), as the operator picks it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpPromptInfo {
    pub server: String,
    /// The prompt's own name, at its server.
    pub prompt: String,
    /// `<server>/<prompt>`: what `/prompt` and `theseus prompt` name.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<McpPromptArgument>,
    /// The digest of its definition: its name, description, and arguments.
    pub digest: String,
    /// Whether the list is the stored one, the server not having listed
    /// prompts since the start.
    #[serde(default)]
    pub stored: bool,
}

/// `mcp.prompt.list`: the prompts of one server, or of every server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpPromptListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub server: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpPromptListResult {
    pub prompts: Vec<McpPromptInfo>,
}

/// The prompt a `turn.submit` runs as its input (36c): the core asks the
/// server for it, and its messages become the turn's input nodes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpPromptRef {
    pub server: String,
    pub name: String,
    #[serde(default)]
    pub arguments: std::collections::BTreeMap<String, String>,
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

/// `mcp.login.begin` (issue 55): start the sign-in of a server that signs in
/// (`oauth`). Admitted from the CLI's surface alone. `manual` skips the wait
/// for the redirect and goes straight to the paste.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpLoginBeginParams {
    pub name: String,
    #[serde(default)]
    pub manual: bool,
}

/// The address the owner opens, which holds no secret (a client id, a PKCE
/// challenge, a state), the host it signs in at, and how the redirect comes.
/// Its `Debug` prints none of what a server chose.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpLoginBeginResult {
    /// Names this sign-in for `mcp.login.finish`.
    pub login_id: String,
    pub name: String,
    /// The host of the address: where the owner signs in.
    pub host: String,
    pub url: String,
    /// The loopback redirect the daemon listens on; empty with `manual`.
    pub redirect: String,
    /// How long the sign-in stays open, in seconds, and when the CLI offers
    /// the paste.
    pub expires_secs: u64,
    pub paste_after_secs: u64,
    /// Every place of the sign-in's endpoints that is not its issuer's own:
    /// the CLI names each and asks the owner to agree before it shows the
    /// address, and the daemon requires that agreement exactly when this is
    /// not empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[cfg_attr(test, ts(as = "Option<Vec<String>>", optional))]
    pub elsewhere: Vec<String>,
}

impl std::fmt::Debug for McpLoginBeginResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpLoginBeginResult")
            .field("login_id", &self.login_id)
            .field("name", &self.name)
            .field("expires_secs", &self.expires_secs)
            .field("paste_after_secs", &self.paste_after_secs)
            .finish_non_exhaustive()
    }
}

/// `mcp.login.finish`: wait for the redirect (`pasted` empty), or take the
/// address the owner's browser was sent to, pasted.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpLoginFinishParams {
    pub login_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pasted: Option<String>,
    /// With no paste: wait this long for the redirect (seconds), then answer
    /// `waiting`.
    #[serde(default)]
    pub wait_secs: u64,
    /// The owner agreed to the other hosts `elsewhere` named.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub agreed: bool,
}

/// The pasted address carries the code: never printed.
impl std::fmt::Debug for McpLoginFinishParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpLoginFinishParams")
            .field("login_id", &self.login_id)
            .field("pasted", &self.pasted.as_ref().map(|_| "(hidden)"))
            .field("wait_secs", &self.wait_secs)
            .field("agreed", &self.agreed)
            .finish()
    }
}

/// What the sign-in came to. No token, code or secret is in it; its `Debug`
/// prints no host a server chose.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpLoginFinishResult {
    /// `done`, or `waiting` (no redirect yet: paste the address).
    pub state: String,
    pub name: String,
    pub host: String,
    /// Whether the server came up with the sign-in, and the tools it lists.
    pub connected: bool,
    pub tools: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

impl std::fmt::Debug for McpLoginFinishResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpLoginFinishResult")
            .field("state", &self.state)
            .field("name", &self.name)
            .field("connected", &self.connected)
            .field("tools", &self.tools)
            .finish_non_exhaustive()
    }
}

/// `mcp.logout`: forget a server's sign-in, revoked at its server when it
/// offers that. Admitted from the CLI's surface alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpLogoutParams {
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct McpLogoutResult {
    pub name: String,
    /// Whether a sign-in was kept, and is now forgotten.
    pub forgot: bool,
    pub revoked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Its kept file could not be removed: it would be used again when the
    /// daemon next starts.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub kept_remains: bool,
}
