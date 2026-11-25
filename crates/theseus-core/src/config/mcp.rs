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

/// Where a server's process runs: L0, as the daemon's own child, or L1
/// (M7 43a), below the sandbox's init in the view a job gets, through the
/// daemon's `mcp-sandbox` role (`theseus_kernel::mcp_l1`). L0 stays the
/// default for a configured server; an extension always runs in L1.
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
    /// HTTP: the server signs in in a browser (OAuth, issue 55): the owner
    /// runs `theseus mcp login <name>` once, and the daemon keeps and renews
    /// what the sign-in gave. Never with `auth_secret`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOauth>,
    /// The tools the operator calls read-class (`Read`, safe to repeat).
    /// Every other tool is `Run` and not repeatable: a server's own hints
    /// never loosen a tool.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<String>,
    /// Where its process runs: `l0` (the default), or `l1`.
    #[serde(default)]
    pub sandbox: McpSandbox,
    /// In L1: the hosts it may reach through its egress proxy, as `[sandbox]
    /// egress` lists them for a job. Empty: no network at all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub egress: Vec<String>,
    /// An extension's frozen copy (43a), never the config's: its working
    /// directory, bound read-only into its view. `None` for a configured
    /// server, which starts in the workspace's first root.
    #[serde(skip)]
    pub frozen: Option<std::path::PathBuf>,
    /// Its results are marked as outside text (`external` on the node).
    /// `false` only for a server whose output is the operator's own.
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

