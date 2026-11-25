//! The authorization request and its redirect: a fresh PKCE verifier (S256)
//! and `state`, the address the owner opens (it holds no secret), a one-use
//! listener on `127.0.0.1` at a port the system picks, alive only while its
//! sign-in waits, and the redirect's code, taken only with this sign-in's
//! `state` (and its issuer, when the redirect names one, RFC 9207). The owner
//! may paste the address the browser was sent to instead.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use reqwest::Url;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use zeroize::Zeroizing;

use super::discover::AuthServer;
use super::{standard_code, Holder, MIN_SECRET};

/// How long a sign-in's listener waits for the redirect.
pub const LISTEN_FOR: Duration = Duration::from_secs(600);
/// The path the redirect comes back to.
pub const CALLBACK: &str = "/callback";

fn random_b64(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// A sign-in's PKCE pair and its `state`. The verifier stays in the daemon.
pub struct Pkce {
    verifier: Zeroizing<String>,
    pub challenge: String,
    pub state: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = Zeroizing::new(random_b64(32));
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
            state: random_b64(24),
        }
    }

    pub fn verifier(&self) -> &str {
        &self.verifier
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// The address the owner opens. It holds the client id, the challenge, the
/// state, the redirect, the resource and the scope, and no secret.
pub fn authorize_url(
    server: &AuthServer,
    client_id: &str,
    redirect: &str,
    pkce: &Pkce,
    resource: &str,
    scope: Option<&str>,
) -> String {
    let mut u = server.authorization_endpoint.clone();
    {
        let mut q = u.query_pairs_mut();
        q.append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", redirect)
            .append_pair("code_challenge", &pkce.challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &pkce.state)
            .append_pair("resource", resource);
        if let Some(s) = scope.filter(|s| !s.trim().is_empty()) {
            q.append_pair("scope", s);
        }
    }
    u.to_string()
}

/// An authorization code: one use, held in memory, never shown.
pub struct Code(Zeroizing<String>);

impl Code {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Code(<hidden>)")
    }
}

/// Why a redirect gave no code. No variant holds the code or the state.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RedirectError {
    #[error("the redirect's state is not this sign-in's: nothing was kept")]
    StateMismatch,
    #[error("the redirect names another issuer: nothing was kept")]
    IssuerMismatch,
    #[error("the redirect does not name its issuer, which this sign-in needs: nothing was kept")]
    NoIssuer,
    #[error("the redirect repeats a part of it: nothing was kept")]
    Repeated,
    #[error("the redirect's code is too short to keep from view: nothing was kept")]
    ShortCode,
    #[error(
        "the address is not this sign-in's redirect: paste the whole address your browser was \
         sent to"
    )]
    NotThisAddress,
    #[error("the address holds no code: paste the whole address your browser was sent to")]
    NoCode,
    #[error("the sign-in was not granted ({0})")]
    Denied(String),
    #[error("no redirect came within {} minutes", LISTEN_FOR.as_secs() / 60)]
    Timeout,
    #[error("the listener failed")]
    Listener,
}

/// What a redirect must be to be this sign-in's: the exact address the
/// listener is on, the `state`, and the issuer, which it must name when
/// `iss_required` (an authorization server whose endpoints are elsewhere).
pub struct Expect {
    pub redirect: Url,
    pub state: String,
    pub issuer: String,
    pub iss_required: bool,
}

/// The parts of a redirect that are read, each of which may come once.
const READ: [&str; 4] = ["code", "state", "iss", "error"];

/// The code in the address the owner pasted: it must be this sign-in's
/// redirect whole (its scheme, host, port and path), with its query.
pub fn parse_pasted(text: &str, expect: &Expect, hold: &Holder) -> Result<Code, RedirectError> {
    let u = Url::parse(text.trim()).map_err(|_| RedirectError::NotThisAddress)?;
    // Every code it carries is held first, whatever it turns out to be.
    hold_codes(u.query().unwrap_or(""), hold);
    let r = &expect.redirect;
    if u.scheme() != r.scheme()
        || u.host_str() != r.host_str()
        || u.port_or_known_default() != r.port_or_known_default()
        || u.path() != r.path()
        || u.fragment().is_some()
    {
        return Err(RedirectError::NotThisAddress);
    }
    parse_query(u.query().unwrap_or(""), expect)
}

