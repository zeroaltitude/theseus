//! The sender: one task per pipeline, fed by the turns through a bounded
//! queue and by the metrics' interval. A turn records its metrics and hands
//! its trace over; it never waits for the network. The sender posts each
//! trace, and the metrics every interval, as OTLP/HTTP JSON over the
//! workspace's reqwest. A failure, a 429, or a 5xx is retried once after a
//! backoff; then the batch is dropped and counted, as is the oldest trace
//! when the queue is full. Health reports what was sent, what was dropped,
//! and the last error.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde_json::Value;
use theseus_protocol::{Span, TelemetryStatus};
use tokio::sync::{watch, Notify};

use super::metrics::Metrics;
use super::otlp::{self, KeyValue};
use super::{spans, TelemetryConfig};
use crate::secrets::Secret;

/// How long the sender waits before its one retry.
const BACKOFF: Duration = Duration::from_secs(1);
/// Traces waiting at most; past it the oldest is dropped.
const QUEUE: usize = 64;

/// How the sender runs: from the config, with the constants above. Tests
/// shorten them.
#[derive(Debug, Clone)]
pub(super) struct Tuning {
    pub interval: Duration,
    pub timeout: Duration,
    pub backoff: Duration,
    pub queue: usize,
}

impl Tuning {
    pub(super) fn from_config(cfg: &TelemetryConfig) -> Self {
        Self {
            interval: Duration::from_secs(cfg.metrics_interval_secs.max(1)),
            timeout: Duration::from_secs(cfg.export_timeout_secs.max(1)),
            backoff: BACKOFF,
            queue: QUEUE,
        }
    }
}

#[derive(Debug, Default)]
struct Counts {
    traces_sent: u64,
    metrics_sent: u64,
    spans_sent: u64,
    dropped: u64,
    last_error: Option<(String, u64)>,
    /// Since the last success: a failure is logged once, and a recovery.
    failing: bool,
}

/// What the turns and health share with the sender.
pub(super) struct Shared {
    endpoint: String,
    queue: Mutex<VecDeque<Span>>,
    cap: usize,
    wake: Notify,
    pub(super) metrics: Mutex<Metrics>,
    counts: Mutex<Counts>,
    /// Flushes asked for, and the last one the sender finished.
    asked: AtomicU64,
    done: watch::Sender<u64>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

impl Shared {
    /// A finished trace for the sender. When the queue is full its oldest
    /// is dropped, and counted.
    pub(super) fn push(&self, trace: Span) {
        let overflow = {
            let mut q = lock(&self.queue);
            q.push_back(trace);
            q.len() > self.cap && q.pop_front().is_some()
        };
        if overflow {
            self.dropped("the queue was full, so its oldest trace was dropped".into());
        }
        self.wake.notify_one();
    }

    fn pop(&self) -> Option<Span> {
        lock(&self.queue).pop_front()
    }

    fn dropped(&self, why: String) {
        let mut c = lock(&self.counts);
        c.dropped += 1;
        if !c.failing {
            c.failing = true;
            tracing::warn!(endpoint = %self.endpoint, error = %why, "telemetry: a batch was dropped; health counts any more");
        }
        c.last_error = Some((why, theseus_protocol::now_unix_ms()));
    }

    fn sent(&self, traces: u64, metrics: u64, spans: u64) {
        let mut c = lock(&self.counts);
        c.traces_sent += traces;
        c.metrics_sent += metrics;
        c.spans_sent += spans;
        if c.failing {
            c.failing = false;
            tracing::info!(endpoint = %self.endpoint, dropped = c.dropped, "telemetry: the receiver takes batches again");
        }
    }

    /// A batch the receiver took only in part: sent, and the reason kept.
    fn partly(&self, why: String) {
        lock(&self.counts).last_error = Some((why, theseus_protocol::now_unix_ms()));
    }

    /// Send what waits, and the metrics, now: true once done, false when
    /// `within` passed first.
    pub(super) async fn flush(&self, within: Duration) -> bool {
        let n = self.asked.fetch_add(1, Ordering::AcqRel) + 1;
        let mut done = self.done.subscribe();
        self.wake.notify_one();
        let finished =
            tokio::time::timeout(within, async { done.wait_for(|d| *d >= n).await.is_ok() }).await;
        finished.unwrap_or(false)
    }

