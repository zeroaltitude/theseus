//! The client: one call at a time, any operation of any service.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime};

use serde_json::{Map, Value};
use theseus_aws_catalog::{Catalog, Classification, OperationRef, RetryClass};

use crate::build::{build, Built};
use crate::creds::Credentials;
use crate::error::CallError;
use crate::paginate;
use crate::request::HttpRequest;
use crate::response::{self, Answer, RawResponse};
use crate::retry::{decide, Decision, Failure, RetryPolicy};
use crate::sign;
use crate::value;

/// The client's settings.
#[derive(Clone, Debug)]
pub struct ClientConfig {
    /// The region a call goes to unless it names one.
    pub region: String,
    /// The user agent's first token (`theseus/0.0.1`).
    pub product: String,
    /// Sends every request here instead of the catalog's endpoint: a test's
    /// fake endpoint. S3 goes path-style, and host prefixes are left off;
    /// the signature's scope is still the catalog's.
    pub endpoint_override: Option<String>,
    pub retry: RetryPolicy,
    pub connect_timeout: Duration,
    /// One attempt, from sending to the last byte of the answer.
    pub attempt_timeout: Duration,
    /// The largest answer read; a bigger one fails as `TooLarge`, which says
    /// the call ran.
    pub max_response_bytes: usize,
    /// The most pages one call follows, whatever it asks.
    pub max_pages: u32,
}

impl ClientConfig {
    pub fn new(region: impl Into<String>) -> ClientConfig {
        ClientConfig {
            region: region.into(),
            product: format!("theseus/{}", env!("CARGO_PKG_VERSION")),
            endpoint_override: None,
            retry: RetryPolicy::default(),
            connect_timeout: Duration::from_millis(3100),
            attempt_timeout: Duration::from_secs(60),
            max_response_bytes: 16 << 20,
            max_pages: 100,
        }
    }
}

/// Who a call is for, so CloudTrail can say (AWS design §3.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribution {
    /// The execution (`exe_…`), which is also the session's name.
    pub execution: Option<String>,
    /// The tool call's correlation id. It fills an operation's idempotency
    /// token, so a retried call runs at most once.
    pub call: Option<String>,
}

/// One call: an operation, its input as JSON keyed by member names, and how
/// many pages to read.
#[derive(Clone, Debug)]
pub struct Call<'a> {
    pub service: &'a str,
    pub operation: &'a str,
    pub input: &'a Value,
    /// The call's region, else the client's.
    pub region: Option<&'a str>,
    /// Pages to read: 1 reads one, and the output says where the next starts.
    pub pages: u32,
    pub attribution: &'a Attribution,
}

/// A call's answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The output shape as JSON, keyed by member names; several pages joined.
    pub body: Value,
    pub status: u16,
    /// AWS's request id (CloudTrail's `requestID`): the first page's.
    pub request_id: Option<String>,
    /// Every page's request id, in order.
    pub request_ids: Vec<String>,
    /// Attempts across every page.
    pub attempts: u32,
    pub pages: u32,
    /// The input members that read the next page, when pages remain.
    pub next: Option<Map<String, Value>>,
    /// The idempotency token sent, and the member it went in.
    pub idempotency_token: Option<(String, String)>,
}

/// A request as it would be sent: signed, with what the gate reads.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub request: HttpRequest,
    pub classification: Classification,
    pub idempotency_token: Option<(String, String)>,
}

/// The AWS client. Building one does nothing (AWS design §3.10): the
/// catalog's index decodes on the first call, the HTTP client (its TLS
/// roots) is built on the first send, and no connection opens before it.
pub struct Client {
    http: OnceLock<Result<reqwest::Client, String>>,
    /// A test's catalog; `None` is the embedded one.
    catalog: Option<&'static Catalog>,
    config: ClientConfig,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("config", &self.config)
            .finish()
    }
}

/// An attempt that got no answer to read.
enum Fault {
    NotSent(String),
    AfterSend(String),
    TooLarge(Option<String>),
}

