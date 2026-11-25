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
use super::standard_code;

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
    #[error("the address holds no code: paste the whole address your browser was sent to")]
    NoCode,
    #[error("the sign-in was not granted ({0})")]
    Denied(String),
    #[error("no redirect came within {} minutes", LISTEN_FOR.as_secs() / 60)]
    Timeout,
    #[error("the listener failed")]
    Listener,
}

/// The code in a redirect: the whole address, or its query alone. The state
/// must be this sign-in's, and an `iss` the redirect carries must be `issuer`.
pub fn parse_redirect(text: &str, state: &str, issuer: &str) -> Result<Code, RedirectError> {
    let text = text.trim();
    let query = match text.contains("://") {
        true => Url::parse(text)
            .map_err(|_| RedirectError::NoCode)?
            .query()
            .unwrap_or("")
            .to_string(),
        false => text
            .rsplit_once('?')
            .map_or(text, |(_, q)| q)
            .trim_start_matches(['?', '/'])
            .to_string(),
    };
    let pairs: Vec<(String, String)> = url_pairs(&query);
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if get("state") != Some(state) {
        return Err(RedirectError::StateMismatch);
    }
    if let Some(iss) = get("iss") {
        if iss.trim_end_matches('/') != issuer.trim_end_matches('/') {
            return Err(RedirectError::IssuerMismatch);
        }
    }
    if let Some(e) = get("error") {
        return Err(RedirectError::Denied(
            standard_code(e).unwrap_or("a code of its own").to_string(),
        ));
    }
    match get("code") {
        Some(c) if !c.is_empty() => Ok(Code(Zeroizing::new(c.to_string()))),
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

    /// The code from the first redirect with this sign-in's state, or why
    /// there was none. A stray request (a favicon, a probe, another state)
    /// is answered and waited past. The listener closes when this returns.
    pub async fn wait(
        self,
        state: &str,
        issuer: &str,
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
                if !target.starts_with(CALLBACK) {
                    respond(&mut stream, 404, "Not found.").await;
                    continue;
                }
                match parse_redirect(&target, state, issuer) {
                    Ok(code) => {
                        respond(&mut stream, 200, "Signed in. You can close this tab.").await;
                        return Ok(code);
                    }
                    Err(RedirectError::StateMismatch) => {
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
    use super::*;

    fn server() -> AuthServer {
        AuthServer {
            issuer: "https://as.example.invalid".into(),
            authorization_endpoint: Url::parse("https://as.example.invalid/authorize?x=1").unwrap(),
            token_endpoint: Url::parse("https://as.example.invalid/token").unwrap(),
            registration_endpoint: None,
            revocation_endpoint: None,
        }
    }

    #[test]
    fn a_state_that_is_not_this_sign_ins_is_refused() {
        let p = Pkce::new();
        let iss = "https://as.example.invalid";
        let ok = format!("http://127.0.0.1:4000/callback?code=abc&state={}", p.state);
        assert_eq!(parse_redirect(&ok, &p.state, iss).unwrap().expose(), "abc");
        for bad in [
            "http://127.0.0.1:4000/callback?code=abc&state=someone-elses".to_string(),
            "http://127.0.0.1:4000/callback?code=abc".to_string(),
            "hello".to_string(),
        ] {
            assert_eq!(
                parse_redirect(&bad, &p.state, iss).unwrap_err(),
                RedirectError::StateMismatch,
                "{bad}"
            );
        }
        let other = format!(
            "http://127.0.0.1:4000/callback?code=abc&state={}&iss=https%3A%2F%2Fevil.invalid",
            p.state
        );
        assert_eq!(
            parse_redirect(&other, &p.state, iss).unwrap_err(),
            RedirectError::IssuerMismatch
        );
        let same = format!(
            "/callback?code=abc&state={}&iss=https%3A%2F%2Fas.example.invalid%2F",
            p.state
        );
        assert_eq!(
            parse_redirect(&same, &p.state, iss).unwrap().expose(),
            "abc"
        );
        let denied = format!(
            "?error=access_denied&error_description=no+way&state={}",
            p.state
        );
        assert_eq!(
            parse_redirect(&denied, &p.state, iss).unwrap_err(),
            RedirectError::Denied("access_denied".into())
        );
        let own = format!("?error=nope&state={}", p.state);
        assert_eq!(
            parse_redirect(&own, &p.state, iss).unwrap_err().to_string(),
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
