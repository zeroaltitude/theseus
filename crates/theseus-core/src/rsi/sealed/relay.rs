//! The model relay (theseus-pw1q.21.5, the hard keel): how a sealed run's
//! candidate reaches a model with no key in its hands and no word of its own
//! about what it spent.
//!
//! The candidate's `model.api_base` is `http://<name>`, and its job's egress
//! list is that one name. The production daemon serves the name itself, in
//! process, on the job's listener (`theseus_kernel::job_egress::register`):
//! a request is read, checked, forwarded to the provider with the daemon's
//! own key, and its answer handed back. Nothing else is reachable.
//!
//! - **Only the profile under test**: a request for any other model is
//!   refused.
//! - **Only while self-improvement runs**: refused while `rsi::gate` says off
//!   or halted.
//! - **Reserve, then spend**: each call reserves its worst case (its
//!   `max_tokens` and an input estimate at the catalog's prices) on the item's
//!   line (`[self.sealed] item_usd`) and on `[self.budget]`, branch and day,
//!   before it is forwarded, and settles to what the provider's usage says. A
//!   call that would pass either line is refused, and the item is marked: its
//!   run is a failure ([`Item::refused`]). A call whose answer never reached
//!   its end settles at its worst case.
//! - **The meter is the cost**: what a run spent is [`Relay::finish`]'s
//!   figure, summed from these settles, never the candidate's report.
//! - **Usage only**: the log keeps a model, token counts, dollars and a
//!   latency ([`Relay::log`]); a request or a response body, which carries the
//!   task, is held in memory for the length of its call and written nowhere.

use std::net::TcpStream as StdStream;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use theseus_kernel::job_egress::{self, Local, Registered};
use theseus_protocol::Usage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::provider::{apply_usage, API_VERSION};
use crate::rpc::Core;
use crate::rsi::budget::Reserved;
use crate::rsi::Gate;

/// The longest request head, and body, the relay reads.
const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 32 * 1024 * 1024;
/// The log keeps this many usage lines, the newest.
const LOG_MAX: usize = 10_000;
/// The input estimate: a token for every this many bytes of a request, the
/// safe side of any text.
const BYTES_PER_TOKEN: usize = 3;

/// One call's usage line, the whole of what the relay records of it.
#[derive(Debug, Clone, PartialEq)]
pub struct UsageLine {
    pub model: String,
    pub usage: Usage,
    pub usd: f64,
    pub ms: u64,
    pub status: u16,
}

impl std::fmt::Display for UsageLine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "sealed relay: model={} input={} output={} cache_read={} cache_write={} usd={:.6} ms={} status={}",
            self.model,
            self.usage.input_tokens,
            self.usage.output_tokens,
            self.usage.cache_read_input_tokens,
            self.usage.cache_creation_input_tokens,
            self.usd,
            self.ms,
            self.status
        )
    }
}

/// What an item spent, and whether the relay stopped it.
#[derive(Debug, Clone, PartialEq)]
pub struct Metered {
    pub usd: f64,
    pub calls: u64,
    /// A call was refused at a line or by the gate: the run is a failure.
    pub refused: bool,
}

/// The item being run: the relay serves one at a time.
struct Item {
    cap_micros: u64,
    spent_micros: u64,
    /// Held by calls in flight.
    open_micros: u64,
    calls: u64,
    refused: bool,
}

#[derive(Default)]
struct State {
    item: Option<Item>,
    total_micros: u64,
    log: Vec<UsageLine>,
}

/// A request the relay refuses: the status, the API's error type, and a fixed
/// sentence that carries nothing of the request.
struct Refusal {
    code: u16,
    kind: &'static str,
    why: &'static str,
    /// Stops the item: its run counts as failed.
    stops: bool,
}

const fn refusal(code: u16, kind: &'static str, why: &'static str, stops: bool) -> Refusal {
    Refusal {
        code,
        kind,
        why,
        stops,
    }
}