/// A user agent token: `[A-Za-z0-9._-]`, at most 64 characters.
fn ua_token(s: &str) -> String {
    s.chars()
        .take(64)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// A token fits a member when it is as long as the member allows (64
/// characters, AWS's usual limit, where the model names none: EC2's
/// `ClientToken` is a bare string) and made of the characters tokens use.
fn fits(token: &str, op: OperationRef<'_>, member: &str) -> bool {
    let Some(shape) = op.input().and_then(|i| i.member(member)).map(|m| m.shape()) else {
        return false;
    };
    let len = token.len() as i64;
    shape.min().is_none_or(|m| len >= m)
        && len <= shape.max().unwrap_or(64)
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Client {
    /// A client over the catalog the binary carries. It does nothing yet.
    pub fn new(config: ClientConfig) -> Client {
        Client {
            http: OnceLock::new(),
            catalog: None,
            config,
        }
    }

    /// A client over another catalog (a test's).
    pub fn with_catalog(config: ClientConfig, catalog: &'static Catalog) -> Client {
        Client {
            http: OnceLock::new(),
            catalog: Some(catalog),
            config,
        }
    }

    /// The catalog, its index decoded on the first use.
    pub fn catalog(&self) -> Result<&'static Catalog, CallError> {
        match self.catalog {
            Some(c) => Ok(c),
            None => Catalog::embedded().map_err(|e| CallError::InvalidInput(e.to_string())),
        }
    }

    fn http(&self) -> Result<&reqwest::Client, Fault> {
        self.http
            .get_or_init(|| {
                reqwest::Client::builder()
                    .connect_timeout(self.config.connect_timeout)
                    // A redirect would resend a signed request elsewhere;
                    // AWS's redirects are errors that name the right region.
                    .redirect(reqwest::redirect::Policy::none())
                    .no_proxy()
                    .build()
                    .map_err(|e| format!("the HTTP client: {e}"))
            })
            .as_ref()
            .map_err(|e| Fault::NotSent(e.clone()))
    }

    pub fn config(&self) -> &ClientConfig {
        &self.config
    }

    fn operation<'c>(
        svc: &'c theseus_aws_catalog::Service,
        call: &Call<'_>,
    ) -> Result<OperationRef<'c>, CallError> {
        svc.operation(call.operation).ok_or_else(|| {
            CallError::InvalidInput(format!(
                "{} has no operation {:?} (aws.describe lists them)",
                svc.name(),
                call.operation
            ))
        })
    }

    /// The input with its idempotency token filled from the correlation id
    /// (or a fresh one), when the caller left it out.
    fn fill_token(op: OperationRef<'_>, call: &Call<'_>) -> (Value, Option<(String, String)>) {
        let mut input = call.input.clone();
        let Some(member) = op.idempotency_token() else {
            return (input, None);
        };
        let name = member.name().to_owned();
        if let Some(Value::String(given)) = input.get(&name) {
            let given = given.clone();
            return (input, Some((name, given)));
        }
        let token = call
            .attribution
            .call
            .as_deref()
            .filter(|t| fits(t, op, &name))
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let id = uuid::Uuid::now_v7();
                if fits(&id.to_string(), op, &name) {
                    id.to_string()
                } else {
                    id.simple().to_string()
                }
            });
        if input.is_null() {
            input = Value::Object(Map::new());
        }
        if let Value::Object(o) = &mut input {
            o.insert(name.clone(), Value::String(token.clone()));
        }
        (input, Some((name, token)))
    }

    fn user_agent(&self, a: &Attribution) -> String {
        let mut ua = self.config.product.clone();
        if let Some(e) = &a.execution {
            ua.push_str(&format!(" exec/{}", ua_token(e)));
        }
        if let Some(c) = &a.call {
            ua.push_str(&format!(" call/{}", ua_token(c)));
        }
        ua
    }

    fn built(
        &self,
        op: OperationRef<'_>,
        input: &Value,
        call: &Call<'_>,
    ) -> Result<Built, CallError> {
        // A call the client cannot make says so before its input is checked.
        if let Some(why) = crate::build::unsupported(op) {
            return Err(CallError::Unsupported(why));
        }
        let skip = op.idempotency_token().map(|m| m.name().to_owned());
        let typed = value::normalize(op.input(), input, skip.as_deref())
            .map_err(CallError::InvalidInput)?;
        let region = call.region.unwrap_or(&self.config.region);
        let mut b = build(
            self.catalog()?,
            op,
            typed.as_ref(),
            region,
            self.config.endpoint_override.as_deref(),
        )?;
        b.req
            .headers
            .push(("User-Agent".into(), self.user_agent(call.attribution)));
        Ok(b)
    }

    /// The signed request a call's first page would send, without sending
    /// it, and what the gate reads from the operation.
    pub fn prepare(
        &self,
        call: &Call<'_>,
        creds: &Credentials,
        now: SystemTime,
    ) -> Result<Prepared, CallError> {
        let svc = self
            .catalog()?
            .service(call.service)
            .map_err(|e| CallError::InvalidInput(e.to_string()))?;
        let op = Client::operation(&svc, call)?;
        let (input, token) = Client::fill_token(op, call);
        let mut b = self.built(op, &input, call)?;
        if b.signed {
            sign::sign_request(&mut b.req, creds, &b.scope, now)
                .map_err(CallError::InvalidInput)?;
        }
        Ok(Prepared {
            request: b.req,
            classification: op.classify(),
            idempotency_token: token,
        })
    }

    /// A presigned URL for a call (S3 `GetObject`, say), valid for `expires`.
    pub fn presign(
        &self,
        call: &Call<'_>,
        creds: &Credentials,
        now: SystemTime,
        expires: Duration,
    ) -> Result<String, CallError> {
        let svc = self
            .catalog()?
            .service(call.service)
            .map_err(|e| CallError::InvalidInput(e.to_string()))?;
        let op = Client::operation(&svc, call)?;
        let (input, _) = Client::fill_token(op, call);
        let mut b = self.built(op, &input, call)?;
        // A presigned URL is used by someone else's client: no user agent.
        b.req
            .headers
            .retain(|(k, _)| !k.eq_ignore_ascii_case("user-agent"));
        sign::presign_request(&b.req, creds, &b.scope, now, expires)
            .map_err(CallError::InvalidInput)
    }

    /// Makes a call: sends it, retries it by its retry class, reads the
    /// answer, and follows its pages.
    pub async fn call(&self, call: &Call<'_>, creds: &Credentials) -> Result<Output, CallError> {
        let svc = self
            .catalog()?
            .service(call.service)
            .map_err(|e| CallError::InvalidInput(e.to_string()))?;
        let op = Client::operation(&svc, call)?;
        let class = op.classify().retry;
        let (mut input, token) = Client::fill_token(op, call);
        let want = call.pages.clamp(1, self.config.max_pages.max(1));
        let mut pages = Vec::new();
        let mut ids = Vec::new();
        let mut attempts = 0;
        let mut status;
        let mut next;
        loop {
            let (body, request_id, st, n) = self.page(op, class, &input, call, creds).await?;
            attempts += n;
            status = st;
            ids.extend(request_id);
            next = op
                .paginator()
                .and_then(|p| paginate::next_input(p, &body, &input));
            pages.push(body);
            match (&next, pages.len() < want as usize) {
                (Some(tokens), true) => {
                    if let Value::Object(o) = &mut input {
                        o.extend(tokens.clone());
                    }
                }
                _ => break,
            }
        }
        let n = pages.len() as u32;
        let body = match op.paginator() {
            Some(p) => paginate::merge(p, &pages),
            None => pages.pop().unwrap_or(Value::Null),
        };
        Ok(Output {
            body,
            status,
            request_id: ids.first().cloned(),
            request_ids: ids,
            attempts,
            pages: n,
            next,
            idempotency_token: token,
        })
    }

    /// One page: attempts until an answer, a failure the class does not
    /// retry, or the attempts run out.
    async fn page(
        &self,
        op: OperationRef<'_>,
        class: RetryClass,
        input: &Value,
        call: &Call<'_>,
        creds: &Credentials,
    ) -> Result<(Value, Option<String>, u16, u32), CallError> {
        let built = self.built(op, input, call)?;
        let max = self.config.retry.max_attempts.max(1);
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut req = built.req.clone();
            if built.signed {
                // Each attempt is signed afresh: the signature carries its time.
                sign::sign_request(&mut req, creds, &built.scope, SystemTime::now())
                    .map_err(CallError::InvalidInput)?;
            }
            let retry = match self.send(&req).await {
                Ok(resp) => match response::read(op, &resp) {
                    Answer::Ok { body, request_id } => {
                        return Ok((body, request_id, resp.status, attempt));
                    }
                    Answer::Unreadable(reason) => {
                        return Err(CallError::Unreadable {
                            status: resp.status,
                            request_id: resp.request_id(),
                            reason,
                        });
                    }
                    Answer::Err(e) => match decide(class, &Failure::Aws(&e)) {
                        Decision::Retry if attempt < max => true,
                        Decision::Unknown => {
                            return Err(CallError::OutcomeUnknown {
                                attempts: attempt,
                                reason: format!(
                                    "AWS answered {} to a call that is not safe to repeat",
                                    e.code
                                ),
                                error: Some(Box::new(e)),
                            });
                        }
                        Decision::Retry | Decision::Fail => {
                            return Err(CallError::Aws(Box::new(e)))
                        }
                    },
                },
                Err(Fault::TooLarge(request_id)) => {
                    return Err(CallError::TooLarge {
                        limit: self.config.max_response_bytes,
                        request_id,
                    });
                }
                Err(Fault::NotSent(reason)) => match decide(class, &Failure::NotSent) {
                    Decision::Retry if attempt < max => true,
                    _ => {
                        return Err(CallError::NotSent {
                            attempts: attempt,
                            reason,
                        })
                    }
                },
                Err(Fault::AfterSend(reason)) => match decide(class, &Failure::AfterSend) {
                    Decision::Retry if attempt < max => true,
                    _ => {
                        return Err(CallError::OutcomeUnknown {
                            attempts: attempt,
                            reason,
                            error: None,
                        })
                    }
                },
            };
            if retry {
                tokio::time::sleep(self.config.retry.delay(attempt + 1)).await;
            }
        }
    }

    /// One attempt on the wire, its body read up to the cap.
    async fn send(&self, req: &HttpRequest) -> Result<RawResponse, Fault> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes())
            .map_err(|e| Fault::NotSent(e.to_string()))?;
        let mut rb = self
            .http()?
            .request(method, req.url())
            .timeout(self.config.attempt_timeout);
        for (k, v) in &req.headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        let mut resp = rb.body(req.body.clone()).send().await.map_err(|e| {
            // A connection that never opened sent nothing; anything later
            // may have reached AWS.
            if e.is_connect() || e.is_builder() {
                Fault::NotSent(describe(&e))
            } else {
                Fault::AfterSend(describe(&e))
            }
        })?;
        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_ascii_lowercase(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let mut body = Vec::new();
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > self.config.max_response_bytes {
                        let raw = RawResponse {
                            status,
                            headers,
                            body: Vec::new(),
                        };
                        return Err(Fault::TooLarge(raw.request_id()));
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(e) => return Err(Fault::AfterSend(describe(&e))),
            }
        }
        Ok(RawResponse {
            status,
            headers,
            body,
        })
    }
}

/// A transport error's chain of causes, on one line.
fn describe(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(c) = src {
        s.push_str(": ");
        s.push_str(&c.to_string());
        src = c.source();
    }
    if e.is_timeout() {
        s.push_str(" (timed out)");
    }
    s
}
