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
//! What comes from outside is checked before it is used: every address a
//! server names is `https` (a test's loopback server alone may be `http`, by
//! a rule only tests build) and is reached only at a public address, by the
//! web fetch's own rule ([`Rule`]); no redirect is followed off its own
//! origin; the authorization server's `issuer` is the one asked, and its
//! endpoints are on its own origin unless its redirect names it; the
//! resource's metadata names this server. Every value is given to the
//! daemon's [`Holder`] where it is first read, before any check of it can
//! return, so the scrubber knows it from the moment it exists.

pub mod discover;
pub mod exchange;
pub mod fake;
pub mod register;
pub mod request;

use std::sync::Arc;
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

/// The longest address a server may name.
pub const MAX_URL: usize = 2048;

/// The most authorization servers a resource's metadata may have tried.
pub const MAX_ISSUERS: usize = 4;

/// The longest an access token is taken to last: a longer `expires_in` is
/// read as this.
pub const MAX_EXPIRES_SECS: u64 = 366 * 24 * 3600;

/// Why a step of a sign-in failed, in the daemon's words. No variant holds a
/// value, a body or an address a server chose.
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

/// Told each value of a sign-in where it is first read: its kind (`access`,
/// `refresh`, `code`, `client`, `verifier`) and the value. The daemon puts it
/// on its secret board, so the scrubber knows it.
pub type Holder = Arc<dyn Fn(&'static str, &str) + Send + Sync>;

/// Where a sign-in's calls may go: the daemon's rule for an address, the web
/// fetch's own (`theseus-core`'s `web::net`), given to this crate whole.
#[derive(Clone)]
pub struct Rule {
    /// Resolves every name a call reaches; it refuses one that resolves to an
    /// address that is not public, and the connection uses the very answer it
    /// checked (nothing is resolved twice).
    pub resolver: Arc<dyn reqwest::dns::Resolve>,
    /// Why an address's host, as written, is not reached (a private address,
    /// `localhost`), or None.
    pub private_host: fn(&Url) -> Option<String>,
    /// Whether a failed request was the resolver's refusal.
    pub refused: fn(&(dyn std::error::Error + 'static)) -> bool,
    /// Whether `http` is taken for this address: never in the daemon; a test's
    /// loopback server, by the rule its tests build.
    pub plaintext: fn(&Url) -> bool,
    /// Whether a token, code or secret can be kept from view: the daemon's
    /// scrubber's own test (`scrub::protectable`). One it would not mask is a
    /// bad answer, never used.
    pub protectable: fn(&str) -> bool,
}

impl Rule {
    /// Why `u` is not an address this rule reaches as written (it is not
    /// `https`, or not `http` the rule takes; it carries a name, a key or a
    /// fragment; its host is not public), or None.
    pub fn refuses(&self, u: &Url) -> Option<&'static str> {
        let scheme_ok = match u.scheme() {
            "https" => true,
            "http" => (self.plaintext)(u),
            _ => false,
        };
        if !scheme_ok {
            return Some("is not https");
        }
        if !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
            return Some("carries a name, a key or a fragment");
        }
        if (self.private_host)(u).is_some() {
            return Some("is at an address that is not public");
        }
        None
    }

    /// The rule's resolver, as reqwest takes one: every new connection asks
    /// it, and connects to the very answer it checked.
    pub fn resolver(&self) -> Arc<impl reqwest::dns::Resolve> {
        Arc::new(Via(self.resolver.clone()))
    }
}

/// `Rule::resolver` behind the `Sized` resolver reqwest takes.
struct Via(Arc<dyn reqwest::dns::Resolve>);

impl reqwest::dns::Resolve for Via {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        self.0.resolve(name)
    }
}

/// A sign-in's calls: the client every one goes through, the rule it keeps,
/// and the daemon's holder.
#[derive(Clone)]
pub struct Reach {
    client: reqwest::Client,
    rule: Rule,
    hold: Holder,
}

