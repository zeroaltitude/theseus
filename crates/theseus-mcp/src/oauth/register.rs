//! A client of our own (RFC 7591), when the authorization server offers
//! registration: a public client (no secret asked for), for the authorization
//! code and the refresh, with this sign-in's loopback redirect. A server that
//! gives a secret anyway is answered with it as it asks. A secret is held
//! where it is first read; one the scrubber could not keep from view is a bad
//! answer.

use reqwest::Url;
use serde_json::json;
use zeroize::Zeroizing;

use super::{OauthError, Reach};

/// The client a sign-in uses.
#[derive(Clone)]
pub struct ClientId {
    pub id: String,
    /// A secret the server gave at registration, if it gave one.
    pub secret: Option<Zeroizing<String>>,
    /// The secret goes in `Authorization: Basic` rather than the form.
    pub basic: bool,
}

impl ClientId {
    /// A client the settings name: an id alone.
    pub fn named(id: &str) -> Self {
        Self {
            id: id.to_string(),
            secret: None,
            basic: false,
        }
    }
}

/// Never prints the secret, nor the id a server chose.
impl std::fmt::Debug for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientId")
            .field("secret", &self.secret.as_ref().map(|_| "(set)"))
            .field("basic", &self.basic)
            .finish_non_exhaustive()
    }
}

/// The longest client id taken.
const MAX_ID: usize = 512;

/// The name a registration gives.
pub const CLIENT_NAME: &str = "Theseus";

/// Register a client at `endpoint` for `redirect`.
pub async fn register(
    reach: &Reach,
    endpoint: &Url,
    redirect: &str,
) -> Result<ClientId, OauthError> {
    const WHAT: &str = "the registration endpoint";
    let req = reach.client().post(endpoint.clone()).json(&json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    }));
    let hold = |v: &serde_json::Value| {
        if let Some(s) = v["client_secret"].as_str() {
            reach.hold("client", s);
        }
    };
    let (_, v) = reach.post(req, WHAT, hold).await?;
    let bad = OauthError::Undecodable { what: WHAT };
    let id = v["client_id"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= MAX_ID)
        .ok_or(bad.clone())?;
    let secret = match &v["client_secret"] {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) if s.is_empty() => None,
        serde_json::Value::String(s) if reach.protects(s) => Some(Zeroizing::new(s.clone())),
        _ => return Err(bad),
    };
    let basic =
        secret.is_some() && v["token_endpoint_auth_method"].as_str() == Some("client_secret_basic");
    Ok(ClientId {
        id: id.to_string(),
        secret,
        basic,
    })
}

/// The client's part of a request to the token or revocation endpoint: its id
/// in the form, and its secret in the form or in Basic, as it registered.
pub(crate) fn authorize(
    req: reqwest::RequestBuilder,
    client: &ClientId,
    form: &mut Vec<(&'static str, String)>,
) -> reqwest::RequestBuilder {
    match (&client.secret, client.basic) {
        (Some(s), true) => req.basic_auth(&client.id, Some(s.as_str())),
        (Some(s), false) => {
            form.push(("client_id", client.id.clone()));
            form.push(("client_secret", s.to_string()));
            req
        }
        (None, _) => {
            form.push(("client_id", client.id.clone()));
            req
        }
    }
}
