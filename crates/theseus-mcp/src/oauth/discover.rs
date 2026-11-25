//! Discovery: from a server's 401 to the authorization server it trusts.
//!
//! 1. The 401's `WWW-Authenticate: Bearer resource_metadata="…"` (RFC 9728
//!    §5.1), with its `scope` when it gives one ([`challenge`]).
//! 2. The resource's metadata: at the header's address; else the well-known
//!    addresses, the server's path inserted first, then the root
//!    ([`resource_metadata_urls`]). Its `resource` must be this server: the
//!    same origin, and a path the server's own path starts with.
//! 3. The authorization servers it names, in order, the first
//!    [`MAX_ISSUERS`] of them; with no metadata at all, the server's own
//!    origin (the older revision of the specification).
//! 4. Each one's metadata (RFC 8414 §3, then OpenID discovery), whose
//!    `issuer` must be the one asked, whose endpoints are each checked
//!    ([`Reach::endpoint`]), and which, when it lists PKCE methods, lists S256.
//!    Its endpoints are on the issuer's own origin; one elsewhere is taken only
//!    when the metadata says its redirect names its issuer (RFC 9207), and
//!    then the redirect must name it and the owner is shown every host.
//!
//! Every address a server names is reached by the [`Reach`]'s rule. No type
//! here prints what a server chose in its `Debug`.

use reqwest::Url;
use serde_json::Value;

use super::{same_origin, OauthError, Reach, MAX_ISSUERS};

/// What a 401's Bearer challenge says.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

/// What a server chose is not printed.
impl std::fmt::Debug for Challenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Challenge(<a server's words>)")
    }
}

/// The Bearer challenge of a `WWW-Authenticate` value, among any others it
/// holds (`Basic realm="x", Bearer resource_metadata="…"`). None without one.
pub fn challenge(header: &str) -> Option<Challenge> {
    let mut found: Option<Challenge> = None;
    let mut in_bearer = false;
    let mut rest = header.trim();
    while !rest.is_empty() {
        rest = rest.trim_start_matches([',', ' ', '\t']);
        let end = rest
            .find(|c: char| c == '=' || c == ',' || c.is_whitespace())
            .unwrap_or(rest.len());
        let token = &rest[..end];
        let after = rest[end..].trim_start_matches([' ', '\t']);
        if let Some(value_part) = after.strip_prefix('=') {
            // A parameter of the scheme before it.
            let value_part = value_part.trim_start();
            let (value, left) = match value_part.strip_prefix('"') {
                Some(q) => quoted(q),
                None => {
                    let e = value_part
                        .find([',', ' ', '\t'])
                        .unwrap_or(value_part.len());
                    (value_part[..e].to_string(), &value_part[e..])
                }
            };
            if in_bearer {
                let c = found.get_or_insert_with(Challenge::default);
                match token.to_ascii_lowercase().as_str() {
                    "resource_metadata" => c.resource_metadata = Some(value),
                    "scope" => c.scope = Some(value),
                    "error" => c.error = Some(value),
                    _ => {}
                }
            }
            rest = left;
        } else if token.is_empty() {
            break;
        } else {
            // A scheme's name.
            in_bearer = token.eq_ignore_ascii_case("bearer");
            if in_bearer {
                found.get_or_insert_with(Challenge::default);
            }
            rest = after;
        }
    }
    found
}

/// A quoted string's value, and what follows its closing quote.
fn quoted(s: &str) -> (String, &str) {
    let mut out = String::new();
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some((_, n)) = chars.next() {
                    out.push(n);
                }
            }
            '"' => return (out, &s[i + 1..]),
            c => out.push(c),
        }
    }
    (out, "")
}

/// The well-known addresses of `server`'s resource metadata, in the order
/// tried: the server's path inserted after the well-known name, then the root.
pub fn resource_metadata_urls(server: &Url) -> Vec<Url> {
    well_known(server, "oauth-protected-resource")
}

