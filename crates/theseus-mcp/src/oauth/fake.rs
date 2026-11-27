//! A fake authorization server (issue 55), for tests: on 127.0.0.1, its
//! metadata (RFC 8414, or OpenID discovery's address alone), registration,
//! an `/authorize` that grants at once (a 302 to the redirect with a code),
//! the token endpoint (the code with its PKCE verifier checked, one use; the
//! refresh, rotating when asked; refusals by their standard codes), and the
//! revoke. The fake MCP server ([`crate::fake::Config::oauth`]) asks it
//! whether a token is good, and serves the resource's metadata it describes.
//!
//! It keeps every value it issued and every verifier it was sent
//! ([`AuthSeen::secrets`]), so a test can look for each one where it must
//! never be.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use reqwest::Url;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// Where the fake MCP server says its resource metadata is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataAt {
    /// In the 401's `WWW-Authenticate`, and at the path-inserted well-known
    /// address.
    #[default]
    Header,
    /// Only at `/.well-known/oauth-protected-resource/mcp`.
    PathWellKnown,
    /// Only at `/.well-known/oauth-protected-resource`.
    RootWellKnown,
    /// Nowhere: the server's own origin is its authorization server.
    Nowhere,
}

#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Offer a registration endpoint.
    pub register: bool,
    /// The one client a server without registration knows.
    pub client_id: String,
    /// The access token's `expires_in`.
    pub access_ttl: u64,
    /// A refresh gives a new refresh token, and the old one is spent.
    pub rotate: bool,
    /// List S256 among its PKCE methods (else only `plain`).
    pub s256: bool,
    /// The `issuer` its metadata names, when not its own address.
    pub issuer: Option<String>,
    /// Its token endpoint, when not its own `/token`.
    pub token_endpoint: Option<String>,
    /// Serve its metadata only at OpenID discovery's address.
    pub oidc_only: bool,
    /// Every refresh is refused, `invalid_grant`.
    pub refuse_refresh: bool,
    /// The resource's metadata names this `resource` instead of the server.
    pub resource: Option<String>,
    pub metadata_at: MetadataAt,
    /// Keys left out of its metadata and the resource's, to test what a
    /// server may leave out.
    pub omit: Vec<String>,
    /// Registration gives a client secret (sent in the form).
    pub client_secret: bool,
    /// A hostile server: the fake MCP server echoes the token it was sent in
    /// its 401's `WWW-Authenticate`, and in a JSON-RPC error's message and
    /// data to `tools/list` and `tools/call`.
    pub echo: bool,
    /// The redirect names the issuer (`iss`).
    pub iss: bool,
    /// Its metadata says the redirect names the issuer.
    pub iss_supported: bool,
    /// Its authorization endpoint, when not its own `/authorize`.
    pub authorization_endpoint: Option<String>,
    /// The `token_type` its token answers name.
    pub token_type: String,
    /// Its access tokens are this short, too short to keep from view.
    pub short_access: bool,
    /// The resource's metadata names this many authorization servers before
    /// the fake itself, none of which publishes metadata.
    pub decoys: usize,
    /// A hostile server: `tools/call` answers a successful envelope with
    /// `isError = true`, the token it was sent in its content and its
    /// structured content.
    pub error_echo: bool,
    /// A hostile server: its tool list names a tool after the token it was
    /// last sent, and each call is followed by `tools/list_changed`.
    pub tool_named_token: bool,
    /// A hostile server (issue 55, round 4): its tools and prompts say the
    /// token it was last sent (`fake::Config::says`).
    pub says_token: bool,
    /// Registration gives this as its client secret (an alias of a label).
    pub client_secret_is: Option<String>,
    /// The token endpoint issues this as the access token (an alias of a
    /// label that was public before the redemption).
    pub access_token_is: Option<String>,
    /// A renewal (the refresh grant) issues this as the access token: a
    /// value acquired after a list was accepted (round 5).
    pub renewed_access_is: Option<String>,
    /// The authorization gives this as its code (an alias of a label, round 6).
    pub code_is: Option<String>,
    /// Registration gives its client secret as the client id too.
    pub client_id_is_secret: bool,
    /// Registration gives this as the client id (an old value, aliased).
    pub client_id_alias: Option<String>,
    /// Its authorization, token, registration and revocation endpoints are
    /// all at this other base (which serves them as the fake does).
    pub endpoints_at: Option<String>,
    /// Registration answers with this status, and no client.
    pub register_status: Option<u16>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            register: true,
            client_id: "settings-client".into(),
            access_ttl: 3600,
            rotate: true,
            s256: true,
            issuer: None,
            token_endpoint: None,
            oidc_only: false,
            refuse_refresh: false,
            resource: None,
            metadata_at: MetadataAt::Header,
            omit: Vec::new(),
            client_secret: false,
            echo: false,
            iss: true,
            iss_supported: false,
            authorization_endpoint: None,
            token_type: "Bearer".into(),
            short_access: false,
            decoys: 0,
            error_echo: false,
            tool_named_token: false,
            says_token: false,
            client_secret_is: None,
            access_token_is: None,
            renewed_access_is: None,
            code_is: None,
            client_id_is_secret: false,
            client_id_alias: None,
            endpoints_at: None,
            register_status: None,
        }
    }
}