const NOT_FOUND: Refusal = refusal(
    404,
    "not_found_error",
    "the relay serves /v1/messages",
    false,
);
const BAD: Refusal = refusal(
    400,
    "invalid_request_error",
    "the request is not a Messages request",
    false,
);
const WRONG_MODEL: Refusal = refusal(403, "permission_error", "the relay serves one model", false);
const HALTED: Refusal = refusal(
    403,
    "permission_error",
    "self-improvement is off or halted",
    true,
);
const NO_ITEM: Refusal = refusal(403, "permission_error", "no sealed item is running", false);
const ITEM_LINE: Refusal = refusal(
    403,
    "permission_error",
    "the item's line stops this call",
    true,
);
const BUDGET_LINE: Refusal = refusal(
    403,
    "permission_error",
    "the self budget's line stops this call",
    true,
);
const NO_KEY: Refusal = refusal(500, "api_error", "the relay has no key", false);

/// The relay for one scoring run: one profile, one branch.
pub struct Relay {
    core: Weak<Core>,
    rt: tokio::runtime::Handle,
    http: reqwest::Client,
    model: String,
    upstream: String,
    key_secret: String,
    branch: String,
    host: String,
    state: Mutex<State>,
}

impl Relay {
    /// The relay for `profile` on `branch`, naming itself `host`
    /// (`<name>:80`'s name): the profile's model is the only one it serves,
    /// its provider the upstream. Call it from the runtime that will serve it.
    pub fn new(core: &Arc<Core>, profile: &str, branch: &str, host: &str) -> Result<Arc<Self>> {
        let p = core.cfg.profile(profile)?;
        let provider = core
            .cfg
            .all_providers()
            .get(&p.provider)
            .with_context(|| format!("profile {profile} names no provider"))?;
        Ok(Arc::new(Self {
            core: Arc::downgrade(core),
            rt: tokio::runtime::Handle::current(),
            http: reqwest::Client::builder()
                .user_agent(format!("theseus/{}", crate::VERSION))
                .build()?,
            model: p.model.clone(),
            upstream: provider.api_base.trim_end_matches('/').to_string(),
            key_secret: provider.api_key_secret.clone(),
            branch: branch.to_string(),
            host: host.to_string(),
            state: Mutex::default(),
        }))
    }

    /// The model it serves.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Its name on a job's egress list.
    pub fn entry(&self) -> String {
        format!("{}:80", self.host)
    }

    /// The `model.api_base` that reaches it from inside the job.
    pub fn url(&self) -> String {
        format!("http://{}", self.host)
    }

    /// Serve it to jobs of this process until the registration is dropped.
    pub fn serve(self: &Arc<Self>) -> Registered {
        job_egress::register(&self.entry(), Arc::new(Serving(self.clone())))
    }

    /// Serve one connection from a job, on the runtime: what a registered
    /// relay does with each connection its job's listener accepts.
    pub fn serve_connection(self: &Arc<Self>, conn: StdStream) {
        let relay = self.clone();
        self.rt.spawn(async move {
            if conn.set_nonblocking(true).is_err() {
                return;
            }
            if let Ok(conn) = TcpStream::from_std(conn) {
                relay.serve_one(conn).await;
            }
        });
    }

    /// Open an item whose calls may spend `cap_usd` in all.
    pub fn begin(&self, cap_usd: f64) {
        self.state().item = Some(Item {
            cap_micros: theseus_kernel::usd_to_micros(cap_usd),
            spent_micros: 0,
            open_micros: 0,
            calls: 0,
            refused: false,
        });
    }

    /// Close the item: what its calls spent, from the meter.
    pub fn finish(&self) -> Metered {
        let item = self.state().item.take();
        item.map_or(
            Metered {
                usd: 0.0,
                calls: 0,
                refused: false,
            },
            |i| Metered {
                usd: micros_usd(i.spent_micros),
                calls: i.calls,
                refused: i.refused,
            },
        )
    }

    /// Everything the run spent so far, from the meter.
    pub fn spent_usd(&self) -> f64 {
        micros_usd(self.state().total_micros)
    }

