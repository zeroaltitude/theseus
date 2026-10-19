//! The one-time consent (Google design §2.2): an OAuth 2.0 Desktop client with
//! a loopback redirect and PKCE S256 (RFC 8252). A one-shot listener on
//! `127.0.0.1` catches the redirect, or the owner pastes the address bar; the
//! code is redeemed with the verifier, and what comes back is checked by the
//! caller (the granted scopes, the account) before anything is stored.

use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use zeroize::Zeroizing;

use crate::client::Endpoints;
use crate::error::{oauth_kind, of_token_error, GoogleError, Kind};
use crate::http1;
use crate::secret::ClientSecret;
use crate::token::{post_form, AccessToken};

/// How long the loopback listener waits for the redirect.
pub const LISTEN_FOR: Duration = Duration::from_secs(600);
/// How long the CLI waits before it asks for the paste.
pub const PASTE_AFTER: Duration = Duration::from_secs(90);

fn random_b64(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

/// A consent's PKCE pair and its `state`. The verifier stays in the daemon.
pub struct Pkce {
    verifier: Zeroizing<String>,
    pub challenge: String,
    pub state: String,
}

impl Pkce {
    pub fn new() -> Self {
        let verifier = Zeroizing::new(random_b64(48));
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

/// The consent URL. It holds no secret: the client id, the challenge, the
/// state. `prompt=consent` makes Google issue a refresh token even on a
/// re-auth; `include_granted_scopes=true` and exactly the requested scopes.
pub fn consent_url(
    ep: &Endpoints,
    client_id: &str,
    redirect: &str,
    scopes: &[String],
    pkce: &Pkce,
    login_hint: &str,
) -> String {
    let mut u = url::Url::parse(&ep.auth).expect("the auth endpoint is a URL");
    u.query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect)
        .append_pair("response_type", "code")
        .append_pair("scope", &scopes.join(" "))
        .append_pair("code_challenge", &pkce.challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &pkce.state)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true")
        .append_pair("login_hint", login_hint);
    u.to_string()
}

/// An authorization code: single-use, held in memory, never shown.
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
pub enum AuthError {
    #[error("the redirect's state is not this consent's: nothing was stored")]
    StateMismatch,
    #[error(
        "the address holds no code: paste the whole address bar of the page that failed to load"
    )]
    NoCode,
    #[error("Google did not grant it: {0}")]
    Denied(String),
    #[error("no redirect arrived within {} minutes", LISTEN_FOR.as_secs() / 60)]
    Timeout,
    #[error("the listener failed: {0}")]
    Listener(String),
}

/// The code in a redirect: the whole address (`http://127.0.0.1:41999/?code=…&state=…`),
/// or its query alone. The state must be the consent's.
pub fn parse_redirect(text: &str, state: &str) -> Result<Code, AuthError> {
    let text = text.trim();
    let query = if text.contains("://") {
        url::Url::parse(text)
            .map_err(|_| AuthError::NoCode)?
            .query()
            .unwrap_or("")
            .to_string()
    } else {
        text.trim_start_matches(['?', '/']).to_string()
    };
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    if get("state") != Some(state) {
        return Err(AuthError::StateMismatch);
    }
    if let Some(e) = get("error") {
        let words = GoogleError {
            status: 400,
            reason: e.to_string(),
            message: String::new(),
            kind: oauth_kind(e, get("error_description").unwrap_or(""), 400),
        };
        let shown = match words.kind {
            Kind::Refused => e.to_string(),
            _ => words.words(),
        };
        return Err(AuthError::Denied(shown));
    }
    match get("code") {
        Some(c) if !c.is_empty() => Ok(Code(Zeroizing::new(c.to_string()))),
        _ => Err(AuthError::NoCode),
    }
}

/// The one-shot listener on `127.0.0.1:0`.
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
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// The code from the first redirect that carries this consent's state,
    /// or why there was none. A stray request (a favicon, a probe, a wrong
    /// state) is answered and waited past.
    pub async fn wait(self, state: &str, within: Duration) -> Result<Code, AuthError> {
        let run = async {
            loop {
                let (mut stream, _) = self
                    .listener
                    .accept()
                    .await
                    .map_err(|e| AuthError::Listener(e.to_string()))?;
                let Some(req) = http1::read_request(&mut stream).await else {
                    continue;
                };
                let target = req.target.clone();
                if !target.contains("code=") && !target.contains("error=") {
                    http1::write_response(&mut stream, 404, "text/plain", &[], b"not found").await;
                    continue;
                }
                match parse_redirect(&target, state) {
                    Ok(code) => {
                        let page =
                            format!("{PAGE}Google access is granted. You can close this tab.</p>");
                        http1::write_response(&mut stream, 200, "text/html", &[], page.as_bytes())
                            .await;
                        return Ok(code);
                    }
                    Err(AuthError::StateMismatch) => {
                        let page = format!("{PAGE}That redirect is not for this sign-in.</p>");
                        http1::write_response(&mut stream, 400, "text/html", &[], page.as_bytes())
                            .await;
                    }
                    Err(e) => {
                        let page = format!("{PAGE}Not granted. You can close this tab.</p>");
                        http1::write_response(&mut stream, 200, "text/html", &[], page.as_bytes())
                            .await;
                        return Err(e);
                    }
                }
            }
        };
        tokio::time::timeout(within, run)
            .await
            .unwrap_or(Err(AuthError::Timeout))
    }
}