/// `<origin>/.well-known/<name><path>`, then `<origin>/.well-known/<name>`.
fn well_known(at: &Url, name: &str) -> Vec<Url> {
    let path = at.path().trim_end_matches('/');
    let mut out = Vec::new();
    let mut base = at.clone();
    base.set_query(None);
    base.set_fragment(None);
    if !path.is_empty() {
        let mut u = base.clone();
        u.set_path(&format!("/.well-known/{name}{path}"));
        out.push(u);
    }
    let mut u = base;
    u.set_path(&format!("/.well-known/{name}"));
    out.push(u);
    out
}

/// A protected resource's metadata, checked.
#[derive(Clone, PartialEq, Eq)]
pub struct Resource {
    /// What the server calls itself: the `resource` sent with each request.
    pub resource: String,
    pub authorization_servers: Vec<String>,
}

impl std::fmt::Debug for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Resource(<a server's words>)")
    }
}

/// Whether `named`, a resource's identifier, is `server`: the same origin,
/// and a path `server`'s own path begins with (the server's URL itself, its
/// origin, or a path above it).
pub fn names_this_server(named: &str, server: &Url) -> bool {
    let Ok(n) = Url::parse(named) else {
        return false;
    };
    if !same_origin(&n, server) || n.fragment().is_some() {
        return false;
    }
    let np = n.path().trim_end_matches('/');
    let sp = server.path().trim_end_matches('/');
    np.is_empty() || sp == np || sp.starts_with(&format!("{np}/"))
}

/// The resource's metadata, from the challenge's address or the well-known
/// ones. `Ok(None)`: the server publishes none.
pub async fn protected_resource(
    reach: &Reach,
    server: &Url,
    challenge: Option<&Challenge>,
) -> Result<Option<Resource>, OauthError> {
    const WHAT: &str = "the server's resource metadata";
    let doc = match challenge.and_then(|c| c.resource_metadata.as_deref()) {
        Some(at) => {
            let url = reach.endpoint(at, "resource metadata address")?;
            Some(
                reach
                    .get_json(&url, WHAT)
                    .await?
                    .ok_or(OauthError::Status {
                        what: WHAT,
                        status: 404,
                        code: None,
                    })?,
            )
        }
        None => {
            let mut doc = None;
            for url in resource_metadata_urls(server) {
                // A fallback that fails is passed over for the next.
                if let Ok(Some(v)) = reach.get_json(&url, WHAT).await {
                    doc = Some(v);
                    break;
                }
            }
            doc
        }
    };
    let Some(doc) = doc else { return Ok(None) };
    let resource = match doc["resource"].as_str() {
        Some(r) if names_this_server(r, server) => r.to_string(),
        Some(_) => {
            return Err(OauthError::Refused(
                "the server's metadata names another resource: refused".into(),
            ))
        }
        // Required by RFC 9728; a server that leaves it out is taken as
        // naming itself.
        None => server.to_string(),
    };
    let authorization_servers: Vec<String> = doc["authorization_servers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .take(MAX_ISSUERS)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if authorization_servers.is_empty() {
        return Err(OauthError::Refused(
            "the server's metadata names no authorization server: nothing to sign in at".into(),
        ));
    }
    Ok(Some(Resource {
        resource,
        authorization_servers,
    }))
}

/// An authorization server's metadata, checked.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthServer {
    pub issuer: String,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub registration_endpoint: Option<Url>,
    pub revocation_endpoint: Option<Url>,
    /// Its metadata says its redirect names its issuer (`iss`, RFC 9207).
    pub iss_in_redirect: bool,
    /// The origins of its endpoints that are not the issuer's own, in order:
    /// the owner is shown each, and the redirect must name the issuer.
    pub elsewhere: Vec<String>,
}

impl std::fmt::Debug for AuthServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthServer(<a server's words>)")
    }
}

/// Where an issuer's metadata is looked for, in order: RFC 8414's address
/// (the issuer's path after the well-known name), then OpenID discovery's two
/// (the path after the name, then the name after the path).
pub fn metadata_urls(issuer: &Url) -> Vec<Url> {
    let path = issuer.path().trim_end_matches('/');
    let mut base = issuer.clone();
    base.set_query(None);
    base.set_fragment(None);
    let at = |p: String| {
        let mut u = base.clone();
        u.set_path(&p);
        u
    };
    let mut out = vec![at(format!("/.well-known/oauth-authorization-server{path}"))];
    out.push(at(format!("/.well-known/openid-configuration{path}")));
    if !path.is_empty() {
        out.push(at(format!("{path}/.well-known/openid-configuration")));
    }
    out
}

