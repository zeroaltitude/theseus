//! The token endpoint: the code for tokens (with the verifier and the
//! resource), the refresh (a new refresh token replaces the old one when the
//! answer gives one), and the revoke (RFC 7009). A refresh the server refuses
//! for good (`invalid_grant`, `invalid_client`, `unauthorized_client`) is told
//! apart from one that may pass: only the first means "sign in again".

use reqwest::Url;
use serde_json::Value;
use zeroize::{Zeroize, Zeroizing};

use super::register::{authorize, ClientId};
use super::request::Code;
use super::{post, OauthError};

/// What the token endpoint gave.
pub struct Tokens {
    pub access: Zeroizing<String>,
    pub refresh: Option<Zeroizing<String>>,
    /// Seconds until the access token runs out, when the server says.
    pub expires_in: Option<u64>,
    pub scope: Option<String>,
}

/// Never prints a token.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("refresh", &self.refresh.as_ref().map(|_| "(set)"))
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

const WHAT: &str = "the token endpoint";

/// A form's values wiped once its body is built.
fn forget(form: &mut [(&'static str, String)]) {
    for (_, v) in form.iter_mut() {
        v.zeroize();
    }
}

/// The tokens in an answer of the token endpoint. Only a Bearer token is
/// taken (a server that names no type is taken as giving one).
fn tokens_of(v: &Value) -> Result<Tokens, OauthError> {
    let access = v["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(OauthError::Undecodable { what: WHAT })?;
    if let Some(kind) = v["token_type"].as_str() {
        if !kind.eq_ignore_ascii_case("bearer") {
            return Err(OauthError::Refused(
                "the token endpoint gave a token that is not a Bearer token: refused".into(),
            ));
        }
    }
    let expires_in = match &v["expires_in"] {
        Value::Number(n) => n.as_u64(),
        // Some servers send a number as a string.
        Value::String(s) => s.parse().ok(),
        _ => None,
    };
    Ok(Tokens {
        access: Zeroizing::new(access.to_string()),
        refresh: v["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| Zeroizing::new(s.to_string())),
        expires_in,
        scope: v["scope"].as_str().map(String::from),
    })
}

async fn token_request(
    http: &reqwest::Client,
    token_endpoint: &Url,
    client: &ClientId,
    mut form: Vec<(&'static str, String)>,
) -> Result<Tokens, OauthError> {
    let req = authorize(http.post(token_endpoint.clone()), client, &mut form).form(&form);
    forget(&mut form);
    let (_, v) = post(req, WHAT).await?;
    tokens_of(&v)
}

/// The code, with the verifier, for tokens.
pub async fn redeem(
    http: &reqwest::Client,
    token_endpoint: &Url,
    client: &ClientId,
    code: &Code,
    verifier: &str,
    redirect: &str,
    resource: &str,
) -> Result<Tokens, OauthError> {
    let form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("code", code.expose().to_string()),
        ("redirect_uri", redirect.to_string()),
        ("code_verifier", verifier.to_string()),
        ("resource", resource.to_string()),
    ];
    token_request(http, token_endpoint, client, form).await
}

/// How a refresh went.
#[derive(Debug)]
pub enum Renewal {
    Renewed(Tokens),
    /// Refused for good: the sign-in is over. The words say why.
    ForGood(String),
    /// It may pass: no answer, a server error, a throttle.
    Passing(OauthError),
}

/// The codes that end a sign-in.
const FOR_GOOD: [&str; 3] = ["invalid_grant", "invalid_client", "unauthorized_client"];

/// A refresh with `refresh`.
pub async fn refresh(
    http: &reqwest::Client,
    token_endpoint: &Url,
    client: &ClientId,
    refresh: &str,
    resource: &str,
) -> Renewal {
    let form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh.to_string()),
        ("resource", resource.to_string()),
    ];
    match token_request(http, token_endpoint, client, form).await {
        Ok(t) => Renewal::Renewed(t),
        Err(e @ OauthError::Status { status, code, .. }) => {
            let for_good = code.is_some_and(|c| FOR_GOOD.contains(&c))
                || (code.is_none() && (status == 400 || status == 401));
            match for_good {
                true => Renewal::ForGood(e.to_string()),
                false => Renewal::Passing(e),
            }
        }
        Err(e @ OauthError::Refused(_)) => Renewal::ForGood(e.to_string()),
        Err(e) => Renewal::Passing(e),
    }
}

/// Revoke `token` at `endpoint` (RFC 7009), with its type hint.
pub async fn revoke(
    http: &reqwest::Client,
    endpoint: &Url,
    client: &ClientId,
    token: &str,
    hint: &'static str,
) -> Result<(), OauthError> {
    let mut form = vec![
        ("token", token.to_string()),
        ("token_type_hint", hint.to_string()),
    ];
    let req = authorize(http.post(endpoint.clone()), client, &mut form).form(&form);
    forget(&mut form);
    let resp = req.send().await.map_err(|_| OauthError::Unreachable {
        what: "the revocation endpoint",
        why: "the request failed".into(),
    })?;
    match resp.status().as_u16() {
        200..=299 => Ok(()),
        status => Err(OauthError::Status {
            what: "the revocation endpoint",
            status,
            code: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn only_a_bearer_token_is_taken() {
        let t = tokens_of(&json!({"access_token": "a-1", "token_type": "bearer",
            "expires_in": "60", "refresh_token": "r-1"}))
        .unwrap();
        assert_eq!((t.access.as_str(), t.expires_in), ("a-1", Some(60)));
        assert_eq!(t.refresh.as_deref().map(String::as_str), Some("r-1"));
        assert!(!format!("{t:?}").contains("a-1"));
        assert!(!format!("{t:?}").contains("r-1"));
        let e = tokens_of(&json!({"access_token": "a", "token_type": "DPoP"})).unwrap_err();
        assert!(e.to_string().contains("not a Bearer"), "{e}");
        assert!(tokens_of(&json!({"token_type": "Bearer"})).is_err());
        assert!(tokens_of(&json!({"access_token": "a"})).is_ok());
    }
}
