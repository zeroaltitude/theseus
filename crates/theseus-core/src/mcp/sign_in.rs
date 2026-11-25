//! `theseus mcp login <name>` and `logout` (issue 55): the owner signs in to a
//! server that signs in (`oauth`), in a browser. The three methods are
//! admitted from the CLI's surface alone (`rpc/mcp_login.rs`), as
//! `google.auth.*` are; what stops an agent's job from making them is that
//! surface, the CLI's own refusal inside a job, and the L1 sandbox, which
//! hides the daemon's socket from a sandboxed job. A process at L0 that
//! reaches the socket with the CLI's surface is not stopped here.
//!
//! The daemon does every step that touches a value: it asks the server with
//! no token and reads its 401's challenge, discovers its authorization
//! server (`theseus_mcp::oauth::discover`), registers a client of its own
//! when the server offers that (else the settings' `oauth.client_id`), all
//! within one deadline; draws the PKCE verifier and the `state`, and listens
//! once on `127.0.0.1` for the redirect. The CLI is given the address to
//! open, which holds no secret, the host it signs in at, and every other host
//! what the sign-in returns is sent to, which the owner must agree to. The
//! code comes from the listener, or from the address the owner pastes (over
//! the socket, in a param), which must be the redirect whole; it is redeemed
//! here, the tokens are kept (`oauth.rs`), and the server is started again.
//! On WSL the browser cannot reach the loopback, so the paste comes at once.

use std::sync::Arc;
use std::time::{Duration, Instant};

use theseus_mcp::oauth::discover::{self, Discovered};
use theseus_mcp::oauth::exchange;
use theseus_mcp::oauth::register::{self, ClientId};
use theseus_mcp::oauth::request::{self, Code, Expect, Loopback, Pkce, RedirectError};
use theseus_mcp::oauth::Reach;
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
/// The one deadline over the probe, discovery and registration.
const DISCOVER_FOR: Duration = Duration::from_secs(60);

/// A sign-in waiting for its redirect. It owns its listener: dropped (it
/// ends, or another sign-in replaces it), the listener closes.
pub(crate) struct Pending {
    id: String,
    server: String,
    pkce: Pkce,
    redirect: String,
    /// What a pasted address must be.
    paste: Expect,
    client: ClientId,
    found: Discovered,
    host: String,
    /// What the sign-in returns goes to a host other than the issuer's: the
    /// owner must have agreed.
    agree: bool,
    rx: Option<oneshot::Receiver<Result<Code, RedirectError>>>,
    listener: tokio::task::AbortHandle,
    began: Instant,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.listener.abort();
    }
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

/// Every other place a value of the sign-in is sent to, when its endpoints
/// are not all the issuer's: each is shown on a line of its own.
fn places_elsewhere(server: &discover::AuthServer) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if server.elsewhere.is_empty() {
        return out;
    }
    let signs_at = place(&server.authorization_endpoint);
    for u in [Some(&server.token_endpoint)]
        .into_iter()
        .chain([
            server.registration_endpoint.as_ref(),
            server.revocation_endpoint.as_ref(),
        ])
        .flatten()
    {
        let p = place(u);
        if p != signs_at && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Where an address is, for the owner to read: its host, and its port when
/// it is not the scheme's own.
fn place(u: &reqwest::Url) -> String {
    let host = u.host_str().unwrap_or("?");
    match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    }
}