/// The code in a request the listener took: its target's path must be the
/// redirect's exactly.
fn parse_target(target: &str, expect: &Expect, hold: &Holder) -> Result<Code, RedirectError> {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    hold_codes(query, hold);
    if path != expect.redirect.path() {
        return Err(RedirectError::NotThisAddress);
    }
    parse_query(query, expect)
}

fn hold_codes(query: &str, hold: &Holder) {
    for (k, v) in url_pairs(query) {
        if k == "code" && !v.is_empty() {
            hold("code", &v);
        }
    }
}

/// The code in a redirect's query: each part read comes once, the state is
/// this sign-in's, the issuer the one asked (and named, when it must be).
fn parse_query(query: &str, expect: &Expect) -> Result<Code, RedirectError> {
    let pairs: Vec<(String, String)> = url_pairs(query);
    if READ
        .iter()
        .any(|k| pairs.iter().filter(|(n, _)| n == k).count() > 1)
    {
        return Err(RedirectError::Repeated);
    }
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if get("state") != Some(expect.state.as_str()) {
        return Err(RedirectError::StateMismatch);
    }
    match get("iss") {
        Some(iss) if iss.trim_end_matches('/') != expect.issuer.trim_end_matches('/') => {
            return Err(RedirectError::IssuerMismatch)
        }
        None if expect.iss_required => return Err(RedirectError::NoIssuer),
        _ => {}
    }
    if let Some(e) = get("error") {
        return Err(RedirectError::Denied(
            standard_code(e).unwrap_or("a code of its own").to_string(),
        ));
    }
    match get("code") {
        Some(c) if c.len() >= MIN_SECRET => Ok(Code(Zeroizing::new(c.to_string()))),
        Some(c) if !c.is_empty() => Err(RedirectError::ShortCode),
        _ => Err(RedirectError::NoCode),
    }
}

fn url_pairs(query: &str) -> Vec<(String, String)> {
    let mut u = Url::parse("http://x.invalid/").expect("a URL");
    u.set_query(Some(query));
    u.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// The one-use listener on `127.0.0.1`, at a port the system picks.
pub struct Loopback {
    listener: TcpListener,
    pub port: u16,
}

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Theseus</title>\
<body style=\"font-family:sans-serif;margin:3em\"><h2>Theseus</h2><p>";

impl Loopback {
    pub async fn bind() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        Ok(Self { listener, port })
    }

    pub fn redirect(&self) -> String {
        format!("http://127.0.0.1:{}{CALLBACK}", self.port)
    }

    /// What a redirect to this listener must be, for `pkce`'s sign-in at
    /// `server`.
    pub fn expect(&self, pkce: &Pkce, server: &AuthServer) -> Expect {
        Expect {
            redirect: Url::parse(&self.redirect()).expect("a loopback address"),
            state: pkce.state.clone(),
            issuer: server.issuer.clone(),
            iss_required: !server.elsewhere.is_empty(),
        }
    }

    /// The code from the first redirect with this sign-in's state, or why
    /// there was none. A stray request (a favicon, a probe, another state)
    /// is answered and waited past; a code any request carries is held. The
    /// listener closes when this returns, or when its task is dropped.
    pub async fn wait(
        self,
        expect: Expect,
        hold: Holder,
        within: Duration,
    ) -> Result<Code, RedirectError> {
        let run = async {
            loop {
                let (mut stream, _) = self
                    .listener
                    .accept()
                    .await
                    .map_err(|_| RedirectError::Listener)?;
                let Some(target) = read_target(&mut stream).await else {
                    continue;
                };
                match parse_target(&target, &expect, &hold) {
                    Ok(code) => {
                        respond(&mut stream, 200, "Signed in. You can close this tab.").await;
                        return Ok(code);
                    }
                    Err(RedirectError::NotThisAddress) => {
                        respond(&mut stream, 404, "Not found.").await;
                    }
                    Err(RedirectError::StateMismatch | RedirectError::Repeated) => {
                        respond(&mut stream, 400, "That redirect is not for this sign-in.").await;
                    }
                    Err(e) => {
                        respond(&mut stream, 200, "Not signed in. You can close this tab.").await;
                        return Err(e);
                    }
                }
            }
        };
        tokio::time::timeout(within, run)
            .await
            .unwrap_or(Err(RedirectError::Timeout))
    }
}

