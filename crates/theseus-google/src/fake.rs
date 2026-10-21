//! A loopback Google for tests (shared contract 6): the token endpoint, a
//! route table with the account's own profile in it, a request log, and the
//! switches the failures need (429, 5xx, `invalid_grant`, `SERVICE_DISABLED`).
//! Each service's stubs go in a route of their own; no test touches a real
//! Google account.

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde_json::{json, Value};
use tokio::net::TcpListener;

pub mod gmail;

use crate::http1::{read_request, write_response, Req};
use crate::scopes::Service;

mod calendar;

/// A canned answer.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body,
            headers: Vec::new(),
        }
    }

    pub fn throttled(retry_after: Option<u64>) -> Self {
        let mut r = Self::json(
            429,
            json!({"error": {"code": 429, "message": "slow down",
            "errors": [{"reason": "rateLimitExceeded"}]}}),
        );
        if let Some(s) = retry_after {
            r.headers.push(("Retry-After".into(), s.to_string()));
        }
        r
    }

    pub fn server_error() -> Self {
        Self::json(
            503,
            json!({"error": {"code": 503, "message": "backend error", "errors": [{"reason": "backendError"}]}}),
        )
    }
}

type Handler = Arc<dyn Fn(&Req) -> Reply + Send + Sync>;

/// One request the fake saw.
#[derive(Debug, Clone)]
pub struct Logged {
    pub verb: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub body: Value,
    /// The bearer token the request carried, whole, so a test can grep for it.
    pub bearer: Option<String>,
}

struct Inner {
    account: String,
    granted: Vec<String>,
    refresh_token: String,
    code: String,
    lifetime: u64,
    refresh_delay: Duration,
    invalid_grant: bool,
    issued: u64,
    token_requests: u64,
    revoked: bool,
    log: Vec<Logged>,
    routes: Vec<(String, String, Handler)>,
    failures: VecDeque<Reply>,
    disabled: Option<(Service, String)>,
    forms: Vec<std::collections::BTreeMap<String, String>>,
    mail: gmail::Mailbox,
}

pub struct Fake {
    addr: SocketAddr,
    inner: Arc<Mutex<Inner>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The refresh token every grant issues.
pub const REFRESH_TOKEN: &str = "1//fake-refresh-token-0123456789";
/// The code the fake accepts.
pub const CODE: &str = "4/fake-authorization-code";

impl Fake {
    pub async fn start() -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the fake Google");
        let addr = listener.local_addr().expect("its address");
        let inner = Arc::new(Mutex::new(Inner {
            account: "owner@example.test".into(),
            granted: crate::scopes::requested(&Service::ALL),
            refresh_token: REFRESH_TOKEN.into(),
            code: CODE.into(),
            lifetime: 3600,
            refresh_delay: Duration::ZERO,
            invalid_grant: false,
            issued: 0,
            token_requests: 0,
            revoked: false,
            log: Vec::new(),
            routes: Vec::new(),
            failures: VecDeque::new(),
            disabled: None,
            forms: Vec::new(),
            mail: gmail::Mailbox::default(),
        }));
        let me = inner.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let me = me.clone();
                tokio::spawn(async move {
                    let Some(req) = read_request(&mut stream).await else {
                        return;
                    };
                    let delay = me.lock().unwrap().refresh_delay;
                    if req.path() == "/token" && !delay.is_zero() {
                        tokio::time::sleep(delay).await;
                    }
                    let reply = answer(&me, &req);
                    let extra: Vec<(&str, String)> = reply
                        .headers
                        .iter()
                        .map(|(k, v)| (k.as_str(), v.clone()))
                        .collect();
                    let body = serde_json::to_vec(&reply.body).unwrap_or_default();
                    write_response(&mut stream, reply.status, "application/json", &extra, &body)
                        .await;
                });
            }
        });
        Fake { addr, inner, task }
    }

    /// The origin to put in `[google] endpoint`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.inner.lock().unwrap())
    }

    /// The address the `id_token` names.
    pub fn set_account(&self, email: &str) {
        self.with(|i| i.account = email.into());
    }

    /// The scopes a redeemed code reports as granted.
    pub fn set_granted(&self, scopes: &[&str]) {
        self.with(|i| i.granted = scopes.iter().map(|s| (*s).to_string()).collect());
    }

    /// Every refresh now answers `invalid_grant`.
    pub fn set_invalid_grant(&self, on: bool) {
        self.with(|i| i.invalid_grant = on);
    }

    /// How long a token request takes (for the single flight).
    pub fn set_refresh_delay(&self, d: Duration) {
        self.with(|i| i.refresh_delay = d);
    }

    /// Access tokens live this many seconds.
    pub fn set_lifetime(&self, secs: u64) {
        self.with(|i| i.lifetime = secs);
    }

    /// The next `n` API requests get `reply`, then the routes answer.
    pub fn fail_next(&self, n: usize, reply: &Reply) {
        self.with(|i| (0..n).for_each(|_| i.failures.push_back(reply.clone())));
    }

    /// Every API request of `service` answers `SERVICE_DISABLED` for `project`.
    pub fn service_disabled(&self, service: Service, project: &str) {
        self.with(|i| i.disabled = Some((service, project.to_string())));
    }

    /// `verb path` answers with `handler`.
    pub fn route(
        &self,
        verb: &str,
        path: &str,
        handler: impl Fn(&Req) -> Reply + Send + Sync + 'static,
    ) {
        self.with(|i| i.routes.push((verb.into(), path.into(), Arc::new(handler))));
    }

    /// `verb path` answers a fixed JSON body.
    pub fn json(&self, verb: &str, path: &str, body: Value) {
        self.route(verb, path, move |_| Reply::json(200, body.clone()));
    }

    /// The mailbox the Gmail stubs answer from.
    pub fn mail(&self) -> gmail::Mailbox {
        self.with(|i| i.mail.clone())
    }

    /// Every API request seen, oldest first.
    pub fn log(&self) -> Vec<Logged> {
        self.with(|i| i.log.clone())
    }

    /// The token endpoint's forms, as sent (a test reads what was redeemed).
    pub fn forms(&self) -> Vec<std::collections::BTreeMap<String, String>> {
        self.with(|i| i.forms.clone())
    }

    /// How many times the token endpoint answered a refresh.
    pub fn token_requests(&self) -> u64 {
        self.with(|i| i.token_requests)
    }

    pub fn revoked(&self) -> bool {
        self.with(|i| i.revoked)
    }
}

