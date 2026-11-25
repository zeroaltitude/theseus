//! The token endpoint: the code for tokens (with the verifier and the
//! resource), the refresh (a new refresh token replaces the old one when the
//! answer gives one), and the revoke (RFC 7009). A refresh the server refuses
//! for good (`invalid_grant`, `invalid_client`, `unauthorized_client`) is told
//! apart from one that may pass: only the first means "sign in again".
//!
//! Each token in an answer, refused or not, is held where it is first read,
//! before anything checks it; a token shorter than [`MIN_SECRET`] is a bad
//! answer, and an `expires_in` is a whole number of seconds, read as at most
//! [`MAX_EXPIRES_SECS`].

use reqwest::Url;
use serde_json::Value;
use zeroize::{Zeroize, Zeroizing};

use super::register::{authorize, ClientId};
use super::request::Code;
use super::{OauthError, Reach, MAX_EXPIRES_SECS, MIN_SECRET};

/// What the token endpoint gave.
pub struct Tokens {
    pub access: Zeroizing<String>,
    pub refresh: Option<Zeroizing<String>>,
    /// Seconds until the access token runs out, when the server says.
    pub expires_in: Option<u64>,
    pub scope: Option<String>,
}

/// Never prints a token, nor the scope the server chose.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("refresh", &self.refresh.as_ref().map(|_| "(set)"))
            .field("expires_in", &self.expires_in)
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

/// Every token an answer carries, held before anything reads it further.
fn hold_tokens(reach: &Reach, v: &Value) {
    for (key, what) in [("access_token", "access"), ("refresh_token", "refresh")] {
        if let Some(t) = v[key].as_str() {
            reach.hold(what, t);
        }
    }
}

/// The tokens in an answer of the token endpoint. Only a Bearer token is
/// taken (a server that names no type is taken as giving one); a token too
/// short to keep from view, or an `expires_in` that is not a whole number of
/// seconds, is a bad answer.
fn tokens_of(v: &Value) -> Result<Tokens, OauthError> {
    let bad = OauthError::Undecodable { what: WHAT };
    let access = v["access_token"]
        .as_str()
        .filter(|s| s.len() >= MIN_SECRET)
        .ok_or(bad.clone())?;
    let refresh = match &v["refresh_token"] {
        Value::Null => None,
        Value::String(s) if s.is_empty() => None,
        Value::String(s) if s.len() >= MIN_SECRET => Some(Zeroizing::new(s.clone())),
        _ => return Err(bad),
    };
    if let Some(kind) = v["token_type"].as_str() {
        if !kind.eq_ignore_ascii_case("bearer") {
            return Err(OauthError::Refused(
                "the token endpoint gave a token that is not a Bearer token: refused".into(),
            ));
        }
    }
    let expires_in = match &v["expires_in"] {
        Value::Null => None,
        Value::Number(n) => Some(n.as_u64().ok_or(bad.clone())?),
        // Some servers send a number as a string.
        Value::String(s) => Some(s.trim().parse::<u64>().map_err(|_| bad.clone())?),
        _ => return Err(bad),
    }
    .map(|s| s.min(MAX_EXPIRES_SECS));
    Ok(Tokens {
        access: Zeroizing::new(access.to_string()),
        refresh,
        expires_in,
        scope: v["scope"].as_str().map(String::from),
    })
}