/// What the fake has seen and issued.
#[derive(Debug, Clone, Default)]
pub struct AuthSeen {
    pub registrations: u32,
    pub redeemed: u32,
    /// Every code a token request carried, redeemed or not (round 6).
    pub codes_sent: Vec<String>,
    pub refreshes: u32,
    pub refused_refreshes: u32,
    /// Code exchanges refused for a verifier that is not the challenge's.
    pub wrong_verifiers: u32,
    /// The `resource` of each authorization, exchange and refresh.
    pub resources: Vec<String>,
    /// Each revoke's `token_type_hint`.
    pub revoked: Vec<String>,
    /// The client id of each exchange.
    pub clients: Vec<String>,
    /// Requests to the fake that carried an `Authorization: Bearer`.
    pub bearers_seen: u32,
    /// Every code, token and client secret issued, and every verifier sent.
    pub secrets: Vec<String>,
    /// The paths of every request.
    pub paths: Vec<String>,
}

struct Grant {
    client_id: String,
    challenge: String,
    redirect: String,
}

#[derive(Default)]
struct Tokens {
    codes: HashMap<String, Grant>,
    access: HashSet<String>,
    refresh: HashMap<String, String>,
    clients: HashSet<String>,
}

pub struct FakeAuth {
    cfg: Mutex<AuthConfig>,
    seen: Mutex<AuthSeen>,
    tokens: Mutex<Tokens>,
    next: AtomicU64,
    addr: OnceLock<SocketAddr>,
    /// The last token the fake MCP server was sent.
    last_bearer: Mutex<Option<String>>,
}