impl McpBoard {
    /// The server `name`, which must sign in at an `https` address; its
    /// address, and its sign-in.
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
        if !super::oauth::signs_in_at(&url) {
            return Err(format!(
                "the MCP server {name} is not at an https address, and a server that signs in \
                 must be"
            ));
        }
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
        let reach = signed
            .reach()
            .cloned()
            .ok_or("the sign-in's HTTP client could not start")?;
        let loopback = Loopback::bind()
            .await
            .map_err(|_| "the loopback listener could not start".to_string())?;
        let redirect = loopback.redirect();
        let settings = s.cfg.oauth.clone().unwrap_or_default();
        let found = tokio::time::timeout(
            DISCOVER_FOR,
            discover_and_register(&reach, name, &url, &redirect, settings.client_id.as_deref()),
        )
        .await
        .map_err(|_| {
            format!(
                "the sign-in of {name} found no authorization server within {} s: nothing was kept",
                DISCOVER_FOR.as_secs()
            )
        })??;
        let (found, client) = found;
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
        let elsewhere = places_elsewhere(&found.server);
        let (tx, rx) = oneshot::channel();
        let waits = loopback.expect(&pkce, &found.server);
        let paste = loopback.expect(&pkce, &found.server);
        let hold = reach.holder();
        let listener = tokio::spawn(async move {
            let _ = tx.send(loopback.wait(waits, hold, OPEN_FOR).await);
        })
        .abort_handle();
        let id = uuid::Uuid::now_v7().simple().to_string();
        // A sign-in begun before is replaced: its listener closes.
        *self.login.lock().await = Some(Pending {
            id: id.clone(),
            server: name.to_string(),
            agree: !found.server.elsewhere.is_empty(),
            pkce,
            redirect: redirect.clone(),
            paste,
            client,
            found,
            host: host.clone(),
            rx: Some(rx),
            listener,
            began: Instant::now(),
        });
        tracing::info!(server = name, "an MCP sign-in began");
        Ok(McpLoginBeginResult {
            login_id: id,
            name: name.to_string(),
            host,
            url: at,
            redirect: if manual { String::new() } else { redirect },
            expires_secs: OPEN_FOR.as_secs(),
            paste_after_secs: if manual { 0 } else { PASTE_AFTER.as_secs() },
            elsewhere,
        })
    }

    /// Wait for the redirect, or take the owner's paste, and finish: the code
    /// redeemed, the tokens kept, and the server started again with them.
    pub async fn login_finish(
        self: &Arc<Self>,
        p: &McpLoginFinishParams,
    ) -> Result<McpLoginFinishResult, String> {
        let (code, pending) = match self.redirect_of(p).await? {
            Got::Code(code, pending) => (code, pending),
            Got::Waiting(w) => return Ok(w),
        };
        let (s, url, signed) = self.signing(&pending.server)?;
        let reach: Reach = signed
            .reach()
            .cloned()
            .ok_or("the sign-in's HTTP client could not start")?;
        let tokens = exchange::redeem(
            &reach,
            &pending.found.server.token_endpoint,
            &pending.client,
            &code,
            pending.pkce.verifier(),
            &pending.redirect,
            &pending.found.resource,
        )
        .await
        .map_err(|e| format!("the sign-in was not completed: {e}"))?;
        if let Err(e) = signed.put(kept_of(&url, &pending, &tokens)).await {
            return Err(format!(
                "the sign-in could not be kept ({e}), so it is not used: nothing changed. Run \
                 `theseus mcp login {}` again",
                s.name
            ));
        }
        tracing::info!(server = %s.name, "an MCP server was signed in");
        let changes = s.changed.subscribe();
        self.restart(&s.name);
        let (connected, tools) = came_up(&s, changes).await;
        Ok(McpLoginFinishResult {
            state: "done".into(),
            name: s.name.clone(),
            host: pending.host.clone(),
            connected,
            tools,
            note: String::new(),
        })
    }

    /// The code of the open sign-in `p` names: its paste, or its listener's
    /// redirect (waited for `p.wait_secs`); or that it is still waiting. The
    /// sign-in is taken off the board with its code: one use.
    async fn redirect_of(&self, p: &McpLoginFinishParams) -> Result<Got, String> {
        let mut slot = self.login.lock().await;
        let Some(pending) = slot
            .as_mut()
            .filter(|c| c.id == p.login_id && c.began.elapsed() < OPEN_FOR)
        else {
            *slot = None;
            return Err("no sign-in is open: run `theseus mcp login` again".into());
        };
        if pending.agree && !p.agreed {
            *slot = None;
            return Err(
                "what this sign-in returns is sent to a host other than its own, and that was \
                 not agreed to: nothing was sent"
                    .into(),
            );
        }
        let (_, _, signed) = self.signing(&pending.server)?;
        let code = match p.pasted.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(pasted) => {
                let hold: theseus_mcp::oauth::Holder =
                    Arc::new(move |what, v| signed.hold_value(what, v));
                request::parse_pasted(pasted, &pending.paste, &hold).map_err(|e| e.to_string())?
            }
            None => {
                let Some(rx) = pending.rx.as_mut() else {
                    return Err("the redirect was taken: run `theseus mcp login` again".into());
                };
                match tokio::time::timeout(Duration::from_secs(p.wait_secs), rx).await {
                    Err(_) => return Ok(Got::Waiting(waiting(&pending.server, &pending.host))),
                    Ok(Err(_)) => {
                        *slot = None;
                        return Err("the listener stopped: run `theseus mcp login` again".into());
                    }
                    Ok(Ok(Err(e))) => {
                        *slot = None;
                        return Err(e.to_string());
                    }
                    Ok(Ok(Ok(code))) => code,
                }
            }
        };
        // The code is one use: whatever follows, this sign-in is over, and
        // its listener closes when the pending sign-in goes.
        let pending = slot.take().expect("a pending sign-in");
        Ok(Got::Code(code, pending))
    }

    /// Forget `name`'s sign-in: the file removed, revoked at its server when
    /// it offers that, and the server started again (now `needs sign-in`). A
    /// file that could not be removed is said: it would be used again at the
    /// daemon's next start.
    pub async fn logout(self: &Arc<Self>, name: &str) -> Result<McpLogoutResult, String> {
        let (s, _, signed) = self.signing(name)?;
        let (kept, removed) = signed.forget().await;
        let mut notes = Vec::new();
        let mut revoked = false;
        if let (Some(k), Some(reach)) = (&kept, signed.reach()) {
            match k
                .revocation_endpoint
                .as_deref()
                .map(|e| reach.endpoint(e, "revocation endpoint"))
            {
                Some(Ok(ep)) => {
                    let client = k.client();
                    let mut all = true;
                    if let Some(r) = &k.refresh_token {
                        all &= exchange::revoke(reach, &ep, &client, r, "refresh_token")
                            .await
                            .map_err(|e| {
                                notes.push(format!("the refresh token was not revoked ({e})"))
                            })
                            .is_ok();
                    }
                    all &= exchange::revoke(reach, &ep, &client, &k.access_token, "access_token")
                        .await
                        .map_err(|e| notes.push(format!("the access token was not revoked ({e})")))
                        .is_ok();
                    revoked = all;
                }
                Some(Err(_)) | None => notes
                    .push("its server offers no revoke: the sign-in ends when it runs out".into()),
            }
        }
        let kept_remains = removed.is_err();
        if let Err(e) = removed {
            notes.push(format!(
                "the kept sign-in could not be removed ({e}): it would be used again when the \
                 daemon next starts"
            ));
        }
        // What was kept stays held on the secret board, so the scrubber still
        // knows each value.
        tracing::info!(server = name, revoked, "an MCP server was signed out");
        self.restart(&s.name);
        Ok(McpLogoutResult {
            name: name.to_string(),
            forgot: kept.is_some() && !kept_remains,
            revoked,
            note: notes.join("; "),
            kept_remains,
        })
    }
}