/// A request's target from its first line, read within a few seconds.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    let read = async {
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Some(())
    };
    tokio::time::timeout(Duration::from_secs(5), read)
        .await
        .ok()??;
    let text = String::from_utf8_lossy(&buf);
    let line = text.lines().next()?;
    let mut parts = line.split_whitespace();
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(String::from)
}

async fn respond(stream: &mut TcpStream, status: u16, words: &str) {
    let body = format!("{PAGE}{words}</p>");
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: text/html\r\ncontent-length: {}\r\n\
         connection: close\r\ncache-control: no-store\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body.as_bytes()).await;
    let _ = stream.flush().await;
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    fn server() -> AuthServer {
        AuthServer {
            issuer: "https://as.example.invalid".into(),
            authorization_endpoint: Url::parse("https://as.example.invalid/authorize?x=1").unwrap(),
            token_endpoint: Url::parse("https://as.example.invalid/token").unwrap(),
            registration_endpoint: None,
            revocation_endpoint: None,
            iss_in_redirect: false,
            elsewhere: vec![],
        }
    }

    fn expect(p: &Pkce, iss_required: bool) -> Expect {
        Expect {
            redirect: Url::parse("http://127.0.0.1:4000/callback").unwrap(),
            state: p.state.clone(),
            issuer: "https://as.example.invalid".into(),
            iss_required,
        }
    }

    fn holder() -> (Holder, Arc<Mutex<Vec<String>>>) {
        let held = Arc::new(Mutex::new(Vec::new()));
        let h = held.clone();
        (
            Arc::new(move |_, v: &str| h.lock().unwrap().push(v.to_string())),
            held,
        )
    }

    const CODE: &str = "code-value-0001";

    /// The pasted address must be this sign-in's redirect whole; its state,
    /// each part once, and a code long enough to keep from view. Every code
    /// it carries is held, refused or not.
    #[test]
    fn a_redirect_is_this_sign_ins_whole_or_nothing_is_kept() {
        let p = Pkce::new();
        let e = expect(&p, false);
        let (hold, held) = holder();
        let ok = format!(
            "http://127.0.0.1:4000/callback?code={CODE}&state={}",
            p.state
        );
        assert_eq!(parse_pasted(&ok, &e, &hold).unwrap().expose(), CODE);
        for (bad, why) in [
            (
                format!("http://127.0.0.1:4000/callback?code={CODE}&state=someone-elses"),
                RedirectError::StateMismatch,
            ),
            (
                format!("http://127.0.0.1:4000/callback?code={CODE}"),
                RedirectError::StateMismatch,
            ),
            (
                format!(
                    "http://127.0.0.1:4001/callback?code={CODE}&state={}",
                    p.state
                ),
                RedirectError::NotThisAddress,
            ),
            (
                format!(
                    "http://127.0.0.1:4000/callbackx?code={CODE}&state={}",
                    p.state
                ),
                RedirectError::NotThisAddress,
            ),
            (
                format!(
                    "https://127.0.0.1:4000/callback?code={CODE}&state={}",
                    p.state
                ),
                RedirectError::NotThisAddress,
            ),
            (
                format!(
                    "http://localhost:4000/callback?code={CODE}&state={}",
                    p.state
                ),
                RedirectError::NotThisAddress,
            ),
            (
                format!("?code={CODE}&state={}", p.state),
                RedirectError::NotThisAddress,
            ),
            (
                format!(
                    "http://127.0.0.1:4000/callback?code={CODE}&code=other-code-02&state={}",
                    p.state
                ),
                RedirectError::Repeated,
            ),
            (
                format!(
                    "http://127.0.0.1:4000/callback?code={CODE}&state={0}&state={0}",
                    p.state
                ),
                RedirectError::Repeated,
            ),
            (
                format!(
                    "http://127.0.0.1:4000/callback?code=short&state={}",
                    p.state
                ),
                RedirectError::ShortCode,
            ),
            ("hello".to_string(), RedirectError::NotThisAddress),
        ] {
            assert_eq!(parse_pasted(&bad, &e, &hold).unwrap_err(), why, "{bad}");
        }
        assert!(held.lock().unwrap().iter().any(|v| v == "other-code-02"));
        assert!(held.lock().unwrap().iter().any(|v| v == "short"));
        // The listener's own request: the path exactly.
        let t = format!("/callback?code={CODE}&state={}", p.state);
        assert_eq!(parse_target(&t, &e, &hold).unwrap().expose(), CODE);
        let t = format!("/callback/x?code={CODE}&state={}", p.state);
        assert_eq!(
            parse_target(&t, &e, &hold).unwrap_err(),
            RedirectError::NotThisAddress
        );
    }

    /// The redirect's issuer: another is refused, one that differs by a `/`
    /// alone is it, and it must be named when the sign-in needs it; a
    /// refusal is said by its standard code alone.
    #[test]
    fn a_redirect_names_its_issuer_and_a_refusal_its_code() {
        let p = Pkce::new();
        let e = expect(&p, false);
        let (hold, _) = holder();
        let ok = format!("http://127.0.0.1:4000/callback?code={CODE}&state={}", p.state);
        let other = format!(
            "http://127.0.0.1:4000/callback?code={CODE}&state={}&iss=https%3A%2F%2Fevil.invalid",
            p.state
        );
        assert_eq!(
            parse_pasted(&other, &e, &hold).unwrap_err(),
            RedirectError::IssuerMismatch
        );
        let same = format!(
            "http://127.0.0.1:4000/callback?code={CODE}&state={}&iss=https%3A%2F%2Fas.example.invalid%2F",
            p.state
        );
        assert_eq!(parse_pasted(&same, &e, &hold).unwrap().expose(), CODE);
        // An authorization server whose endpoints are elsewhere: the issuer
        // must be named.
        let must = expect(&p, true);
        assert_eq!(
            parse_pasted(&ok, &must, &hold).unwrap_err(),
            RedirectError::NoIssuer
        );
        assert_eq!(parse_pasted(&same, &must, &hold).unwrap().expose(), CODE);
        let denied = format!(
            "http://127.0.0.1:4000/callback?error=access_denied&error_description=no+way&state={}",
            p.state
        );
        assert_eq!(
            parse_pasted(&denied, &e, &hold).unwrap_err(),
            RedirectError::Denied("access_denied".into())
        );
        let own = format!(
            "http://127.0.0.1:4000/callback?error=nope&state={}",
            p.state
        );
        assert_eq!(
            parse_pasted(&own, &e, &hold).unwrap_err().to_string(),
            "the sign-in was not granted (a code of its own)"
        );
    }

    #[test]
    fn the_address_holds_the_challenge_the_resource_and_no_verifier() {
        let p = Pkce::new();
        let u = authorize_url(
            &server(),
            "client-1",
            "http://127.0.0.1:4000/callback",
            &p,
            "https://mcp.example.invalid/mcp",
            Some("read"),
        );
        let url = Url::parse(&u).unwrap();
        let q: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        for (k, v) in [
            ("x", "1"),
            ("response_type", "code"),
            ("client_id", "client-1"),
            ("code_challenge_method", "S256"),
            ("resource", "https://mcp.example.invalid/mcp"),
            ("scope", "read"),
            ("redirect_uri", "http://127.0.0.1:4000/callback"),
        ] {
            assert_eq!(q[k], v, "{k}");
        }
        assert_eq!(q["state"], p.state);
        assert!(!u.contains(p.verifier()));
        let want = URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier().as_bytes()));
        assert_eq!(p.challenge, want);
        assert!(p.verifier().len() >= 43);
    }
}