/// Two issuers are one when they are equal but for one trailing `/`.
fn same_issuer(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// The metadata of the authorization server `issuer`, from the first address
/// of [`metadata_urls`] that has it.
pub async fn authorization_server(reach: &Reach, issuer: &str) -> Result<AuthServer, OauthError> {
    const WHAT: &str = "the authorization server's metadata";
    let at = reach.endpoint(issuer, "authorization server")?;
    let (mut doc, mut failed) = (None, None);
    for url in metadata_urls(&at) {
        match reach.get_json(&url, WHAT).await {
            Ok(Some(v)) => {
                doc = Some(v);
                break;
            }
            Ok(None) => {}
            // The next address may have it.
            Err(e) => failed = Some(e),
        }
    }
    let Some(doc) = doc else {
        return Err(failed.unwrap_or_else(|| {
            OauthError::Refused(
                "the authorization server publishes no metadata: nothing to sign in at".into(),
            )
        }));
    };
    if !doc["issuer"]
        .as_str()
        .is_some_and(|i| same_issuer(i, issuer))
    {
        return Err(OauthError::Refused(
            "the authorization server's metadata names another issuer: refused".into(),
        ));
    }
    if let Some(methods) = doc["code_challenge_methods_supported"].as_array() {
        if !methods.iter().any(|m| m.as_str() == Some("S256")) {
            return Err(OauthError::Refused(
                "the authorization server does not offer PKCE S256: refused".into(),
            ));
        }
    }
    let required = |key: &str, what: &str| match doc[key].as_str() {
        Some(u) => reach.endpoint(u, what),
        None => Err(OauthError::Refused(format!(
            "the authorization server's metadata has no {what}"
        ))),
    };
    let optional = |key: &str, what: &str| {
        doc[key]
            .as_str()
            .map(|u| reach.endpoint(u, what))
            .transpose()
    };
    let authorization_endpoint = required("authorization_endpoint", "authorization endpoint")?;
    let token_endpoint = required("token_endpoint", "token endpoint")?;
    let registration_endpoint = optional("registration_endpoint", "registration endpoint")?;
    let revocation_endpoint = optional("revocation_endpoint", "revocation endpoint")?;
    let iss_in_redirect =
        doc["authorization_response_iss_parameter_supported"] == Value::Bool(true);
    let mut elsewhere: Vec<String> = Vec::new();
    for u in [Some(&authorization_endpoint), Some(&token_endpoint)]
        .into_iter()
        .chain([registration_endpoint.as_ref(), revocation_endpoint.as_ref()])
        .flatten()
    {
        let origin = u.origin().ascii_serialization();
        if !same_origin(u, &at) && !elsewhere.contains(&origin) {
            elsewhere.push(origin);
        }
    }
    // A mix-up: a metadata document that names another server's endpoints
    // could have the code, the verifier and the client's secret sent where
    // it chose. Taken only when the redirect names its issuer, so a code
    // from another authorization server is refused before it is sent.
    if !elsewhere.is_empty() && !iss_in_redirect {
        return Err(OauthError::Refused(
            "the authorization server's endpoints are not on its own address, and it does not \
             say its redirect names it: refused"
                .into(),
        ));
    }
    Ok(AuthServer {
        issuer: issuer.to_string(),
        authorization_endpoint,
        token_endpoint,
        registration_endpoint,
        revocation_endpoint,
        iss_in_redirect,
        elsewhere,
    })
}

/// Everything a sign-in needs from discovery.
#[derive(Clone, PartialEq, Eq)]
pub struct Discovered {
    /// The resource to name in the request and the exchange (RFC 8707).
    pub resource: String,
    pub server: AuthServer,
    /// The scope the challenge asked for, when it named one.
    pub scope: Option<String>,
}

impl std::fmt::Debug for Discovered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Discovered(<a server's words>)")
    }
}

