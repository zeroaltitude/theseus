//! The access token (Google design §2.2, "Token life in the daemon"): in
//! memory only, `Zeroizing`, never shown, refreshed single-flight about five
//! minutes before it expires.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use zeroize::Zeroizing;

use crate::client::Endpoints;
use crate::error::{of_token_error, GoogleError, Kind};
use crate::secret::ClientSecret;

/// How long before its expiry a token is replaced.
pub const REFRESH_MARGIN: Duration = Duration::from_secs(300);

/// A bearer token. `Debug` shows nothing of it.
#[derive(Clone)]
pub struct AccessToken(Zeroizing<String>);

impl AccessToken {
    pub fn new(s: impl Into<String>) -> Self {
        Self(Zeroizing::new(s.into()))
    }

    /// For the `Authorization` header, and nothing else.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(<hidden>)")
    }
}

struct Cached {
    token: AccessToken,
    expires: Instant,
}

/// The cache and the single flight. The client and the refresh token are the
/// caller's, read from the secrets board at each use, so a token minted a
/// moment ago works at once.
pub struct Tokens {
    http: reqwest::Client,
    endpoints: Endpoints,
    cache: Mutex<Option<Cached>>,
    /// The scopes the last refresh (or the consent) said the grant holds.
    granted: Mutex<Vec<String>>,
    flight: tokio::sync::Mutex<()>,
    requests: AtomicU64,
}

impl Tokens {
    pub fn new(http: reqwest::Client, endpoints: Endpoints) -> Self {
        Self {
            http,
            endpoints,
            cache: Mutex::new(None),
            granted: Mutex::new(Vec::new()),
            flight: tokio::sync::Mutex::new(()),
            requests: AtomicU64::new(0),
        }
    }

    /// How many times the token endpoint was asked for a refresh.
    pub fn requests(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    fn fresh(&self) -> Option<AccessToken> {
        let c = self.cache.lock().unwrap();
        c.as_ref()
            .filter(|c| c.expires > Instant::now() + REFRESH_MARGIN)
            .map(|c| c.token.clone())
    }

    /// Forget the token: the refresh token changed (a new consent, a revoke).
    pub fn forget(&self) {
        *self.cache.lock().unwrap() = None;
    }

    /// The scopes Google last said the grant holds: empty before any refresh.
    pub fn granted(&self) -> Vec<String> {
        self.granted.lock().unwrap().clone()
    }

    /// The consent's own answer, before any refresh has said.
    pub fn set_granted(&self, scopes: Vec<String>) {
        *self.granted.lock().unwrap() = scopes;
    }

    /// A token good for at least the margin: the cached one, or one fresh
    /// refresh, shared by every caller that came while it ran.
    pub async fn access(
        &self,
        client: &ClientSecret,
        refresh_token: &str,
    ) -> Result<AccessToken, GoogleError> {
        if let Some(t) = self.fresh() {
            return Ok(t);
        }
        let _one = self.flight.lock().await;
        if let Some(t) = self.fresh() {
            return Ok(t);
        }
        self.requests.fetch_add(1, Ordering::Relaxed);
        let (token, lives) = self.refresh(client, refresh_token).await?;
        *self.cache.lock().unwrap() = Some(Cached {
            token: token.clone(),
            expires: Instant::now() + lives,
        });
        Ok(token)
    }

    async fn refresh(
        &self,
        client: &ClientSecret,
        refresh_token: &str,
    ) -> Result<(AccessToken, Duration), GoogleError> {
        let form = [
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client.id.as_str()),
            ("client_secret", client.expose_secret()),
        ];
        let (status, body) = post_form(&self.http, &self.endpoints.token, &form).await?;
        if status != 200 {
            return Err(of_token_error(status, &body));
        }
        let token = body
            .get("access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| GoogleError {
                status,
                reason: "no_access_token".into(),
                message: "the token endpoint answered with no access token".into(),
                kind: Kind::Refused,
            })?;
        let secs = body
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(3600);
        if let Some(scope) = body.get("scope").and_then(Value::as_str) {
            self.set_granted(scope.split_whitespace().map(str::to_string).collect());
        }
        Ok((AccessToken::new(token), Duration::from_secs(secs)))
    }
}

/// A form POST to the token or revoke endpoint: the status and the JSON body.
pub async fn post_form(
    http: &reqwest::Client,
    url: &str,
    form: &[(&str, &str)],
) -> Result<(u16, Value), GoogleError> {
    let resp = http.post(url).form(form).send().await.map_err(|e| {
        let reason = if e.is_connect() { "connect" } else { "timeout" };
        GoogleError {
            reason: reason.into(),
            ..GoogleError::network(without_url(e))
        }
    })?;
    let status = resp.status().as_u16();
    let text = resp
        .text()
        .await
        .map_err(|e| GoogleError::network(without_url(e)))?;
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

/// A transport error without its URL, which may carry a code or a token in
/// its query.
pub fn without_url(e: reqwest::Error) -> String {
    e.without_url().to_string()
}