async fn token_request(
    reach: &Reach,
    token_endpoint: &Url,
    client: &ClientId,
    mut form: Vec<(&'static str, String)>,
) -> Result<Tokens, OauthError> {
    let req = authorize(
        reach.client().post(token_endpoint.clone()),
        client,
        &mut form,
    )
    .form(&form);
    forget(&mut form);
    let (_, v) = reach.post(req, WHAT, |v| hold_tokens(reach, v)).await?;
    tokens_of(&v)
}

/// The code, with the verifier, for tokens.
pub async fn redeem(
    reach: &Reach,
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
    token_request(reach, token_endpoint, client, form).await
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

/// Whether a refused refresh ends the sign-in: one of [`FOR_GOOD`]'s codes,
/// or a 400 or 401 that names no standard code (a server that says no other
/// way is taken at its word). Any other status may pass.
fn for_good(status: u16, code: Option<&str>) -> bool {
    code.is_some_and(|c| FOR_GOOD.contains(&c))
        || (code.is_none() && (status == 400 || status == 401))
}

/// A refresh with `refresh`.
pub async fn refresh(
    reach: &Reach,
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
    match token_request(reach, token_endpoint, client, form).await {
        Ok(t) => Renewal::Renewed(t),
        Err(e @ OauthError::Status { status, code, .. }) => match for_good(status, code) {
            true => Renewal::ForGood(e.to_string()),
            false => Renewal::Passing(e),
        },
        Err(e @ OauthError::Refused(_)) => Renewal::ForGood(e.to_string()),
        Err(e) => Renewal::Passing(e),
    }
}

/// Revoke `token` at `endpoint` (RFC 7009), with its type hint.
pub async fn revoke(
    reach: &Reach,
    endpoint: &Url,
    client: &ClientId,
    token: &str,
    hint: &'static str,
) -> Result<(), OauthError> {
    let mut form = vec![
        ("token", token.to_string()),
        ("token_type_hint", hint.to_string()),
    ];
    let req = authorize(reach.client().post(endpoint.clone()), client, &mut form).form(&form);
    forget(&mut form);
    match reach.send(req, "the revocation endpoint").await? {
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

    /// The readings of a refused refresh: for good, or one that may pass.
    #[test]
    fn a_refused_refresh_is_for_good_only_when_it_says_so() {
        assert!(for_good(400, Some("invalid_grant")));
        assert!(for_good(401, Some("invalid_client")));
        assert!(for_good(400, Some("unauthorized_client")));
        assert!(for_good(400, None), "a 400 with no standard code");
        assert!(for_good(401, None));
        assert!(!for_good(400, Some("temporarily_unavailable")));
        assert!(!for_good(503, None));
        assert!(!for_good(429, None));
        assert!(!for_good(500, Some("server_error")));
    }

    /// A Bearer token long enough to keep from view, and an expiry that is a
    /// whole number of seconds, at most a year; anything else is a bad answer.
    #[test]
    fn only_a_bearer_token_is_taken() {
        let t = tokens_of(&json!({"access_token": "access-1", "token_type": "bearer",
            "expires_in": "60", "refresh_token": "refresh-1", "scope": "secret-scope"}))
        .unwrap();
        assert_eq!((t.access.as_str(), t.expires_in), ("access-1", Some(60)));
        assert_eq!(t.refresh.as_deref().map(String::as_str), Some("refresh-1"));
        for v in ["access-1", "refresh-1", "secret-scope"] {
            assert!(!format!("{t:?}").contains(v), "{v}");
        }
        let e = tokens_of(&json!({"access_token": "access-1", "token_type": "DPoP"})).unwrap_err();
        assert!(e.to_string().contains("not a Bearer"), "{e}");
        assert!(tokens_of(&json!({"token_type": "Bearer"})).is_err());
        assert!(tokens_of(&json!({"access_token": "access-1"})).is_ok());
        // Too short to keep from view.
        assert!(tokens_of(&json!({"access_token": "a"})).is_err());
        assert!(tokens_of(&json!({"access_token": "access-1", "refresh_token": "r"})).is_err());
        assert!(tokens_of(&json!({"access_token": "access-1", "refresh_token": 7})).is_err());
        // The expiry: bounded, and a bad one is a bad answer, not "none said".
        let huge = tokens_of(&json!({"access_token": "access-1", "expires_in": u64::MAX})).unwrap();
        assert_eq!(huge.expires_in, Some(MAX_EXPIRES_SECS));
        for bad in [json!(-5), json!(1.5), json!("soon"), json!([1])] {
            assert!(
                tokens_of(&json!({"access_token": "access-1", "expires_in": bad})).is_err(),
                "{bad}"
            );
        }
        let none = tokens_of(&json!({"access_token": "access-1", "expires_in": null})).unwrap();
        assert_eq!(none.expires_in, None);
    }
}