/// What a redeemed code gave.
pub struct Grant {
    pub refresh_token: Zeroizing<String>,
    pub access: AccessToken,
    pub expires_in: u64,
    /// The scopes Google says were granted (a granular consent may grant fewer).
    pub scopes: Vec<String>,
    /// The `id_token`'s `email`, which came straight off the TLS token endpoint.
    pub email: Option<String>,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

/// The code, redeemed with the verifier and the client secret.
pub async fn redeem(
    http: &reqwest::Client,
    ep: &Endpoints,
    client: &ClientSecret,
    code: &Code,
    verifier: &str,
    redirect: &str,
) -> Result<Grant, GoogleError> {
    let form = [
        ("grant_type", "authorization_code"),
        ("code", code.expose()),
        ("code_verifier", verifier),
        ("redirect_uri", redirect),
        ("client_id", client.id.as_str()),
        ("client_secret", client.expose_secret()),
    ];
    let (status, body) = post_form(http, &ep.token, &form).await?;
    if status != 200 {
        return Err(of_token_error(status, &body));
    }
    let text = |k: &str| body.get(k).and_then(Value::as_str);
    let refused = |why: &str| GoogleError {
        status,
        reason: "no_refresh_token".into(),
        message: why.into(),
        kind: Kind::Refused,
    };
    let refresh = text("refresh_token").ok_or_else(|| {
        refused("Google issued no refresh token: run `theseus google auth` again")
    })?;
    let access = text("access_token").ok_or_else(|| refused("Google issued no access token"))?;
    Ok(Grant {
        refresh_token: Zeroizing::new(refresh.to_string()),
        access: AccessToken::new(access),
        expires_in: body
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(3600),
        scopes: text("scope")
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect(),
        email: text("id_token").and_then(email_of_id_token),
    })
}

/// The `email` claim of an `id_token`. The token came over TLS from the token
/// endpoint itself, so its signature is not checked (§2.2).
pub fn email_of_id_token(jwt: &str) -> Option<String> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("email").and_then(Value::as_str).map(str::to_string)
}

/// Revoke a refresh (or access) token at Google.
pub async fn revoke(
    http: &reqwest::Client,
    ep: &Endpoints,
    token: &str,
) -> Result<(), GoogleError> {
    let (status, body) = post_form(http, &ep.revoke, &[("token", token)]).await?;
    match status {
        200 => Ok(()),
        // Already revoked or expired: the goal holds.
        400 if body.get("error").and_then(Value::as_str) == Some("invalid_token") => Ok(()),
        _ => Err(of_token_error(status, &body)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_state_mismatch_is_refused() {
        let p = Pkce::new();
        let ok = format!("http://127.0.0.1:4000/?code=abc&state={}", p.state);
        assert_eq!(parse_redirect(&ok, &p.state).unwrap().expose(), "abc");
        let bad = "http://127.0.0.1:4000/?code=abc&state=someone-elses";
        assert_eq!(
            parse_redirect(bad, &p.state).unwrap_err(),
            AuthError::StateMismatch
        );
        assert_eq!(
            parse_redirect("http://127.0.0.1:4000/?code=abc", &p.state).unwrap_err(),
            AuthError::StateMismatch
        );
        // The challenge is S256 of the verifier.
        let want = URL_SAFE_NO_PAD.encode(Sha256::digest(p.verifier().as_bytes()));
        assert_eq!(p.challenge, want);
    }

    #[test]
    fn a_denied_consent_says_why_and_a_paste_may_be_the_query_alone() {
        let p = Pkce::new();
        let denied = format!("http://127.0.0.1:1/?error=access_denied&state={}", p.state);
        assert!(
            matches!(parse_redirect(&denied, &p.state), Err(AuthError::Denied(e)) if e == "access_denied")
        );
        let q = format!("?state={}&code=4%2Fabc", p.state);
        assert_eq!(parse_redirect(&q, &p.state).unwrap().expose(), "4/abc");
        assert_eq!(
            parse_redirect("hello", &p.state).unwrap_err(),
            AuthError::StateMismatch
        );
    }

    #[test]
    fn the_consent_url_holds_exactly_the_requested_scopes_and_no_secret() {
        let ep = Endpoints::production();
        let p = Pkce::new();
        let scopes = crate::scopes::requested(&[crate::Service::Gmail]);
        let u = consent_url(
            &ep,
            "id-1",
            "http://127.0.0.1:4000/",
            &scopes,
            &p,
            "owner@example.test",
        );
        let url = url::Url::parse(&u).unwrap();
        let q: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], scopes.join(" "));
        for (k, v) in [
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("include_granted_scopes", "true"),
            ("code_challenge_method", "S256"),
            ("login_hint", "owner@example.test"),
            ("response_type", "code"),
        ] {
            assert_eq!(q[k], v, "{k}");
        }
        assert!(!u.contains(p.verifier()));
    }

    #[test]
    fn an_id_token_names_its_email() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"email":"owner@example.test","sub":"1"}"#);
        assert_eq!(
            email_of_id_token(&format!("x.{payload}.y")).as_deref(),
            Some("owner@example.test")
        );
        assert_eq!(email_of_id_token("nonsense"), None);
    }
}