impl Reach {
    /// The client of `rule`: it follows a redirect only on its own origin (a
    /// 307 would carry a code or a refresh token to wherever it pointed), at
    /// most three; it resolves every name through the rule's resolver, so a
    /// same-origin redirect is checked again; and no proxy resolves a name
    /// past it. A client that cannot be built is a refusal, never a client
    /// with fewer limits.
    pub fn new(rule: Rule, hold: Holder) -> Result<Self, OauthError> {
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
        let client = reqwest::Client::builder()
            .redirect(policy)
            .no_proxy()
            .dns_resolver(rule.resolver())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| OauthError::Refused("the sign-in's HTTP client could not start".into()))?;
        Ok(Self { client, rule, hold })
    }

    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Give `value`, of kind `what`, to the daemon's holder.
    pub fn hold(&self, what: &'static str, value: &str) {
        if !value.is_empty() {
            (self.hold)(what, value);
        }
    }

    pub fn holder(&self) -> Holder {
        self.hold.clone()
    }

    /// Whether `value` can be kept from view ([`Rule::protectable`]).
    pub fn protects(&self, value: &str) -> bool {
        (self.rule.protectable)(value)
    }

    pub fn rule(&self) -> &Rule {
        &self.rule
    }

    /// `text` as an address a sign-in may reach: at most [`MAX_URL`] long,
    /// `https` (or `http` where the rule takes it), with no name or key before
    /// an `@` and no fragment, and not at a host that is not public.
    pub fn endpoint(&self, text: &str, what: &str) -> Result<Url, OauthError> {
        if text.len() > MAX_URL {
            return Err(OauthError::Refused(format!(
                "the sign-in's {what} is too long: refused"
            )));
        }
        let u = Url::parse(text)
            .map_err(|_| OauthError::Refused(format!("the sign-in's {what} is not an address")))?;
        match self.rule.refuses(&u) {
            Some(why) => Err(OauthError::Refused(format!(
                "the sign-in's {what} {why}: refused"
            ))),
            None => Ok(u),
        }
    }

    /// A send's failure, without its address (an address can carry a query)
    /// and without what the resolver found.
    fn unreachable(&self, what: &'static str, e: reqwest::Error) -> OauthError {
        if (self.rule.refused)(&e) {
            return OauthError::Refused(format!(
                "{what} is at an address that is not public: refused"
            ));
        }
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

    async fn body_of(
        &self,
        what: &'static str,
        resp: reqwest::Response,
    ) -> Result<Vec<u8>, OauthError> {
        let mut resp = resp;
        let mut out = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| self.unreachable(what, e))? {
            if out.len() + chunk.len() > MAX_BODY {
                return Err(OauthError::Undecodable { what });
            }
            out.extend_from_slice(&chunk);
        }
        Ok(out)
    }

    /// GET a JSON document. `Ok(None)`: the server has none there (404 or 410).
    pub async fn get_json(
        &self,
        url: &Url,
        what: &'static str,
    ) -> Result<Option<Value>, OauthError> {
        let resp = self
            .client
            .get(url.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| self.unreachable(what, e))?;
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::GONE {
            return Ok(None);
        }
        let body = self.body_of(what, resp).await?;
        if !status.is_success() {
            return Err(refusal(what, status, &body));
        }
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) if v.is_object() => Ok(Some(v)),
            _ => Err(OauthError::Undecodable { what }),
        }
    }

    /// POST a request body and read a JSON object back: `(status, object)`,
    /// or the refusal of a status that is not success. `first` is told the
    /// object before anything checks it.
    pub(crate) async fn post(
        &self,
        req: reqwest::RequestBuilder,
        what: &'static str,
        first: impl FnOnce(&Value),
    ) -> Result<(u16, Value), OauthError> {
        let resp = req
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| self.unreachable(what, e))?;
        let status = resp.status();
        let body = self.body_of(what, resp).await?;
        let v = serde_json::from_slice::<Value>(&body).ok();
        if let Some(v) = &v {
            first(v);
        }
        if !status.is_success() {
            return Err(refusal(what, status, &body));
        }
        match v {
            Some(v) if v.is_object() => Ok((status.as_u16(), v)),
            _ => Err(OauthError::Undecodable { what }),
        }
    }

    /// POST with no answer read but its status: the revoke.
    pub(crate) async fn send(
        &self,
        req: reqwest::RequestBuilder,
        what: &'static str,
    ) -> Result<u16, OauthError> {
        let resp = req.send().await.map_err(|e| self.unreachable(what, e))?;
        Ok(resp.status().as_u16())
    }
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

/// Tests' rules, which product code cannot build: `loopback` takes a test's
/// `http` server on this machine and checks no address; `strict` is the
/// daemon's shape, with this machine as the address that is not public.
#[cfg(test)]
pub(crate) mod rules {
    use super::*;

    struct System;

    impl reqwest::dns::Resolve for System {
        fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
            let host = name.as_str().to_string();
            Box::pin(async move {
                let addrs: Vec<std::net::SocketAddr> =
                    tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
                Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
            })
        }
    }

    /// The scrubber's test, as the daemon gives it: a trimmed length of
    /// eight or more.
    pub fn eight(v: &str) -> bool {
        v.trim().len() >= 8
    }

    pub fn loopback() -> Rule {
        Rule {
            resolver: Arc::new(System),
            private_host: |_| None,
            refused: |_| false,
            plaintext: is_loopback,
            protectable: eight,
        }
    }

    pub fn strict() -> Rule {
        Rule {
            resolver: Arc::new(System),
            private_host: |u| is_loopback(u).then(|| "this machine".to_string()),
            refused: |_| false,
            plaintext: |_| false,
            protectable: eight,
        }
    }

    pub fn reach(rule: Rule) -> Reach {
        Reach::new(rule, Arc::new(|_, _| {})).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The daemon's shape: `https` alone, at a public address; a test's rule
    /// takes `http` on this machine, and nothing else more.
    #[test]
    fn only_https_at_a_public_address_and_a_tests_loopback() {
        let strict = rules::reach(rules::strict());
        assert!(strict
            .endpoint("https://as.example.invalid/token", "token endpoint")
            .is_ok());
        for (bad, says) in [
            ("http://127.0.0.1:9/token", "is not https"),
            ("https://127.0.0.1:9/token", "not public"),
            ("https://localhost/token", "not public"),
            ("http://as.example.invalid/token", "is not https"),
        ] {
            let e = strict.endpoint(bad, "token endpoint").unwrap_err();
            assert!(e.to_string().contains(says), "{bad}: {e}");
        }
        let tests = rules::reach(rules::loopback());
        assert!(tests
            .endpoint("http://127.0.0.1:9/token", "token endpoint")
            .is_ok());
        assert!(tests
            .endpoint("http://localhost:9/token", "token endpoint")
            .is_ok());
        let long = format!("https://as.example.invalid/{}", "a".repeat(MAX_URL));
        for bad in [
            "http://as.example.invalid/token",
            "ftp://as.example.invalid/token",
            "https://user:key@as.example.invalid/token",
            "https://as.example.invalid/token#x",
            "not an address",
            long.as_str(),
        ] {
            let e = tests.endpoint(bad, "token endpoint").unwrap_err();
            assert!(matches!(e, OauthError::Refused(_)), "{bad}: {e}");
        }
        assert_eq!(
            tests
                .endpoint("http://as.example.invalid/t", "token endpoint")
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