/// What the redirect gave: the code and its sign-in, or "still waiting".
enum Got {
    Code(Code, Pending),
    Waiting(McpLoginFinishResult),
}

/// What a finished sign-in keeps: its address, what discovery found, its
/// client, and the tokens.
fn kept_of(url: &reqwest::Url, pending: &Pending, tokens: &exchange::Tokens) -> Kept {
    let now = theseus_protocol::now_unix_ms();
    let server = &pending.found.server;
    let mut kept = Kept {
        url: url.to_string(),
        resource: pending.found.resource.clone(),
        issuer: server.issuer.clone(),
        token_endpoint: server.token_endpoint.to_string(),
        revocation_endpoint: server.revocation_endpoint.as_ref().map(ToString::to_string),
        elsewhere: server.elsewhere.clone(),
        client_id: pending.client.id.clone(),
        client_secret: pending.client.secret.as_ref().map(|s| s.to_string()),
        client_basic: pending.client.basic,
        access_token: String::new(),
        refresh_token: None,
        expires_at_ms: None,
        signed_in_at_ms: now,
    };
    kept.renewed(tokens, now);
    kept
}

/// Ask the server with no token, read its challenge, discover its
/// authorization server, and take a client: registered, or the settings'.
async fn discover_and_register(
    reach: &Reach,
    name: &str,
    url: &reqwest::Url,
    redirect: &str,
    client_id: Option<&str>,
) -> Result<(Discovered, ClientId), String> {
    let at = reach
        .endpoint(url.as_str(), "server's address")
        .map_err(|e| e.to_string())?;
    let asked = reach
        .client()
        .post(at.clone())
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
        .filter(|v| v.len() <= theseus_mcp::oauth::MAX_URL)
        .and_then(discover::challenge);
    drop(asked);
    let found = discover::discover(reach, &at, challenge.as_ref())
        .await
        .map_err(|e| e.to_string())?;
    let client = match (client_id, &found.server.registration_endpoint) {
        (Some(id), _) => ClientId::named(id),
        (None, Some(ep)) => register::register(reach, ep, redirect)
            .await
            .map_err(|e| e.to_string())?,
        (None, None) => {
            return Err(format!(
                "the authorization server of {name} offers no registration, and the server's \
                 settings name no `oauth.client_id`: the owner asks the server's maker for a \
                 client id, and it is added to the settings"
            ))
        }
    };
    Ok((found, client))
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