fn token_reply(i: &mut Inner, req: &Req) -> Reply {
    let form = req.form();
    i.forms.push(form.clone());
    let grant = form.get("grant_type").map(String::as_str).unwrap_or("");
    let bad = |e: &str, d: &str| Reply::json(400, json!({"error": e, "error_description": d}));
    if form.get("client_id").is_none_or(String::is_empty)
        || form.get("client_secret").is_none_or(String::is_empty)
    {
        return bad("invalid_client", "the client is not identified");
    }
    i.issued += 1;
    let access = format!("ya29.fake-access-{}", i.issued);
    match grant {
        "authorization_code" => {
            if form.get("code") != Some(&i.code)
                || form.get("code_verifier").is_none_or(String::is_empty)
            {
                return bad("invalid_grant", "Bad Request");
            }
            let payload =
                URL_SAFE_NO_PAD.encode(json!({"email": i.account, "sub": "1"}).to_string());
            Reply::json(
                200,
                json!({"access_token": access, "refresh_token": i.refresh_token, "expires_in": i.lifetime,
                    "scope": i.granted.join(" "), "token_type": "Bearer", "id_token": format!("h.{payload}.s")}),
            )
        }
        "refresh_token" => {
            i.token_requests += 1;
            if i.invalid_grant || i.revoked || form.get("refresh_token") != Some(&i.refresh_token) {
                return bad("invalid_grant", "Token has been expired or revoked.");
            }
            Reply::json(
                200,
                json!({"access_token": access, "expires_in": i.lifetime, "scope": i.granted.join(" "), "token_type": "Bearer"}),
            )
        }
        _ => bad("unsupported_grant_type", ""),
    }
}

fn disabled_reply(service: Service, project: &str) -> Reply {
    let host = service.api_host();
    Reply::json(
        403,
        json!({"error": {"code": 403,
            "message": format!("{} has not been used in project {project} before or it is disabled.", service.api_label()),
            "errors": [{"reason": "accessNotConfigured", "domain": "usageLimits"}],
            "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "SERVICE_DISABLED",
                "metadata": {"service": host, "consumer": format!("projects/{project}"),
                    "activationUrl": format!("https://console.developers.google.com/apis/api/{host}/overview?project={project}")}}]}}),
    )
}

fn service_of(path: &str) -> Option<Service> {
    crate::methods::service_of_path(path)
}

fn answer(me: &Arc<Mutex<Inner>>, req: &Req) -> Reply {
    let mut i = me.lock().unwrap();
    match (req.method.as_str(), req.path()) {
        ("POST", "/token") => return token_reply(&mut i, req),
        ("POST", "/revoke") => {
            i.revoked = true;
            return Reply::json(200, json!({}));
        }
        _ => {}
    }
    let bearer = req
        .headers
        .get("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .map(str::to_string);
    i.log.push(Logged {
        verb: req.method.clone(),
        path: req.path().to_string(),
        query: req.query(),
        body: req.json(),
        bearer: bearer.clone(),
    });
    if !bearer.is_some_and(|b| b.starts_with("ya29.fake-access-")) {
        return Reply::json(
            401,
            json!({"error": {"code": 401, "message": "Invalid Credentials", "status": "UNAUTHENTICATED"}}),
        );
    }
    if let Some(f) = i.failures.pop_front() {
        return f;
    }
    if let Some((service, project)) = &i.disabled {
        if service_of(req.path()) == Some(*service) {
            return disabled_reply(*service, project);
        }
    }
    let handler = i
        .routes
        .iter()
        .rev()
        .find(|(v, p, _)| *v == req.method && p == req.path())
        .map(|(_, _, h)| h.clone());
    let account = i.account.clone();
    let mail = i.mail.clone();
    drop(i);
    if let Some(h) = handler {
        return h(req);
    }
    if let Some(r) = gmail::answer(&mail, req) {
        return r;
    }
    if let Some(r) = calendar::stub(req, &account) {
        return r;
    }
    match (req.method.as_str(), req.path()) {
        ("GET", "/gmail/v1/users/me/profile") => Reply::json(
            200,
            json!({"emailAddress": account, "messagesTotal": 3, "threadsTotal": 2, "historyId": "100"}),
        ),
        ("GET", "/drive/v3/about") => {
            Reply::json(200, json!({"user": {"emailAddress": account, "me": true}}))
        }
        ("GET", "/calendar/v3/users/me/calendarList") => {
            Reply::json(200, json!({"items": [{"id": account, "primary": true}]}))
        }
        _ => Reply::json(
            404,
            json!({"error": {"code": 404, "message": "Not Found", "errors": [{"reason": "notFound"}]}}),
        ),
    }
}
