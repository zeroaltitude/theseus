//! Signing in to a remote MCP server (issue 55): the MCP specification's
//! authorization, which is OAuth 2.1's authorization code with PKCE S256 over
//! protected-resource metadata (RFC 9728), authorization-server metadata
//! (RFC 8414, with OpenID discovery as its fallback), dynamic client
//! registration (RFC 7591) and resource indicators (RFC 8707).
//!
//! The parts apart: [`discover`] (the 401's challenge, the resource's
//! metadata, the authorization server's), [`register`] (a client of our own,
//! when the server offers registration), [`request`] (PKCE, `state`, the
//! address the owner opens, the loopback listener and the redirect), and
//! [`exchange`] (the code for tokens, the refresh, the revoke). Nothing here
//! keeps a value, and no error or log line holds a token, a code, a verifier,
//! a client secret or a response's body: a refusal is said by its status and
//! a standard OAuth code alone.
//!
//! What comes from outside is checked before it is used: every endpoint is
//! `https`, or `http` on a loopback host; no redirect is followed off its own
//! origin; the authorization server's `issuer` is the one asked; the
//! resource's metadata names this server.

pub mod discover;
pub mod exchange;
pub mod fake;
pub mod register;
pub mod request;

use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

/// The OAuth error codes (RFC 6749 §5.2 and §4.1.2.1, RFC 6750 §3.1): the only
/// words of a refusal's body that are ever repeated.
pub const CODES: [&str; 11] = [
    "invalid_request",
    "invalid_token",
    "insufficient_scope",
    "invalid_client",
    "invalid_grant",
    "unauthorized_client",
    "unsupported_grant_type",
    "invalid_scope",
    "access_denied",
    "server_error",
    "temporarily_unavailable",
];

/// `code` when it is one of [`CODES`].
pub fn standard_code(code: &str) -> Option<&'static str> {
    CODES.iter().find(|c| **c == code).copied()
}

/// Why a step of a sign-in failed, in the daemon's words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OauthError {
    /// A check of what came from outside refused it.
    #[error("{0}")]
    Refused(String),
    /// The other side answered with a status that is not success.
    #[error("{what} answered HTTP {status}{}", code.map(|c| format!(" ({c})")).unwrap_or_default())]
    Status {
        what: &'static str,
        status: u16,
        code: Option<&'static str>,
    },
    #[error("{what} could not be reached ({why})")]
    Unreachable { what: &'static str, why: String },
    #[error("{what} answered with something that is not its JSON")]
    Undecodable { what: &'static str },
}

/// The client every sign-in call goes through: it follows a redirect only
/// on its own origin (a 307 would carry a code or a refresh token to wherever
/// it pointed), at most three, and never to another scheme. A loopback
/// server's calls skip any proxy from the environment, as the MCP client's do.
pub fn http(loopback: bool) -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let same = attempt
            .previous()
            .last()
            .is_some_and(|p| same_origin(p, attempt.url()));
        if attempt.previous().len() > 3 || !same {
            attempt.stop()
        } else {
            attempt.follow()
        }
    });
    let mut b = reqwest::Client::builder()
        .redirect(policy)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30));
    if loopback {
        b = b.no_proxy();
    }
    b.build().unwrap_or_default()
}

/// Whether two addresses have one scheme, host and port.
pub fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Whether `u`'s host is this machine.
pub fn is_loopback(u: &Url) -> bool {
    u.host_str().is_some_and(|h| {
        let h = h.trim_start_matches('[').trim_end_matches(']');
        h.eq_ignore_ascii_case("localhost")
            || h.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}

/// `text` as an endpoint a sign-in may use: `https`, or `http` on a loopback
/// host, with no name or key before an `@` and no fragment.
pub fn endpoint(text: &str, what: &str) -> Result<Url, OauthError> {
    let u = Url::parse(text)
        .map_err(|_| OauthError::Refused(format!("the sign-in's {what} is not an address")))?;
    let scheme_ok = match u.scheme() {
        "https" => true,
        "http" => is_loopback(&u),
        _ => false,
    };
    if !scheme_ok {
        return Err(OauthError::Refused(format!(
            "the sign-in's {what} is not https: refused"
        )));
    }
    if !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        return Err(OauthError::Refused(format!(
            "the sign-in's {what} carries a name, a key or a fragment: refused"
        )));
    }
    Ok(u)
}

/// A send's failure, without its address (an address can carry a query).
fn unreachable(what: &'static str, e: reqwest::Error) -> OauthError {
    let why = if e.is_timeout() {
        "no answer in time"
    } else if e.is_connect() {
        "no connection"
    } else if e.is_redirect() {
        "too many redirects"
    } else {
        "the request failed"
    };
    OauthError::Unreachable {
        what,
        why: why.into(),
    }
}

/// A non-success answer's status and standard code; a redirect that was not
/// followed is a refusal.
fn refusal(what: &'static str, status: reqwest::StatusCode, body: &[u8]) -> OauthError {
    if status.is_redirection() {
        return OauthError::Refused(format!(
            "{what} answered with a redirect elsewhere: refused"
        ));
    }
    let code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().and_then(standard_code));
    OauthError::Status {
        what,
        status: status.as_u16(),
        code,
    }
}