    /// The usage lines, oldest first: the relay's whole log.
    pub fn log(&self) -> Vec<UsageLine> {
        self.state().log.clone()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn serve_one(&self, mut conn: TcpStream) {
        match self.handle(&mut conn).await {
            Ok(()) => {}
            Err(r) => {
                if r.stops {
                    self.stop_item();
                }
                respond(&mut conn, &r).await;
            }
        }
        let _ = conn.shutdown().await;
    }

    async fn handle(&self, conn: &mut TcpStream) -> Result<(), Refusal> {
        let req = read_request(conn).await.ok_or(BAD)?;
        if !req.target.ends_with("/v1/messages") || req.method != "POST" {
            return Err(NOT_FOUND);
        }
        let body: Value = serde_json::from_slice(&req.body).map_err(|_| BAD)?;
        if body["model"].as_str() != Some(self.model.as_str()) {
            return Err(WRONG_MODEL);
        }
        let max_tokens = body["max_tokens"].as_u64().ok_or(BAD)?;
        let core = self.core.upgrade().ok_or(HALTED)?;
        if !matches!(crate::rsi::gate(&core.self_ctx()), Gate::Allowed) {
            return Err(HALTED);
        }
        let entry = core.catalog.get(&self.model).ok_or(BAD)?;
        let worst = entry.reserve_micros(
            u32::try_from(max_tokens).unwrap_or(u32::MAX),
            req.body.len().div_ceil(BYTES_PER_TOKEN) as u64,
        );
        self.hold_on_item(worst)?;
        let reservation =
            match core
                .runner
                .self_budget
                .reserve("sealed", Some(&self.branch), micros_usd(worst))
            {
                Ok(Reserved::Granted(r)) => r,
                Ok(Reserved::Stopped(_)) | Err(_) => {
                    self.release_on_item(worst, 0, false);
                    return Err(BUDGET_LINE);
                }
            };
        let started = Instant::now();
        let (status, usage, whole) = match self.forward(&core, &req, conn).await {
            Ok(sent) => sent,
            Err(r) => {
                let _ = core.runner.self_budget.settle(reservation, 0.0);
                self.release_on_item(worst, 0, false);
                return Err(r);
            }
        };
        // A call that never reached its end settles at its worst case.
        let paid = if whole {
            entry.cost_micros(&usage)
        } else {
            worst
        };
        let _ = core
            .runner
            .self_budget
            .settle(reservation, micros_usd(paid));
        self.release_on_item(worst, paid, true);
        self.record(UsageLine {
            model: self.model.clone(),
            usd: micros_usd(paid),
            usage,
            ms: started.elapsed().as_millis() as u64,
            status,
        });
        Ok(())
    }

    /// Hold `worst` on the item's line, or refuse.
    fn hold_on_item(&self, worst: u64) -> Result<(), Refusal> {
        let mut st = self.state();
        let Some(item) = st.item.as_mut() else {
            return Err(NO_ITEM);
        };
        if item.spent_micros + item.open_micros + worst > item.cap_micros {
            return Err(ITEM_LINE);
        }
        item.open_micros += worst;
        Ok(())
    }

    /// A call is over: its hold goes, and what it paid (when it counted) is spent.
    fn release_on_item(&self, held: u64, paid: u64, counted: bool) {
        let mut st = self.state();
        st.total_micros += paid;
        if let Some(item) = st.item.as_mut() {
            item.open_micros = item.open_micros.saturating_sub(held);
            item.spent_micros += paid;
            item.calls += u64::from(counted);
        }
    }

    fn stop_item(&self) {
        if let Some(item) = self.state().item.as_mut() {
            item.refused = true;
        }
    }

    fn record(&self, line: UsageLine) {
        tracing::info!("{line}");
        let mut st = self.state();
        if st.log.len() == LOG_MAX {
            st.log.remove(0);
        }
        st.log.push(line);
    }

    /// Send the request to the provider with the daemon's key and stream the
    /// answer back to the candidate, metering it as it passes. The status, the
    /// usage seen, and whether the answer ran to its end.
    async fn forward(
        &self,
        core: &Core,
        req: &Request,
        conn: &mut TcpStream,
    ) -> Result<(u16, Usage, bool), Refusal> {
        let key = core.secrets.get(&self.key_secret).ok_or(NO_KEY)?;
        let mut post = self
            .http
            .post(format!("{}/v1/messages", self.upstream))
            .header("x-api-key", key.expose())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .timeout(Duration::from_secs(core.cfg.route_bench.whole_seconds))
            .body(req.body.clone());
        if let Some(beta) = &req.beta {
            post = post.header("anthropic-beta", beta);
        }
        let mut resp = match post.send().await {
            Ok(resp) => resp,
            Err(e) => {
                tracing::warn!(
                    "the sealed relay's provider did not answer: {}",
                    e.without_url()
                );
                return Err(refusal(
                    502,
                    "api_error",
                    "the provider did not answer",
                    false,
                ));
            }
        };
        let status = resp.status().as_u16();
        let kind = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json")
            .to_string();
        let head = format!(
            "HTTP/1.1 {status} {}\r\ncontent-type: {kind}\r\nconnection: close\r\n\r\n",
            resp.status().canonical_reason().unwrap_or("")
        );
        let mut meter = Meter::new(kind.starts_with("text/event-stream"));
        if conn.write_all(head.as_bytes()).await.is_err() {
            return Ok((status, Usage::default(), false));
        }
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    meter.feed(&chunk);
                    if conn.write_all(&chunk).await.is_err() {
                        return Ok((status, meter.usage, false));
                    }
                }
                Ok(None) => break,
                Err(_) => return Ok((status, meter.usage, false)),
            }
        }
        // An error answer carries no usage and costs nothing.
        let whole = meter.whole() || status >= 400;
        Ok((status, meter.usage, whole))
    }
}

