//! `theseus mcp login <name>` and `logout` (issue 55): the owner signs in to a
//! server that signs in (`oauth`), from the CLI on this machine
//! (`rpc/mcp_login.rs` admits it from there alone), in a browser.
//!
//! The daemon does every step that touches a value: it asks the server with
//! no token and reads its 401's challenge, discovers its authorization
//! server (`theseus_mcp::oauth::discover`), registers a client of its own
//! when the server offers that (else the settings' `oauth.client_id`), draws
//! the PKCE verifier and the `state`, and listens once on `127.0.0.1` for the
//! redirect. The CLI is given the address to open, which holds no secret, and
//! the host it signs in at. The code comes from the listener, or from the
//! address the owner pastes (over the socket, in a param); it is redeemed
//! here, the tokens are kept (`oauth.rs`), and the server is started again.
//! On WSL the browser cannot reach the loopback, so the paste comes at once.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_mcp::oauth::discover::{self, Discovered};
use theseus_mcp::oauth::exchange;
use theseus_mcp::oauth::register::{self, ClientId};
use theseus_mcp::oauth::request::{self, Code, Loopback, Pkce, RedirectError};
use theseus_protocol::mcp::{
    McpLoginBeginResult, McpLoginFinishParams, McpLoginFinishResult, McpLogoutResult,
};
use tokio::sync::oneshot;

use super::oauth::{Kept, Signed};
use super::{McpBoard, Server, State};

/// How long a sign-in stays open: its listener's bound.
const OPEN_FOR: Duration = request::LISTEN_FOR;
/// How long the CLI waits for the redirect before it offers the paste.
const PASTE_AFTER: Duration = Duration::from_secs(90);

/// A sign-in waiting for its redirect.
pub(crate) struct Pending {
    id: String,
    server: String,
    pkce: Pkce,
    redirect: String,
    client: ClientId,
    found: Discovered,
    host: String,
    rx: Option<oneshot::Receiver<Result<Code, RedirectError>>>,
    began: Instant,
}

/// The `initialize` a server is asked with no token, to read its challenge.
fn probe_body() -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0", "id": 0, "method": "initialize",
        "params": {
            "protocolVersion": theseus_mcp::LATEST_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "theseus", "version": env!("CARGO_PKG_VERSION")},
        },
    })
}

impl McpBoard {
    /// The server `name`, which must sign in; its address, and its sign-in.
    fn signing(&self, name: &str) -> Result<(Arc<Server>, reqwest::Url, Arc<Signed>), String> {
        let s = self
            .servers
            .get(name)
            .cloned()
            .or(self.loaded_server(name))
            .ok_or_else(|| format!("no MCP server named {name} is attached"))?;
        if s.cfg.oauth.is_none() {
            return Err(format!(
                "the MCP server {name} does not sign in: its settings have no `oauth`. A server \
                 that wants a sign-in is attached again with one (`oauth`), then signed in"
            ));
        }
        let url = s
            .cfg
            .url
            .as_deref()
            .and_then(|u| reqwest::Url::parse(u).ok())
            .ok_or_else(|| format!("the MCP server {name} has no address"))?;
        let signed = self.signed(&s).ok_or("the server's sign-in is not known")?;
        Ok((s, url, signed))
    }