/// The most of an answer read.
const MAX_BODY: usize = 256 * 1024;

async fn body_of(what: &'static str, resp: reqwest::Response) -> Result<Vec<u8>, OauthError> {
    let mut resp = resp;
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| unreachable(what, e))? {
        out.extend_from_slice(&chunk);
        if out.len() > MAX_BODY {
            return Err(OauthError::Undecodable { what });
        }
    }
    Ok(out)
}

/// GET a JSON document. `Ok(None)`: the server has none there (404 or 410).
pub async fn get_json(
    http: &reqwest::Client,
    url: &Url,
    what: &'static str,
) -> Result<Option<Value>, OauthError> {
    let resp = http
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| unreachable(what, e))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
        return Ok(None);
    }
    let body = body_of(what, resp).await?;
    if !status.is_success() {
        return Err(refusal(what, status, &body));
    }
    match serde_json::from_slice::<Value>(&body) {
        Ok(v) if v.is_object() => Ok(Some(v)),
        _ => Err(OauthError::Undecodable { what }),
    }
}

/// POST a request body and read a JSON object back: `(status, object)`, or
/// the refusal of a status that is not success.
pub(crate) async fn post(
    req: reqwest::RequestBuilder,
    what: &'static str,
) -> Result<(u16, Value), OauthError> {
    let resp = req
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|e| unreachable(what, e))?;
    let status = resp.status();
    let body = body_of(what, resp).await?;
    if !status.is_success() {
        return Err(refusal(what, status, &body));
    }
    match serde_json::from_slice::<Value>(&body) {
        Ok(v) if v.is_object() => Ok((status.as_u16(), v)),
        _ => Err(OauthError::Undecodable { what }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_https_or_a_loopback_http_endpoint_is_used() {
        assert!(endpoint("https://as.example.invalid/token", "token endpoint").is_ok());
        assert!(endpoint("http://127.0.0.1:9/token", "token endpoint").is_ok());
        assert!(endpoint("http://localhost:9/token", "token endpoint").is_ok());
        for bad in [
            "http://as.example.invalid/token",
            "ftp://as.example.invalid/token",
            "https://user:key@as.example.invalid/token",
            "https://as.example.invalid/token#x",
            "not an address",
        ] {
            let e = endpoint(bad, "token endpoint").unwrap_err();
            assert!(matches!(e, OauthError::Refused(_)), "{bad}: {e}");
        }
        assert_eq!(
            endpoint("http://as.example.invalid/t", "token endpoint")
                .unwrap_err()
                .to_string(),
            "the sign-in's token endpoint is not https: refused"
        );
    }

    #[test]
    fn a_refusal_says_its_status_and_a_standard_code_only() {
        let e = refusal(
            "the token endpoint",
            reqwest::StatusCode::BAD_REQUEST,
            br#"{"error":"invalid_grant","error_description":"the harbour portal says no"}"#,
        );
        assert_eq!(
            e.to_string(),
            "the token endpoint answered HTTP 400 (invalid_grant)"
        );
        let e = refusal(
            "the token endpoint",
            reqwest::StatusCode::BAD_REQUEST,
            br#"{"error":"a code of its own"}"#,
        );
        assert_eq!(e.to_string(), "the token endpoint answered HTTP 400");
        let e = refusal("the token endpoint", reqwest::StatusCode::FOUND, b"");
        assert!(e.to_string().contains("redirect elsewhere"), "{e}");
    }
}