/// From a server's address and its 401 challenge to its authorization
/// server: each one the resource names is tried in order, and the first whose
/// metadata passes is taken.
pub async fn discover(
    reach: &Reach,
    server: &Url,
    challenge: Option<&Challenge>,
) -> Result<Discovered, OauthError> {
    let scope = challenge.and_then(|c| c.scope.clone());
    let Some(r) = protected_resource(reach, server, challenge).await? else {
        // No resource metadata: the server's own origin is its authorization
        // server (the specification's revision of 2025-03-26).
        let origin = server.origin().ascii_serialization();
        let as_ = authorization_server(reach, &origin).await?;
        return Ok(Discovered {
            resource: server.to_string(),
            server: as_,
            scope,
        });
    };
    let mut last = None;
    for issuer in &r.authorization_servers {
        match authorization_server(reach, issuer).await {
            Ok(as_) => {
                return Ok(Discovered {
                    resource: r.resource,
                    server: as_,
                    scope,
                })
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or(OauthError::Refused(
        "the server names no authorization server".into(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bearer_challenge_is_read_among_others() {
        let c = challenge(
            r#"Basic realm="x", Bearer error="invalid_token", resource_metadata="https://mcp.example.invalid/.well-known/oauth-protected-resource", scope="read write""#,
        )
        .unwrap();
        assert_eq!(
            c.resource_metadata.as_deref(),
            Some("https://mcp.example.invalid/.well-known/oauth-protected-resource")
        );
        assert_eq!(c.scope.as_deref(), Some("read write"));
        assert_eq!(c.error.as_deref(), Some("invalid_token"));
        assert_eq!(challenge("Bearer").unwrap(), Challenge::default());
        assert_eq!(
            challenge(r#"bearer resource_metadata=https://h.invalid/m"#)
                .unwrap()
                .resource_metadata
                .as_deref(),
            Some("https://h.invalid/m")
        );
        assert_eq!(challenge(r#"Basic realm="x""#), None);
        assert_eq!(challenge(""), None);
    }

    #[test]
    fn the_well_known_addresses_insert_the_path_first() {
        let u = Url::parse("https://h.invalid/tenant/mcp").unwrap();
        let got: Vec<String> = resource_metadata_urls(&u)
            .iter()
            .map(Url::to_string)
            .collect();
        assert_eq!(
            got,
            [
                "https://h.invalid/.well-known/oauth-protected-resource/tenant/mcp",
                "https://h.invalid/.well-known/oauth-protected-resource",
            ]
        );
        let root = Url::parse("https://h.invalid/").unwrap();
        assert_eq!(resource_metadata_urls(&root).len(), 1);
        let issuer = Url::parse("https://as.invalid/realm").unwrap();
        let got: Vec<String> = metadata_urls(&issuer).iter().map(Url::to_string).collect();
        assert_eq!(
            got,
            [
                "https://as.invalid/.well-known/oauth-authorization-server/realm",
                "https://as.invalid/.well-known/openid-configuration/realm",
                "https://as.invalid/realm/.well-known/openid-configuration",
            ]
        );
    }

    /// The readings of "the resource is this server" that are accepted: the
    /// server's own address, its origin, and a path above it; and the ones
    /// refused.
    #[test]
    fn a_resource_names_this_server_only_on_its_origin_and_path() {
        let s = Url::parse("https://h.invalid/tenant/mcp").unwrap();
        for ok in [
            "https://h.invalid/tenant/mcp",
            "https://h.invalid/tenant/mcp/",
            "https://h.invalid",
            "https://h.invalid/",
            "https://h.invalid/tenant",
        ] {
            assert!(names_this_server(ok, &s), "{ok}");
        }
        for bad in [
            "https://other.invalid/tenant/mcp",
            "http://h.invalid/tenant/mcp",
            "https://h.invalid:8443/tenant/mcp",
            "https://h.invalid/ten",
            "https://h.invalid/other",
            "not a url",
        ] {
            assert!(!names_this_server(bad, &s), "{bad}");
        }
    }
}
