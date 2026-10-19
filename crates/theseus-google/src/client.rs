//! The REST client: one caller for every method of every service (Google
//! design §1, §4.3). It signs nothing and keeps nothing: the caller hands it a
//! token, and it retries, backs off, and maps what Google answers.

use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use serde_json::Value;

use crate::error::{of_api_error, GoogleError, Kind};
use crate::retry::{decide, Decision, Policy, Repeat};
use crate::scopes::Service;
use crate::token::{without_url, AccessToken};

/// The longest a `Retry-After` is honoured.
const RETRY_AFTER_MAX: Duration = Duration::from_secs(60);

/// Where the requests go. Production, or one loopback stand-in for every
/// service (the config's `endpoint`, as `[aws] endpoint`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    one_base: Option<String>,
}

impl Endpoints {
    pub fn production() -> Self {
        Self {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            one_base: None,
        }
    }

    /// Every endpoint on one loopback origin: `/auth`, `/token`, `/revoke`,
    /// and the services' own paths.
    pub fn loopback(base: &str) -> Self {
        let base = base.trim_end_matches('/');
        Self {
            auth: format!("{base}/auth"),
            token: format!("{base}/token"),
            revoke: format!("{base}/revoke"),
            one_base: Some(base.to_string()),
        }
    }

    pub fn base(&self, service: Service) -> String {
        if let Some(b) = &self.one_base {
            return b.clone();
        }
        match service {
            Service::Gmail => "https://gmail.googleapis.com",
            Service::Drive | Service::Calendar => "https://www.googleapis.com",
            Service::Sheets => "https://sheets.googleapis.com",
            Service::Slides => "https://slides.googleapis.com",
            Service::Docs => "https://docs.googleapis.com",
        }
        .to_string()
    }
}

#[derive(Debug, Clone, Default)]
pub enum Body {
    #[default]
    None,
    Json(Value),
    Bytes {
        content_type: String,
        data: Vec<u8>,
    },
}

/// One request: the service, the HTTP verb, the path (from the service's
/// root, `/gmail/v1/users/me/profile`), the query, and the body.
#[derive(Debug, Clone)]
pub struct Request {
    pub service: Service,
    pub verb: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Body,
    pub repeat: Repeat,
}

impl Request {
    pub fn get(service: Service, path: impl Into<String>) -> Self {
        Self {
            service,
            verb: "GET".into(),
            path: path.into(),
            query: Vec::new(),
            body: Body::None,
            repeat: Repeat::Safe,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
    /// How many times the request was sent.
    pub attempts: u32,
}

impl Response {
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// A call that did not succeed: why, how many times it was sent, and whether
/// a non-repeatable one may have run (`outcome_unknown`).
#[derive(Debug, Clone)]
pub struct Failure {
    pub error: GoogleError,
    pub attempts: u32,
    pub unknown: bool,
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    endpoints: Endpoints,
    policy: Policy,
}

impl Client {
    pub fn new(endpoints: Endpoints) -> Self {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(90))
            .build()
            .unwrap_or_default();
        Self {
            http,
            endpoints,
            policy: Policy::default(),
        }
    }

    pub fn with_policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    /// The HTTP client, for the OAuth endpoints.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Send `req` under `token`, retried by the table in `retry.rs`.
    pub async fn send(&self, token: &AccessToken, req: &Request) -> Result<Response, Failure> {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let (error, wait) = match self.once(token, req).await {
                Ok(mut r) => {
                    r.attempts = attempts;
                    return Ok(r);
                }
                Err(e) => e,
            };
            let decision = decide(req.repeat, &error);
            if decision == Decision::Retry && attempts < self.policy.max_attempts {
                let backoff = self.policy.delay(attempts + 1);
                tokio::time::sleep(wait.map_or(backoff, |w| w.max(backoff).min(RETRY_AFTER_MAX)))
                    .await;
                continue;
            }
            return Err(Failure {
                unknown: decision == Decision::Unknown
                    || (req.repeat == Repeat::NonRepeatable && error.may_have_run()),
                error,
                attempts,
            });
        }
    }

    async fn once(
        &self,
        token: &AccessToken,
        req: &Request,
    ) -> Result<Response, (GoogleError, Option<Duration>)> {
        let url = format!("{}{}", self.endpoints.base(req.service), req.path);
        let verb = reqwest::Method::from_bytes(req.verb.as_bytes())
            .map_err(|_| (GoogleError::network("an unknown HTTP verb"), None))?;
        let mut b = self
            .http
            .request(verb, url)
            .bearer_auth(token.expose())
            .query(&req.query);
        b = match &req.body {
            Body::None => b,
            Body::Json(v) => b.json(v),
            Body::Bytes { content_type, data } => b
                .header(reqwest::header::CONTENT_TYPE, content_type.as_str())
                .body(data.clone()),
        };
        let resp = b.send().await.map_err(|e| {
            let reason = if e.is_connect() { "connect" } else { "timeout" };
            (
                GoogleError {
                    reason: reason.into(),
                    ..GoogleError::network(without_url(e))
                },
                None,
            )
        })?;
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = resp
            .bytes()
            .await
            .map_err(|e| {
                (
                    GoogleError {
                        reason: "dropped".into(),
                        ..GoogleError::network(without_url(e))
                    },
                    None,
                )
            })?
            .to_vec();
        if (200..300).contains(&status) {
            return Ok(Response {
                status,
                content_type,
                body,
                attempts: 1,
            });
        }
        let json = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let mut e = of_api_error(status, &json);
        if e.kind == Kind::Refused && status == 403 && e.message.is_empty() {
            e.message = "forbidden".into();
        }
        Err((e, retry_after))
    }
}
