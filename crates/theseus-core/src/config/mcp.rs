//! `[mcp.servers.<name>]` (M7 §2.1, step 36b): the MCP servers the operator
//! attaches. Only a server named here is ever started or reached; its tools
//! are offered in private places alone (the place rule), and its results are
//! outside text unless `external = false`.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// `[mcp]`: the servers, by name. Empty: no MCP tool, and nothing starts.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub servers: BTreeMap<String, McpServerConfig>,
}

impl McpConfig {
    pub fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    /// The servers the board starts: every one not set `enabled = false`.
    pub fn enabled(&self) -> impl Iterator<Item = (&String, &McpServerConfig)> {
        self.servers.iter().filter(|(_, s)| s.enabled)
    }
}

/// Where a server's process runs. L1 is filed for a follow-up: until the
/// job wrapper's L1 path can spawn a long-lived server, a server runs at L0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpSandbox {
    #[default]
    L0,
    L1,
}

/// One server: a process Theseus starts over stdio (`command`), or a remote
/// one over streamable HTTP (`url`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    /// stdio: the program and its arguments, run by their argv (no shell).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<String>,
    /// stdio: environment variables the server gets from `[secrets]`,
    /// variable = secret name, never a value. Each call of the server's
    /// tools then runs at no looser a posture than each secret's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// HTTP: the server's endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP: the `[secrets]` name of a key sent as a bearer token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_secret: Option<String>,
    /// The tools the operator calls read-class (`Read`, safe to repeat).
    /// Every other tool is `Run` and not repeatable: a server's own hints
    /// never loosen a tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<String>,
    /// Where its process runs; `l0` until servers run in L1.
    #[serde(default)]
    pub sandbox: McpSandbox,
    /// Its results are outside text, which holds the session that reads one
    /// (T1). `false` only for a server whose output is the operator's own.
    #[serde(default = "yes")]
    pub external: bool,
    /// `false` keeps it configured and unstarted.
    #[serde(default = "yes")]
    pub enabled: bool,
    /// The longest a start (and a call that waits for one) waits for the
    /// server's handshake and its list.
    #[serde(default = "default_start_timeout")]
    pub start_timeout_secs: u64,
    /// The longest one call waits: under the 120 s in-process deadline.
    #[serde(default = "default_call_timeout")]
    pub call_timeout_secs: u64,
}

fn yes() -> bool {
    true
}

fn default_start_timeout() -> u64 {
    30
}

fn default_call_timeout() -> u64 {
    110
}

/// A call's ceiling, under the core's in-process deadline (120 s).
pub const MAX_CALL_TIMEOUT_SECS: u64 = 115;

impl McpServerConfig {
    /// `stdio` or `http`.
    pub fn transport(&self) -> &'static str {
        if self.url.is_some() {
            "http"
        } else {
            "stdio"
        }
    }

    /// Every `[secrets]` name the server is given.
    pub fn secrets(&self) -> impl Iterator<Item = &String> {
        self.env.values().chain(self.auth_secret.iter())
    }
}

/// A server's name: what its tools' names carry (`mcp:<name>/<tool>`,
/// `mcp__<name>__<tool>`), so letters, digits, `_` and `-`, and no `__`,
/// which would make two servers' wire names meet.
pub fn server_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !name.contains("__")
}

impl super::Config {
    /// `[mcp.servers]`: each server's name, its one transport, its secrets
    /// (each a `[secrets]` entry), and its timeouts. Whether `read` names a
    /// real tool is known once the server lists them: health says so.
    pub(super) fn validate_mcp(&self) -> Result<()> {
        for (name, s) in &self.mcp.servers {
            let at = format!("mcp.servers.{name}");
            if !server_name_ok(name) {
                anyhow::bail!(
                    "mcp.servers.\"{name}\": a server's name is letters, digits, `_` and `-` \
                     (no `__`), at most 32 characters"
                );
            }
            match (s.command.is_empty(), &s.url) {
                (true, None) => anyhow::bail!("{at} needs `command` (stdio) or `url` (HTTP)"),
                (false, Some(_)) => {
                    anyhow::bail!("{at} has both `command` and `url`: a server has one transport")
                }
                (false, None) => {
                    if s.command[0].trim().is_empty() {
                        anyhow::bail!("{at}.command names no program");
                    }
                    if s.auth_secret.is_some() {
                        anyhow::bail!(
                            "{at}.auth_secret is for a server over HTTP; a stdio server gets \
                             its secrets in `env`"
                        );
                    }
                }
                (true, Some(url)) => {
                    if !(url.starts_with("https://") || url.starts_with("http://")) {
                        anyhow::bail!("{at}.url = {url:?} is not an http:// or https:// URL");
                    }
                    if !s.env.is_empty() {
                        anyhow::bail!(
                            "{at}.env is for a server Theseus starts; a server over HTTP gets \
                             its key from `auth_secret`"
                        );
                    }
                }
            }
            for (var, secret) in &s.env {
                if var.is_empty()
                    || !var
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    anyhow::bail!("{at}.env: {var:?} is not an environment variable's name");
                }
                if !self.secrets.contains_key(secret) {
                    anyhow::bail!(
                        "{at}.env.{var} = {secret:?} has no matching entry under [secrets] \
                         (a [secrets] name, never a value)"
                    );
                }
            }
            if let Some(secret) = &s.auth_secret {
                if !self.secrets.contains_key(secret) {
                    anyhow::bail!(
                        "{at}.auth_secret = {secret:?} has no matching entry under [secrets]"
                    );
                }
            }
            if let Some(t) = s.read.iter().find(|t| t.trim().is_empty()) {
                anyhow::bail!("{at}.read names an empty tool ({t:?})");
            }
            if s.sandbox == McpSandbox::L1 {
                anyhow::bail!(
                    "{at}.sandbox = \"l1\": MCP servers run at L0 in this build; a server in \
                     L1 is a follow-up (theseus-ext.1's report). Leave `sandbox` unset"
                );
            }
            if s.start_timeout_secs == 0 || s.start_timeout_secs > 600 {
                anyhow::bail!("{at}.start_timeout_secs must be 1 to 600");
            }
            if s.call_timeout_secs == 0 || s.call_timeout_secs > MAX_CALL_TIMEOUT_SECS {
                anyhow::bail!(
                    "{at}.call_timeout_secs must be 1 to {MAX_CALL_TIMEOUT_SECS}: a call ends \
                     under the 120-second in-process deadline"
                );
            }
        }
        Ok(())
    }
}