/// `oauth = {}`, or `oauth = { client_id = "…", scopes = ["…"] }`: a server
/// that signs in. Both keys are for a server that offers no registration
/// (`client_id`) or asks for scopes it does not name in its 401 (`scopes`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpOauth {
    /// The client id the server gave the owner, for a server that offers no
    /// registration. Not a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The scopes to ask for; none: the scope the server's 401 names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
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
            if name.starts_with(crate::extend::SERVER_PREFIX) {
                anyhow::bail!(
                    "mcp.servers.\"{name}\": the `{}` prefix is an extension's (M7 43b), \
                     loaded by its ack: name a configured server otherwise",
                    crate::extend::SERVER_PREFIX
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
                    if s.oauth.is_some() {
                        anyhow::bail!(
                            "{at}.oauth is for a server over HTTP that signs in in a browser"
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
                if var.is_empty() || !var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    anyhow::bail!("{at}.env: {var:?} is not an environment variable's name");
                }
                if !self.secrets.contains_key(secret) {
                    anyhow::bail!(
                        "{at}.env.{var} = {secret:?} has no matching entry under [secrets] \
                         (a [secrets] name, never a value)"
                    );
                }
            }
            validate_oauth(&at, s)?;
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
            if s.sandbox == McpSandbox::L1 && s.url.is_some() {
                anyhow::bail!(
                    "{at}.sandbox = \"l1\" is for a server Theseus starts; a server over HTTP \
                     runs elsewhere"
                );
            }
            if !s.egress.is_empty() && s.sandbox != McpSandbox::L1 {
                anyhow::bail!(
                    "{at}.egress is an L1 server's list: set sandbox = \"l1\" too (at L0 a \
                     server reaches what the daemon reaches)"
                );
            }
            if let Err(e) = crate::egress::check(&s.egress) {
                anyhow::bail!("{at}.egress has {e}");
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

/// A server's `oauth` (issue 55): never with a fixed key, and a client id or
/// scopes that are not empty.
fn validate_oauth(at: &str, s: &McpServerConfig) -> Result<()> {
    let Some(o) = &s.oauth else { return Ok(()) };
    if s.auth_secret.is_some() {
        anyhow::bail!(
            "{at} has both `auth_secret` and `oauth`: a server either is sent a fixed key or \
             signs in, not both"
        );
    }
    let https = s
        .url
        .as_deref()
        .and_then(|u| reqwest::Url::parse(u).ok())
        .is_some_and(|u| crate::mcp::oauth::signs_in_at(&u));
    if !https {
        anyhow::bail!(
            "{at} signs in (`oauth`), and a server that signs in must be at an https address"
        );
    }
    if o.client_id.as_ref().is_some_and(|c| c.trim().is_empty()) {
        anyhow::bail!("{at}.oauth.client_id is empty");
    }
    if o.scopes
        .iter()
        .any(|x| x.trim().is_empty() || x.contains(' '))
    {
        anyhow::bail!("{at}.oauth.scopes holds an empty scope or one with a space");
    }
    Ok(())
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
    assert!(gh.egress.is_empty());
    let wc = &cfg.mcp.servers["wordcount"];
    assert_eq!(wc.sandbox, McpSandbox::L1);
    assert_eq!(wc.egress, ["api.wordlist.invalid:443"]);
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
        assert_eq!(f.sandbox, super::McpSandbox::L0, "l0 stays the default");
    }

    /// A server that signs in (issue 55): `oauth`, with a client id and
    /// scopes or neither; a fixed key stays as it is.
    #[test]
    fn a_server_that_signs_in_loads_and_a_fixed_key_is_unchanged() {
        let cfg = with(
            "[mcp.servers.docs]\nurl = \"https://docs.example.invalid/mcp\"\noauth = {}\n\n\
             [mcp.servers.wiki]\nurl = \"https://wiki.example.invalid/mcp\"\n\
             oauth = { client_id = \"theseus-1\", scopes = [\"read\", \"write\"] }\n\n\
             [mcp.servers.keyed]\nurl = \"https://k.example.invalid/mcp\"\nauth_secret = \"tok\"\n",
        )
        .unwrap();
        assert_eq!(
            cfg.mcp.servers["docs"].oauth,
            Some(super::McpOauth::default())
        );
        let w = cfg.mcp.servers["wiki"].oauth.clone().unwrap();
        assert_eq!(w.client_id.as_deref(), Some("theseus-1"));
        assert_eq!(w.scopes, ["read", "write"]);
        let k = &cfg.mcp.servers["keyed"];
        assert_eq!(
            (k.oauth.as_ref(), k.auth_secret.as_deref()),
            (None, Some("tok"))
        );
        assert_eq!(cfg.mcp.servers["docs"].secrets().count(), 0);
    }

    /// A stdio server in L1 loads, with its egress list (M7 43a); the
    /// frozen copy is never read from a config.
    #[test]
    fn an_l1_server_loads_with_its_egress_list() {
        let cfg = with(
            "[mcp.servers.wc]\ncommand = [\"wc-mcp\"]\nsandbox = \"l1\"\n\
             egress = [\"api.wordlist.invalid:443\"]\n",
        )
        .unwrap();
        let wc = &cfg.mcp.servers["wc"];
        assert_eq!(wc.sandbox, super::McpSandbox::L1);
        assert_eq!(wc.egress, ["api.wordlist.invalid:443"]);
        assert_eq!(wc.frozen, None);
        let e = format!(
            "{:#}",
            with("[mcp.servers.wc]\ncommand = [\"x\"]\nfrozen = \"/tmp\"").unwrap_err()
        );
        assert!(e.contains("unknown field"), "{e}");
    }

    #[test]
    fn unknown_keys_and_bad_servers_are_refused() {
        for (toml, says) in [
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\nshell = true",
                "unknown field",
            ),
            ("[mcp]\nother = 1", "unknown field"),
            (
                "[mcp.servers.\"ext-wordcount\"]\ncommand = [\"x\"]",
                "an extension's",
            ),
            ("[mcp.servers.a]\nread = [\"x\"]", "needs `command`"),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\nurl = \"http://h/\"",
                "one transport",
            ),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\nenv = { K = \"nope\" }",
                "no matching entry",
            ),
            (
                "[mcp.servers.a]\nurl = \"http://h/\"\nauth_secret = \"nope\"",
                "no matching entry",
            ),
            ("[mcp.servers.a]\nurl = \"ftp://h/\"", "not an http"),
            (
                "[mcp.servers.\"a__b\"]\ncommand = [\"x\"]",
                "a server's name",
            ),
            (
                "[mcp.servers.\"a.b\"]\ncommand = [\"x\"]",
                "a server's name",
            ),
            (
                "[mcp.servers.a]\nurl = \"http://h/\"\nsandbox = \"l1\"",
                "over HTTP runs elsewhere",
            ),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\negress = [\"h.invalid\"]",
                "an L1 server's list",
            ),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\nsandbox = \"l1\"\negress = [\"\"]",
                "egress has",
            ),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\ncall_timeout_secs = 120",
                "under the 120",
            ),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\nauth_secret = \"tok\"",
                "over HTTP",
            ),
            (
                "[mcp.servers.a]\nurl = \"https://h/\"\nauth_secret = \"tok\"\noauth = {}",
                "not both",
            ),
            (
                "[mcp.servers.a]\ncommand = [\"x\"]\noauth = {}",
                "signs in in a browser",
            ),
            (
                "[mcp.servers.a]\nurl = \"https://h/\"\noauth = { secret = \"x\" }",
                "unknown field",
            ),
            (
                "[mcp.servers.a]\nurl = \"https://h/\"\noauth = { client_id = \" \" }",
                "client_id is empty",
            ),
            (
                "[mcp.servers.a]\nurl = \"https://h/\"\noauth = { scopes = [\"a b\"] }",
                "with a space",
            ),
            (
                "[mcp.servers.a]\nurl = \"http://h.example.invalid/mcp\"\noauth = {}",
                "must be at an https address",
            ),
        ] {
            let e = format!("{:#}", with(toml).unwrap_err());
            assert!(e.contains(says), "{toml}: {e}");
        }
    }
}