    /// Start a sign-in of `name`: discovery, a client, the address, and the
    /// listener behind it. `wsl`: the paste at once, as with `manual`.
    pub async fn login_begin(
        &self,
        name: &str,
        manual: bool,
        wsl: bool,
    ) -> Result<McpLoginBeginResult, String> {
        let manual = manual || wsl;
        let (s, url, signed) = self.signing(name)?;
        let http = theseus_mcp::oauth::http(theseus_mcp::oauth::is_loopback(&url));
        let asked = http
            .post(url.clone())
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .json(&probe_body())
            .send()
            .await
            .map_err(|_| format!("the MCP server {name} could not be reached"))?;
        if asked.status().is_success() {
            return Err(format!(
                "the MCP server {name} answered without a sign-in: it does not need one"
            ));
        }
        let challenge = asked
            .headers()
            .get(reqwest::header::WWW_AUTHENTICATE)
            .and_then(|v| v.to_str().ok())
            .and_then(discover::challenge);
        drop(asked);
        let found = discover::discover(&http, &url, challenge.as_ref())
            .await
            .map_err(|e| e.to_string())?;
        let loopback = Loopback::bind()
            .await
            .map_err(|_| "the loopback listener could not start".to_string())?;
        let redirect = loopback.redirect();
        let settings = s.cfg.oauth.clone().unwrap_or_default();
        let client = match (&settings.client_id, &found.server.registration_endpoint) {
            (Some(id), _) => ClientId::named(id),
            (None, Some(ep)) => register::register(&http, ep, &redirect)
                .await
                .map_err(|e| e.to_string())?,
            (None, None) => {
                return Err(format!(
                    "the authorization server of {name} offers no registration, and the \
                     server's settings name no `oauth.client_id`: the owner asks the server's \
                     maker for a client id, and it is added to the settings"
                ))
            }
        };
        if let Some(secret) = &client.secret {
            signed.hold_value("client", secret);
        }
        let pkce = Pkce::new();
        signed.hold_value("verifier", pkce.verifier());
        let scope = match settings.scopes.is_empty() {
            true => found.scope.clone(),
            false => Some(settings.scopes.join(" ")),
        };
        let at = request::authorize_url(
            &found.server,
            &client.id,
            &redirect,
            &pkce,
            &found.resource,
            scope.as_deref(),
        );
        let host = found
            .server
            .authorization_endpoint
            .host_str()
            .unwrap_or("?")
            .to_string();
        let (tx, rx) = oneshot::channel();
        let (state, issuer) = (pkce.state.clone(), found.server.issuer.clone());
        tokio::spawn(async move {
            let _ = tx.send(loopback.wait(&state, &issuer, OPEN_FOR).await);
        });
        let id = uuid::Uuid::now_v7().simple().to_string();
        *self.login.lock().await = Some(Pending {
            id: id.clone(),
            server: name.to_string(),
            pkce,
            redirect: redirect.clone(),
            client,
            found,
            host: host.clone(),
            rx: Some(rx),
            began: Instant::now(),
        });
        tracing::info!(server = name, host = %host, "an MCP sign-in began");
        Ok(McpLoginBeginResult {
            login_id: id,
            name: name.to_string(),
            host,
            url: at,
            redirect: if manual { String::new() } else { redirect },
            expires_secs: OPEN_FOR.as_secs(),
            paste_after_secs: if manual { 0 } else { PASTE_AFTER.as_secs() },
        })
    }