/// The template's commented `[mcp.servers]` examples are real: called by
/// `example_template_uncommented_still_parses`.
#[cfg(test)]
pub(crate) fn the_templates_mcp_section(cfg: &super::Config) {
    let gh = &cfg.mcp.servers["github"];
    assert_eq!(gh.transport(), "stdio");
    assert_eq!(gh.command, ["github-mcp-server", "stdio"]);
    assert_eq!(gh.env["GITHUB_PERSONAL_ACCESS_TOKEN"], "github_token");
    assert_eq!(gh.read, ["get_issue", "list_pull_requests"]);
    assert!(gh.external && gh.enabled);
    assert_eq!(gh.sandbox, McpSandbox::L0);
    assert_eq!((gh.start_timeout_secs, gh.call_timeout_secs), (30, 110));
    let docs = &cfg.mcp.servers["docs"];
    assert_eq!(docs.transport(), "http");
    assert_eq!(docs.auth_secret.as_deref(), Some("docs_mcp_token"));
}

#[cfg(test)]
mod tests {
    use crate::Config;

    fn with(mcp: &str) -> anyhow::Result<Config> {
        Config::parse(&format!(
            "[secrets]\nanthropic_api_key = \"op://v/i/f\"\ntok = \"op://v/t/f\"\n\n{mcp}\n"
        ))
        .map(|(c, _)| c)
    }

    #[test]
    fn a_stdio_and_an_http_server_load_with_their_defaults() {
        let cfg = with(
            "[mcp.servers.fake]\ncommand = [\"theseus-sim\", \"fake-mcp\"]\n\
             env = { FAKE_KEY = \"tok\" }\nread = [\"echo\"]\n\n\
             [mcp.servers.docs]\nurl = \"http://127.0.0.1:9/mcp\"\nauth_secret = \"tok\"\nexternal = false\n",
        )
        .unwrap();
        let f = &cfg.mcp.servers["fake"];
        assert_eq!(f.transport(), "stdio");
        assert!(f.external && f.enabled);
        assert_eq!((f.start_timeout_secs, f.call_timeout_secs), (30, 110));
        assert_eq!(f.secrets().collect::<Vec<_>>(), ["tok"]);
        let d = &cfg.mcp.servers["docs"];
        assert_eq!(d.transport(), "http");
        assert!(!d.external);
        assert_eq!(cfg.mcp.enabled().count(), 2);
    }

    #[test]
    fn unknown_keys_and_bad_servers_are_refused() {
        for (toml, says) in [
            ("[mcp.servers.a]\ncommand = [\"x\"]\nshell = true", "unknown field"),
            ("[mcp]\nother = 1", "unknown field"),
            ("[mcp.servers.a]\nread = [\"x\"]", "needs `command`"),
            ("[mcp.servers.a]\ncommand = [\"x\"]\nurl = \"http://h/\"", "one transport"),
            ("[mcp.servers.a]\ncommand = [\"x\"]\nenv = { K = \"nope\" }", "no matching entry"),
            ("[mcp.servers.a]\nurl = \"http://h/\"\nauth_secret = \"nope\"", "no matching entry"),
            ("[mcp.servers.a]\nurl = \"ftp://h/\"", "not an http"),
            ("[mcp.servers.\"a__b\"]\ncommand = [\"x\"]", "a server's name"),
            ("[mcp.servers.\"a.b\"]\ncommand = [\"x\"]", "a server's name"),
            ("[mcp.servers.a]\ncommand = [\"x\"]\nsandbox = \"l1\"", "follow-up"),
            ("[mcp.servers.a]\ncommand = [\"x\"]\ncall_timeout_secs = 120", "under the 120"),
            ("[mcp.servers.a]\ncommand = [\"x\"]\nauth_secret = \"tok\"", "over HTTP"),
        ] {
            let e = format!("{:#}", with(toml).unwrap_err());
            assert!(e.contains(says), "{toml}: {e}");
        }
    }
}