    pub(super) fn status(&self) -> TelemetryStatus {
        let queued = lock(&self.queue).len() as u64;
        let c = lock(&self.counts);
        TelemetryStatus {
            enabled: true,
            otlp_endpoint: Some(self.endpoint.clone()),
            state: "exporting".into(),
            detail: None,
            traces_sent: c.traces_sent,
            metrics_sent: c.metrics_sent,
            spans_sent: c.spans_sent,
            dropped: c.dropped,
            queued,
            last_error: c.last_error.as_ref().map(|(e, _)| e.clone()),
            last_error_at_ms: c.last_error.as_ref().map(|(_, at)| *at),
        }
    }
}

#[cfg(test)]
impl Shared {
    /// A pipeline's shared half with no sender: what waits stays.
    pub(super) fn for_tests(cap: usize) -> Self {
        Self::new("http://127.0.0.1:9", cap)
    }

    pub(super) fn queued(&self) -> Vec<Span> {
        lock(&self.queue).iter().cloned().collect()
    }
}

impl Shared {
    fn new(endpoint: &str, cap: usize) -> Self {
        let (done, _) = watch::channel(0);
        Self {
            endpoint: endpoint.to_string(),
            queue: Mutex::new(VecDeque::new()),
            cap: cap.max(1),
            wake: Notify::new(),
            metrics: Mutex::new(Metrics::new(now_ns())),
            counts: Mutex::new(Counts::default()),
            asked: AtomicU64::new(0),
            done,
        }
    }
}

/// Build the pipeline and start its sender (inside a tokio runtime).
pub(super) fn start(
    cfg: &TelemetryConfig,
    endpoint: &str,
    headers: Option<&Secret>,
    tuning: Tuning,
) -> Result<Arc<Shared>> {
    let url = reqwest::Url::parse(endpoint)
        .with_context(|| format!("telemetry.otlp_endpoint {endpoint:?} is not a URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("telemetry.otlp_endpoint {endpoint:?} is not http or https");
    }
    let headers = header_map(headers)?;
    let http = reqwest::Client::builder()
        .user_agent(format!("theseus/{}", crate::VERSION))
        .build()
        .context("building the OTLP client")?;
    let shared = Arc::new(Shared::new(endpoint, tuning.queue));
    let sender = Sender {
        shared: shared.clone(),
        http,
        traces_url: format!("{endpoint}/v1/traces"),
        metrics_url: format!("{endpoint}/v1/metrics"),
        headers,
        resource: resource(&cfg.service_name),
        tuning,
    };
    tokio::spawn(sender.run());
    Ok(shared)
}

/// The resource every batch carries.
fn resource(service_name: &str) -> otlp::Resource {
    otlp::Resource {
        attributes: vec![
            KeyValue::string("service.name", service_name),
            KeyValue::string("service.version", crate::VERSION),
            KeyValue::string("service.instance.id", crate::new_id("inst")),
            KeyValue::string("telemetry.sdk.name", crate::NAME),
            KeyValue::string("telemetry.sdk.language", "rust"),
            KeyValue::string("telemetry.sdk.version", crate::VERSION),
        ],
    }
}

/// `Header: value` per line, or `k=v,k2=v2`. Values are never logged.
pub(super) fn parse_headers(secret: &Secret) -> Vec<(String, String)> {
    let s = secret.expose();
    let mut out = Vec::new();
    let (items, sep): (Vec<&str>, char) = if s.contains('\n') || s.contains(": ") {
        (s.lines().collect(), ':')
    } else {
        (s.split(',').collect(), '=')
    };
    for item in items {
        if let Some((k, v)) = item.split_once(sep) {
            let (k, v) = (k.trim(), v.trim());
            if !k.is_empty() && !v.is_empty() {
                out.push((k.to_string(), v.to_string()));
            }
        }
    }
    out
}

/// The headers secret as request headers, each value marked sensitive. One
/// that is not a header refuses the pipeline, naming only the header.
fn header_map(secret: Option<&Secret>) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (k, v) in secret.map(parse_headers).unwrap_or_default() {
        let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| {
            anyhow!("the telemetry headers secret names {k:?}, which is not a header name")
        })?;
        let mut value = HeaderValue::from_str(&v).map_err(|_| {
            anyhow!("the telemetry headers secret's value for {k} is not a header value")
        })?;
        value.set_sensitive(true);
        map.insert(name, value);
    }
    Ok(map)
}

struct Sender {
    shared: Arc<Shared>,
    http: reqwest::Client,
    traces_url: String,
    metrics_url: String,
    headers: HeaderMap,
    resource: otlp::Resource,
    tuning: Tuning,
}