    /// Wait for the redirect, or take the owner's paste, and finish: the code
    /// redeemed, the tokens kept, and the server started again with them.
    pub async fn login_finish(
        self: &Arc<Self>,
        p: &McpLoginFinishParams,
    ) -> Result<McpLoginFinishResult, String> {
        let mut slot = self.login.lock().await;
        let Some(pending) = slot
            .as_mut()
            .filter(|c| c.id == p.login_id && c.began.elapsed() < OPEN_FOR)
        else {
            *slot = None;
            return Err("no sign-in is open: run `theseus mcp login` again".into());
        };
        let (state, issuer) = (
            pending.pkce.state.clone(),
            pending.found.server.issuer.clone(),
        );
        let code = match p.pasted.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(pasted) => {
                request::parse_redirect(pasted, &state, &issuer).map_err(|e| e.to_string())?
            }
            None => {
                let Some(rx) = pending.rx.as_mut() else {
                    return Err("the redirect was taken: run `theseus mcp login` again".into());
                };
                match tokio::time::timeout(Duration::from_secs(p.wait_secs), rx).await {
                    Err(_) => return Ok(waiting(&pending.server, &pending.host)),
                    Ok(Err(_)) => {
                        return Err("the listener stopped: run `theseus mcp login` again".into())
                    }
                    Ok(Ok(Err(e))) => {
                        *slot = None;
                        return Err(e.to_string());
                    }
                    Ok(Ok(Ok(code))) => code,
                }
            }
        };
        // The code is one use: whatever follows, this sign-in is over.
        let pending = slot.take().expect("a pending sign-in");
        drop(slot);
        let (s, url, signed) = self.signing(&pending.server)?;
        signed.hold_value("code", code.expose());
        let http = theseus_mcp::oauth::http(theseus_mcp::oauth::is_loopback(&url));
        let tokens = exchange::redeem(
            &http,
            &pending.found.server.token_endpoint,
            &pending.client,
            &code,
            pending.pkce.verifier(),
            &pending.redirect,
            &pending.found.resource,
        )
        .await
        .map_err(|e| format!("the sign-in was not completed: {e}"))?;
        let now = theseus_protocol::now_unix_ms();
        let mut kept = Kept {
            url: url.to_string(),
            resource: pending.found.resource.clone(),
            issuer: pending.found.server.issuer.clone(),
            token_endpoint: pending.found.server.token_endpoint.to_string(),
            revocation_endpoint: pending
                .found
                .server
                .revocation_endpoint
                .as_ref()
                .map(ToString::to_string),
            client_id: pending.client.id.clone(),
            client_secret: pending.client.secret.as_ref().map(|s| s.to_string()),
            client_basic: pending.client.basic,
            access_token: String::new(),
            refresh_token: None,
            expires_at_ms: None,
            signed_in_at_ms: now,
        };
        kept.renewed(&tokens, now);
        let mut note = String::new();
        if let Err(e) = signed.put(kept).await {
            note = format!(
                "the sign-in could not be written ({e}): it works until the daemon restarts"
            );
        }
        tracing::info!(server = %s.name, "an MCP server was signed in");
        let changes = s.changed.subscribe();
        self.restart(&s.name);
        let (connected, tools) = came_up(&s, changes).await;
        Ok(McpLoginFinishResult {
            state: "done".into(),
            name: s.name.clone(),
            host: pending.host,
            connected,
            tools,
            note,
        })
    }

    /// Forget `name`'s sign-in: revoked at its server when it offers that,
    /// the file removed, and the server started again (now `needs sign-in`).
    pub async fn logout(self: &Arc<Self>, name: &str) -> Result<McpLogoutResult, String> {
        let (s, url, signed) = self.signing(name)?;
        let (kept, removed) = signed.forget().await;
        let mut notes = Vec::new();
        let mut revoked = false;
        if let Some(k) = &kept {
            match k.revocation_endpoint.as_deref().map(reqwest::Url::parse) {
                Some(Ok(ep)) => {
                    let http = theseus_mcp::oauth::http(theseus_mcp::oauth::is_loopback(&url));
                    let client = k.client();
                    let mut all = true;
                    if let Some(r) = &k.refresh_token {
                        all &= exchange::revoke(&http, &ep, &client, r, "refresh_token")
                            .await
                            .map_err(|e| {
                                notes.push(format!("the refresh token was not revoked ({e})"))
                            })
                            .is_ok();
                    }
                    all &= exchange::revoke(&http, &ep, &client, &k.access_token, "access_token")
                        .await
                        .map_err(|e| notes.push(format!("the access token was not revoked ({e})")))
                        .is_ok();
                    revoked = all;
                }
                _ => notes
                    .push("its server offers no revoke: the sign-in ends when it runs out".into()),
            }
        }
        if let Err(e) = removed {
            notes.push(format!("the kept sign-in was not removed ({e})"));
        }
        // What was kept stays held on the secret board, so the scrubber still
        // knows each value.
        tracing::info!(server = name, revoked, "an MCP server was signed out");
        self.restart(&s.name);
        Ok(McpLogoutResult {
            name: name.to_string(),
            forgot: kept.is_some(),
            revoked,
            note: notes.join("; "),
        })
    }
}

/// Whether `s` came up after its restart, within its start's timeout, and how
/// many tools it lists. `rx` was subscribed before the restart, so its first
/// change is the restart's own move.
async fn came_up(s: &Server, mut rx: tokio::sync::watch::Receiver<u64>) -> (bool, u64) {
    let wait = Duration::from_secs(s.cfg.start_timeout_secs);
    let deadline = tokio::time::Instant::now() + wait;
    let _ = tokio::time::timeout_at(deadline, rx.changed()).await;
    loop {
        match s.state() {
            State::Ready if s.live().client.is_some() => {
                return (true, s.live().tools.len() as u64)
            }
            State::Failed | State::NeedsSignIn | State::Restarting => return (false, 0),
            _ => {}
        }
        if tokio::time::timeout_at(deadline, rx.changed())
            .await
            .map_or(true, |r| r.is_err())
        {
            return (s.state() == State::Ready, s.live().tools.len() as u64);
        }
    }
}

fn waiting(name: &str, host: &str) -> McpLoginFinishResult {
    McpLoginFinishResult {
        state: "waiting".into(),
        name: name.into(),
        host: host.into(),
        ..Default::default()
    }
}