/// The relay as a job's local service.
struct Serving(Arc<Relay>);

impl Local for Serving {
    fn serve(&self, conn: StdStream) {
        self.0.serve_connection(conn);
    }
}

fn micros_usd(m: u64) -> f64 {
    m as f64 / theseus_kernel::MICROS_PER_USD as f64
}

/// A read request: what the relay needs of it.
struct Request {
    method: String,
    target: String,
    beta: Option<String>,
    body: Vec<u8>,
}

/// The request's head and body, or none when it is not a well-formed request
/// within the caps.
async fn read_request(conn: &mut TcpStream) -> Option<Request> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD {
            return None;
        }
        let n = conn.read(&mut chunk).await.ok().filter(|&n| n > 0)?;
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, target) = (first.next()?.to_string(), first.next()?.to_string());
    let (mut length, mut beta) = (0usize, None);
    for line in lines {
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        match k.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = v.trim().parse().ok()?,
            "anthropic-beta" => beta = Some(v.trim().to_string()),
            _ => {}
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = buf.split_off(end);
    while body.len() < length {
        let n = conn.read(&mut chunk).await.ok().filter(|&n| n > 0)?;
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Some(Request {
        method,
        target,
        beta,
        body,
    })
}

async fn respond(conn: &mut TcpStream, r: &Refusal) {
    let body = json!({"type": "error", "error": {"type": r.kind, "message": r.why}}).to_string();
    let head = format!(
        "HTTP/1.1 {} Refused\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        r.code,
        body.len()
    );
    let _ = conn.write_all(head.as_bytes()).await;
    let _ = conn.write_all(body.as_bytes()).await;
}

/// The usage an answer carries, read as it passes: an event stream's
/// `message_start` and `message_delta`, or a JSON answer's own `usage`.
struct Meter {
    stream: bool,
    pending: Vec<u8>,
    usage: Usage,
    ended: bool,
}

impl Meter {
    fn new(stream: bool) -> Self {
        Self {
            stream,
            pending: Vec::new(),
            usage: Usage::default(),
            ended: false,
        }
    }

    fn feed(&mut self, chunk: &[u8]) {
        if self.pending.len() + chunk.len() > MAX_BODY {
            self.pending.clear();
        }
        self.pending.extend_from_slice(chunk);
        if !self.stream {
            return;
        }
        while let Some(i) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=i).collect();
            if let Some(event) = line
                .strip_prefix(b"data:")
                .and_then(|d| serde_json::from_slice::<Value>(d.trim_ascii()).ok())
            {
                self.event(&event);
            }
        }
    }

    fn event(&mut self, e: &Value) {
        match e["type"].as_str() {
            Some("message_start") => apply_usage(&mut self.usage, &e["message"]["usage"]),
            Some("message_delta") => {
                apply_usage(&mut self.usage, &e["usage"]);
                self.ended = true;
            }
            _ => {}
        }
    }

    /// Whether the answer ran to the usage that closes it.
    fn whole(&mut self) -> bool {
        if !self.stream {
            if let Ok(v) = serde_json::from_slice::<Value>(&self.pending) {
                apply_usage(&mut self.usage, &v["usage"]);
                self.ended = v["usage"].is_object();
            }
        }
        self.ended
    }
}
