//! A client of our own (RFC 7591), when the authorization server offers
//! registration: a public client (no secret asked for), for the authorization
//! code and the refresh, with this sign-in's loopback redirect. A server that
//! gives a secret anyway is answered with it as it asks.

use reqwest::Url;
use serde_json::json;
use zeroize::Zeroizing;

use super::{post, OauthError};

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

/// Never prints the secret.
impl std::fmt::Debug for ClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientId")
            .field("id", &self.id)
            .field("secret", &self.secret.as_ref().map(|_| "(set)"))
            .field("basic", &self.basic)
            .finish()
    }
}

/// The name a registration gives.
pub const CLIENT_NAME: &str = "Theseus";

/// Register a client at `endpoint` for `redirect`.
pub async fn register(
    http: &reqwest::Client,
    endpoint: &Url,
    redirect: &str,
) -> Result<ClientId, OauthError> {
    const WHAT: &str = "the registration endpoint";
    let req = http.post(endpoint.clone()).json(&json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": [redirect],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    }));
    let (_, v) = post(req, WHAT).await?;
    let id = v["client_id"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or(OauthError::Undecodable { what: WHAT })?;
    let secret = v["client_secret"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(|s| Zeroizing::new(s.to_string()));
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