/// Why a post failed, and whether it is worth one retry.
struct Failure {
    retry: bool,
    why: String,
}

impl Sender {
    async fn run(self) {
        let every = self.tuning.interval;
        let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = self.shared.wake.notified() => {}
                _ = tick.tick() => self.metrics().await,
            }
            while let Some(trace) = self.shared.pop() {
                self.trace(&trace).await;
            }
            let asked = self.shared.asked.load(Ordering::Acquire);
            if asked > *self.shared.done.borrow() {
                self.metrics().await;
                self.shared.done.send_replace(asked);
            }
        }
    }

    async fn trace(&self, root: &Span) {
        let spans = spans::spans(root);
        let n = spans.len() as u64;
        let req = otlp::ExportTraceServiceRequest {
            resource_spans: vec![otlp::ResourceSpans {
                resource: self.resource.clone(),
                scope_spans: vec![otlp::ScopeSpans {
                    scope: otlp::Scope {
                        name: crate::NAME.into(),
                    },
                    spans,
                }],
            }],
        };
        if self.deliver(&self.traces_url, &req, "traces").await {
            self.shared.sent(1, 0, n);
        }
    }

    async fn metrics(&self) {
        let req = {
            let m = lock(&self.shared.metrics);
            if m.is_empty() {
                return;
            }
            m.request(self.resource.clone(), now_ns())
        };
        if self.deliver(&self.metrics_url, &req, "metrics").await {
            self.shared.sent(0, 1, 0);
        }
    }

    /// Post one batch, retrying once: true if the receiver took it.
    async fn deliver(&self, url: &str, req: &impl serde::Serialize, what: &str) -> bool {
        let body = match serde_json::to_vec(req) {
            Ok(b) => b,
            Err(e) => {
                self.shared.dropped(format!("{what}: encoding: {e}"));
                return false;
            }
        };
        let mut failure = match self.post(url, body.clone()).await {
            Ok(partly) => return self.took(partly),
            Err(f) => f,
        };
        if failure.retry {
            tokio::time::sleep(self.tuning.backoff).await;
            match self.post(url, body).await {
                Ok(partly) => return self.took(partly),
                Err(f) => {
                    failure = Failure {
                        retry: false,
                        why: format!("{}, after one retry", f.why),
                    }
                }
            }
        }
        self.shared.dropped(format!("{what}: {}", failure.why));
        false
    }

    fn took(&self, partly: Option<String>) -> bool {
        if let Some(why) = partly {
            self.shared.partly(why);
        }
        true
    }

    /// One POST. A 2xx is taken, perhaps only in part (the reason is
    /// returned); a 429 or a 5xx, or no answer, is worth a retry; any other
    /// status is not.
    async fn post(&self, url: &str, body: Vec<u8>) -> Result<Option<String>, Failure> {
        let resp = self
            .http
            .post(url)
            .headers(self.headers.clone())
            .header(CONTENT_TYPE, "application/json")
            .timeout(self.tuning.timeout)
            .body(body)
            .send()
            .await
            .map_err(|e| Failure {
                retry: true,
                why: if e.is_timeout() {
                    format!("no answer within {} s", self.tuning.timeout.as_secs_f64())
                } else {
                    format!("{:#}", anyhow::Error::new(e.without_url()))
                },
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Failure {
                retry: status.as_u16() == 429 || status.is_server_error(),
                why: format!("the receiver answered {status}"),
            });
        }
        let body = resp.bytes().await.unwrap_or_default();
        Ok(rejected(&body))
    }
}

/// A partial success's reason: `{"partialSuccess": {"rejectedSpans": "2",
/// "errorMessage": …}}` (or `rejectedDataPoints`).
pub(super) fn rejected(body: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let p = v.get("partialSuccess")?;
    let count = |k: &str| -> u64 {
        match p.get(k) {
            Some(Value::String(s)) => s.parse().unwrap_or(0),
            Some(n) => n.as_u64().unwrap_or(0),
            None => 0,
        }
    };
    let (n, what) = match (count("rejectedSpans"), count("rejectedDataPoints")) {
        (0, 0) => return None,
        (n, 0) => (n, "spans"),
        (_, n) => (n, "data points"),
    };
    Some(
        match p
            .get("errorMessage")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
        {
            Some(m) => format!("the receiver rejected {n} {what}: {m}"),
            None => format!("the receiver rejected {n} {what}"),
        },
    )
}