impl std::fmt::Debug for FakeAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeAuth")
            .field("addr", &self.addr.get())
            .finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl FakeAuth {
    /// Serving on 127.0.0.1 at a port the system picks.
    pub async fn start(cfg: AuthConfig) -> Arc<Self> {
        let me = Arc::new(Self {
            cfg: Mutex::new(cfg),
            seen: Mutex::default(),
            tokens: Mutex::default(),
            next: AtomicU64::new(1),
            addr: OnceLock::new(),
            last_bearer: Mutex::default(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("a port");
        let _ = me.addr.set(listener.local_addr().expect("an address"));
        let it = me.clone();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let it = it.clone();
                tokio::spawn(async move { it.conn(stream).await });
            }
        });
        me
    }

    /// Its address, `http://127.0.0.1:<port>`: its issuer.
    pub fn base(&self) -> String {
        format!("http://{}", self.addr.get().expect("started"))
    }

    pub fn config(&self) -> AuthConfig {
        lock(&self.cfg).clone()
    }

    pub fn set(&self, f: impl FnOnce(&mut AuthConfig)) {
        f(&mut lock(&self.cfg));
    }

    pub fn seen(&self) -> AuthSeen {
        lock(&self.seen).clone()
    }

    /// Whether the fake MCP server takes `token`.
    pub fn valid(&self, token: &str) -> bool {
        lock(&self.tokens).access.contains(token)
    }

    /// Every access token stops being good, as if each ran out.
    pub fn expire_access(&self) {
        lock(&self.tokens).access.clear();
    }

    /// Every refresh token stops being good, as if the sign-in were revoked.
    pub fn revoke_all(&self) {
        let mut t = lock(&self.tokens);
        t.access.clear();
        t.refresh.clear();
    }

    /// A value it was told to give, counted among what it issued.
    fn seen_value(&self, v: &str) -> String {
        lock(&self.seen).secrets.push(v.to_string());
        v.to_string()
    }

    fn mint(&self, kind: &str) -> String {
        let n = self.next.fetch_add(1, Ordering::SeqCst);
        let mut buf = [0u8; 18];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut buf);
        let v = format!("fake-{kind}-{n}-{}", URL_SAFE_NO_PAD.encode(buf));
        lock(&self.seen).secrets.push(v.clone());
        v
    }

    /// The fake MCP server was sent `token`.
    pub fn note_bearer(&self, token: &str) {
        *lock(&self.last_bearer) = Some(token.to_string());
    }

    /// With `echo`: the token the fake MCP server was last sent, which it
    /// echoes in its answer to `method`.
    pub fn echo_for(&self, method: &str) -> Option<String> {
        let echoes = self.config().echo && matches!(method, "tools/list" | "tools/call");
        echoes.then(|| lock(&self.last_bearer).clone()).flatten()
    }

    /// The token the fake MCP server was last sent.
    pub fn last_bearer(&self) -> Option<String> {
        lock(&self.last_bearer).clone()
    }

    /// The fake MCP server's 401 header, for a server at `mcp` (its own
    /// `http://host:port`); with `echo`, the token it was sent is in it.
    pub fn challenge(&self, mcp: &str) -> String {
        let mut h = match self.config().metadata_at {
            MetadataAt::Header => format!(
                "Bearer error=\"invalid_token\", resource_metadata=\"{mcp}/.well-known/oauth-protected-resource/mcp\""
            ),
            _ => "Bearer error=\"invalid_token\"".into(),
        };
        if self.config().echo {
            if let Some(t) = lock(&self.last_bearer).clone() {
                h.push_str(&format!(", error_description=\"{t}\""));
            }
        }
        h
    }

    /// The resource metadata the fake MCP server at `mcp` serves at `path`,
    /// if it serves any there.
    pub fn resource_metadata(&self, path: &str, mcp: &str) -> Option<Value> {
        let cfg = self.config();
        let serves = match cfg.metadata_at {
            MetadataAt::Header | MetadataAt::PathWellKnown => {
                path == "/.well-known/oauth-protected-resource/mcp"
            }
            MetadataAt::RootWellKnown => path == "/.well-known/oauth-protected-resource",
            MetadataAt::Nowhere => false,
        };
        serves.then(|| {
            let base = self.base();
            let mut servers: Vec<String> = (0..cfg.decoys)
                .map(|i| format!("{base}/decoy-{i}"))
                .collect();
            servers.push(base);
            let mut doc = json!({
                "resource": cfg.resource.clone().unwrap_or(format!("{mcp}/mcp")),
                "authorization_servers": servers,
                "scopes_supported": ["tools"],
            });
            if let Some(o) = doc.as_object_mut() {
                for k in &cfg.omit {
                    o.remove(k);
                }
            }
            doc
        })
    }

    fn metadata(&self) -> Value {
        let cfg = self.config();
        let issuer = cfg.issuer.clone().unwrap_or(self.base());
        let base = cfg.endpoints_at.clone().unwrap_or(self.base());
        let mut m = json!({
            "issuer": issuer,
            "authorization_endpoint": cfg.authorization_endpoint.clone().unwrap_or(format!("{base}/authorize")),
            "token_endpoint": cfg.token_endpoint.clone().unwrap_or(format!("{base}/token")),
            "revocation_endpoint": format!("{base}/revoke"),
            "response_types_supported": ["code"],
            "code_challenge_methods_supported": if cfg.s256 { json!(["S256"]) } else { json!(["plain"]) },
        });
        if cfg.register {
            m["registration_endpoint"] = json!(format!("{base}/register"));
        }
        if cfg.iss_supported {
            m["authorization_response_iss_parameter_supported"] = json!(true);
        }
        if let Some(o) = m.as_object_mut() {
            for k in &cfg.omit {
                o.remove(k);
            }
        }
        m
    }

    /// What the owner's browser does: open the authorization address, and
    /// follow the fake's redirect to the loopback listener. The address the
    /// browser was sent to is returned (what the owner would paste).
    pub async fn browse(&self, authorize_url: &str, deliver: bool) -> String {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap();
        let resp = http
            .get(authorize_url)
            .send()
            .await
            .expect("the fake answers");
        let to = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .expect("a redirect")
            .to_string();
        if deliver {
            let _ = http.get(&to).send().await;
        }
        to
    }

    async fn conn(self: Arc<Self>, stream: TcpStream) {
        let (r, mut w) = stream.into_split();
        let mut r = BufReader::new(r);
        let mut line = String::new();
        if r.read_line(&mut line).await.is_err() {
            return;
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let target = parts.next().unwrap_or("").to_string();
        let mut headers = HashMap::new();
        loop {
            let mut h = String::new();
            match r.read_line(&mut h).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
            }
        }
        let len: usize = headers
            .get("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0; len.min(1 << 20)];
        if r.read_exact(&mut body).await.is_err() {
            return;
        }
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        {
            let mut s = lock(&self.seen);
            s.paths.push(path.to_string());
            if headers
                .get("authorization")
                .is_some_and(|a| a.to_ascii_lowercase().starts_with("bearer"))
            {
                s.bearers_seen += 1;
            }
        }
        let (status, extra, out) = self.answer(&method, path, query, &headers, &body);
        let mut head = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n");
        for (k, v) in extra {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        let text = out.map(|v| v.to_string()).unwrap_or_default();
        head.push_str(&format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            text.len()
        ));
        let _ = w.write_all(head.as_bytes()).await;
        let _ = w.write_all(text.as_bytes()).await;
        let _ = w.flush().await;
        let _ = tokio::time::timeout(Duration::from_millis(50), w.shutdown()).await;
    }

    fn answer(
        &self,
        method: &str,
        path: &str,
        query: &str,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> (u16, Vec<(String, String)>, Option<Value>) {
        let cfg = self.config();
        match (method, path) {
            ("GET", "/.well-known/oauth-authorization-server") if !cfg.oidc_only => {
                (200, vec![], Some(self.metadata()))
            }
            ("GET", "/.well-known/openid-configuration") => (200, vec![], Some(self.metadata())),
            ("POST", "/register") if cfg.register => self.register(body),
            ("GET", "/authorize") => self.authorize(query),
            ("POST", "/token") => self.token(&form(body), headers),
            ("POST", "/revoke") => {
                let f = form(body);
                let token = f.get("token").cloned().unwrap_or_default();
                let mut t = lock(&self.tokens);
                t.access.remove(&token);
                t.refresh.remove(&token);
                drop(t);
                lock(&self.seen)
                    .revoked
                    .push(f.get("token_type_hint").cloned().unwrap_or_default());
                (200, vec![], None)
            }
            _ => (404, vec![], None),
        }
    }

    fn register(&self, body: &[u8]) -> (u16, Vec<(String, String)>, Option<Value>) {
        let Ok(v) = serde_json::from_slice::<Value>(body) else {
            return (
                400,
                vec![],
                Some(json!({"error": "invalid_client_metadata"})),
            );
        };
        if v["redirect_uris"].as_array().is_none_or(Vec::is_empty) {
            return (400, vec![], Some(json!({"error": "invalid_redirect_uri"})));
        }
        let cfg = self.config();
        if let Some(status) = cfg.register_status {
            return (
                status,
                vec![],
                Some(json!({"error": "invalid_client_metadata"})),
            );
        }
        let secret = match &cfg.client_secret_is {
            Some(v) => Some(self.seen_value(v)),
            None => (cfg.client_secret || cfg.client_id_is_secret).then(|| self.mint("cs")),
        };
        let id = match (&secret, cfg.client_id_is_secret, &cfg.client_id_alias) {
            (Some(s), true, _) => s.clone(),
            (_, _, Some(alias)) => alias.clone(),
            _ => format!("fake-client-{}", self.next.fetch_add(1, Ordering::SeqCst)),
        };
        lock(&self.tokens).clients.insert(id.clone());
        lock(&self.seen).registrations += 1;
        let answer = match secret {
            Some(s) => json!({"client_id": id, "client_secret": s,
                "token_endpoint_auth_method": "client_secret_post"}),
            None => json!({"client_id": id, "token_endpoint_auth_method": "none"}),
        };
        (201, vec![], Some(answer))
    }

    fn authorize(&self, query: &str) -> (u16, Vec<(String, String)>, Option<Value>) {
        let q = form(query.as_bytes());
        let get = |k: &str| q.get(k).cloned().unwrap_or_default();
        let client = get("client_id");
        let known =
            lock(&self.tokens).clients.contains(&client) || client == self.config().client_id;
        if !known || get("code_challenge_method") != "S256" {
            return (400, vec![], Some(json!({"error": "invalid_request"})));
        }
        lock(&self.seen).resources.push(get("resource"));
        let code = match self.config().code_is {
            Some(v) => self.seen_value(&v),
            None => self.mint("code"),
        };
        lock(&self.tokens).codes.insert(
            code.clone(),
            Grant {
                client_id: client,
                challenge: get("code_challenge"),
                redirect: get("redirect_uri"),
            },
        );
        let mut to = Url::parse(&get("redirect_uri")).expect("a redirect");
        to.query_pairs_mut()
            .append_pair("code", &code)
            .append_pair("state", &get("state"));
        if self.config().iss {
            to.query_pairs_mut()
                .append_pair("iss", &self.config().issuer.unwrap_or(self.base()));
        }
        (302, vec![("location".into(), to.to_string())], None)
    }

    fn token(
        &self,
        f: &HashMap<String, String>,
        headers: &HashMap<String, String>,
    ) -> (u16, Vec<(String, String)>, Option<Value>) {
        let refused = |code: &str| {
            (
                400,
                vec![],
                Some(json!({"error": code, "error_description": "the fake says no"})),
            )
        };
        let get = |k: &str| f.get(k).cloned().unwrap_or_default();
        let _ = headers;
        lock(&self.seen).resources.push(get("resource"));
        match get("grant_type").as_str() {
            "authorization_code" => {
                lock(&self.seen).codes_sent.push(get("code"));
                let verifier = get("code_verifier");
                lock(&self.seen).secrets.push(verifier.clone());
                let Some(g) = lock(&self.tokens).codes.remove(&get("code")) else {
                    return refused("invalid_grant");
                };
                let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
                if challenge != g.challenge {
                    lock(&self.seen).wrong_verifiers += 1;
                    return refused("invalid_grant");
                }
                if g.client_id != get("client_id") || g.redirect != get("redirect_uri") {
                    return refused("invalid_grant");
                }
                {
                    let mut s = lock(&self.seen);
                    s.redeemed += 1;
                    s.clients.push(g.client_id.clone());
                }
                let refresh = self.mint("rt");
                lock(&self.tokens)
                    .refresh
                    .insert(refresh.clone(), g.client_id);
                (200, vec![], Some(self.issue(Some(&refresh), false)))
            }
            "refresh_token" => {
                let old = get("refresh_token");
                let owner = lock(&self.tokens).refresh.get(&old).cloned();
                let cfg = self.config();
                if cfg.refuse_refresh || owner.is_none_or(|c| c != get("client_id")) {
                    lock(&self.seen).refused_refreshes += 1;
                    return refused("invalid_grant");
                }
                lock(&self.seen).refreshes += 1;
                if cfg.rotate {
                    let new = self.mint("rt");
                    let mut t = lock(&self.tokens);
                    let c = t.refresh.remove(&old).unwrap_or_default();
                    t.refresh.insert(new.clone(), c);
                    drop(t);
                    (200, vec![], Some(self.issue(Some(&new), true)))
                } else {
                    (200, vec![], Some(self.issue(None, true)))
                }
            }
            _ => refused("unsupported_grant_type"),
        }
    }

    fn issue(&self, refresh: Option<&str>, renewal: bool) -> Value {
        let renewed_is = self.config().renewed_access_is.filter(|_| renewal);
        let access = match (self.config().short_access, self.config().access_token_is) {
            _ if renewed_is.is_some() => self.seen_value(&renewed_is.unwrap_or_default()),
            (_, Some(v)) => self.seen_value(&v),
            // Eight bytes, one of them a character: the scrubber reads it as one.
            (true, None) => "t       ".to_string(),
            (false, None) => self.mint("at"),
        };
        lock(&self.tokens).access.insert(access.clone());
        let mut v = json!({
            "access_token": access,
            "token_type": self.config().token_type,
            "expires_in": self.config().access_ttl,
            "scope": "tools",
        });
        if let Some(r) = refresh {
            v["refresh_token"] = json!(r);
        }
        v
    }
}

/// A form body (or a query) as its pairs.
fn form(body: &[u8]) -> HashMap<String, String> {
    let mut u = Url::parse("http://x.invalid/").expect("a URL");
    u.set_query(Some(&String::from_utf8_lossy(body)));
    u.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
