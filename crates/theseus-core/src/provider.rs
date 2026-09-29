//! Direct Anthropic Messages API client with streaming (spec §3.6).
//!
//! Typed content blocks and stream events, classified errors with the
//! provider's rate-limit headers, and four timeouts (connect, first byte,
//! stream idle, total). No automatic retry: an interrupted or refused call is
//! a classified error the harness ledgers and decides about. The shapes here
//! were checked against the community `claude-sdk` crate's `types` and
//! `streaming` modules and the Messages API reference; nothing is imported.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::Usage;

use crate::secrets::{Secret, SecretBoard};

pub const API_VERSION: &str = "2023-06-01";

// ------------------------------------------------------------------ request

/// One provider call, as the compiler rendered it. Content blocks are JSON
/// values, not a Rust enum: the transcript replays assistant blocks exactly as
/// the provider returned them (thinking blocks and their signatures must come
/// back byte-for-byte, §4.4 and the preserved-thinking rules), and block kinds
/// Theseus does not model yet pass through untouched.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub model: String,
    pub max_tokens: u32,
    /// Top-level system blocks (`{"type":"text","text":…}`, with `cache_control` on the last).
    #[serde(default)]
    pub system: Vec<Value>,
    /// `{"role": "user"|"assistant", "content": [blocks]}`.
    #[serde(default)]
    pub messages: Vec<Value>,
    /// Wire tool definitions, sorted by name.
    #[serde(default)]
    pub tools: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_config: Option<Value>,
    /// Top-level automatic prompt caching.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<Value>,
    /// `anthropic-beta` values.
    #[serde(default)]
    pub betas: Vec<String>,
    /// Other top-level fields (e.g. `fallbacks`).
    #[serde(default)]
    pub extra: BTreeMap<String, Value>,
    /// What the request's images are estimated to cost (theseus-9g2), set
    /// by the compiler, which knows their sizes; never sent.
    #[serde(skip)]
    pub image_tokens: u64,
}

impl ProviderRequest {
    /// A one-message request (tests, probes).
    pub fn simple(model: &str, max_tokens: u32, user_text: &str) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            messages: vec![serde_json::json!({
                "role": "user",
                "content": [{"type": "text", "text": user_text}],
            })],
            ..Default::default()
        }
    }

    /// The JSON body sent to `/v1/messages`, without the `stream` flag.
    pub fn body(&self) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("model".into(), Value::String(self.model.clone()));
        m.insert("max_tokens".into(), Value::from(self.max_tokens));
        if !self.system.is_empty() {
            m.insert("system".into(), Value::Array(self.system.clone()));
        }
        m.insert("messages".into(), Value::Array(self.messages.clone()));
        if !self.tools.is_empty() {
            m.insert("tools".into(), Value::Array(self.tools.clone()));
        }
        if let Some(t) = &self.thinking {
            m.insert("thinking".into(), t.clone());
        }
        if let Some(o) = &self.output_config {
            m.insert("output_config".into(), o.clone());
        }
        if let Some(c) = &self.cache_control {
            m.insert("cache_control".into(), c.clone());
        }
        for (k, v) in &self.extra {
            m.insert(k.clone(), v.clone());
        }
        Value::Object(m)
    }

    /// sha256 of the body, keys sorted: what a reconstruction must reproduce (§4.4).
    pub fn digest(&self) -> String {
        theseus_kernel::digest_json(&self.body())
    }

    /// Rough size of the prompt in tokens (chars / 4) for budgeting. An
    /// image's base64 is not text the model reads: its characters are left
    /// out and its estimated tokens counted instead (theseus-9g2), or a
    /// 1 MB PNG would reserve about 333,000 tokens.
    pub fn estimate_tokens(&self) -> u64 {
        let chars = serde_json::to_string(&self.system)
            .map(|s| s.len())
            .unwrap_or(0)
            + serde_json::to_string(&self.tools)
                .map(|s| s.len())
                .unwrap_or(0)
            + serde_json::to_string(&self.messages)
                .map(|s| s.len())
                .unwrap_or(0);
        let data: usize = self
            .messages
            .iter()
            .map(|m| base64_chars(&m["content"]))
            .sum();
        (chars.saturating_sub(data) / 4) as u64 + self.image_tokens
    }
}

/// Characters of base64 image data in a message's content, tool results
/// included.
fn base64_chars(content: &Value) -> usize {
    content.as_array().map_or(0, |blocks| {
        blocks
            .iter()
            .map(|b| match b.get("type").and_then(Value::as_str) {
                Some("image") => b["source"]["data"].as_str().map_or(0, str::len),
                Some("tool_result") => base64_chars(&b["content"]),
                _ => 0,
            })
            .sum()
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timeouts {
    /// TCP + TLS establishment.
    pub connect_secs: u64,
    /// From request sent to response headers (the model has started).
    pub first_byte_secs: u64,
    /// Longest silence tolerated between stream events.
    pub stream_idle_secs: u64,
    /// Whole call, whatever it is doing.
    pub total_secs: u64,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect_secs: 10,
            first_byte_secs: 60,
            stream_idle_secs: 60,
            total_secs: 600,
        }
    }
}

// ----------------------------------------------------------------- response

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentDelta {
    TextDelta {
        text: String,
    },
    InputJsonDelta {
        partial_json: String,
    },
    ThinkingDelta {
        thinking: String,
    },
    SignatureDelta {
        signature: String,
    },
    CitationsDelta {
        citation: Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamErrorBody {
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub message: String,
}

/// Server-sent events on `/v1/messages` with `stream: true`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    MessageStart {
        message: Value,
    },
    ContentBlockStart {
        index: usize,
        content_block: Value,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        #[serde(default)]
        delta: Value,
        #[serde(default)]
        usage: Option<Value>,
    },
    MessageStop,
    Ping,
    Error {
        error: StreamErrorBody,
    },
    #[serde(other)]
    Unknown,
}

/// The rate-limit headers the API returns on every response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub requests_limit: Option<u64>,
    pub requests_remaining: Option<u64>,
    pub requests_reset: Option<String>,
    pub tokens_limit: Option<u64>,
    pub tokens_remaining: Option<u64>,
    pub tokens_reset: Option<String>,
    pub input_tokens_remaining: Option<u64>,
    pub output_tokens_remaining: Option<u64>,
    pub retry_after_secs: Option<u64>,
}

impl RateLimitInfo {
    pub fn from_headers(h: &reqwest::header::HeaderMap) -> Self {
        let s = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
        let n = |k: &str| s(k).and_then(|v| v.parse::<u64>().ok());
        Self {
            requests_limit: n("anthropic-ratelimit-requests-limit"),
            requests_remaining: n("anthropic-ratelimit-requests-remaining"),
            requests_reset: s("anthropic-ratelimit-requests-reset"),
            tokens_limit: n("anthropic-ratelimit-tokens-limit"),
            tokens_remaining: n("anthropic-ratelimit-tokens-remaining"),
            tokens_reset: s("anthropic-ratelimit-tokens-reset"),
            input_tokens_remaining: n("anthropic-ratelimit-input-tokens-remaining"),
            output_tokens_remaining: n("anthropic-ratelimit-output-tokens-remaining"),
            retry_after_secs: n("retry-after"),
        }
    }
}

/// Timing the ledger wants for every call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CallTiming {
    pub first_byte_ms: Option<u64>,
    pub first_token_ms: Option<u64>,
    pub total_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelResponse {
    /// Concatenated text blocks, in order.
    pub text: String,
    /// Every content block exactly as assembled from the stream.
    pub content: Vec<Value>,
    pub stop_reason: Option<String>,
    /// Populated when `stop_reason` is `refusal`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_details: Option<Value>,
    pub usage: Usage,
    /// The model that served the response (differs from the request on a fallback).
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    pub request_id: Option<String>,
    pub rate_limit: RateLimitInfo,
    pub timing: CallTiming,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_transformations: Option<Value>,
    /// tool_use ids whose streamed input did not parse as JSON, with the raw text.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub invalid_tool_inputs: BTreeMap<String, String>,
}

/// A `tool_use` block, extracted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// The `tool_use` blocks in a content list, in order.
pub fn tool_uses_in(blocks: &[Value]) -> Vec<ToolUse> {
    blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"))
        .map(|b| ToolUse {
            id: b
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: b
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input: b
                .get("input")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
        })
        .collect()
}

/// Concatenated `text` blocks.
pub fn text_of(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect()
}

/// Concatenated `thinking` text (summaries or progress updates; empty when omitted).
pub fn thinking_of(blocks: &[Value]) -> String {
    blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("thinking"))
        .filter_map(|b| b.get("thinking").and_then(Value::as_str))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

impl ModelResponse {
    pub fn tool_uses(&self) -> Vec<ToolUse> {
        tool_uses_in(&self.content)
    }
}

// ------------------------------------------------------------------- errors

/// Where in the call a timeout fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutPhase {
    Connect,
    FirstByte,
    StreamIdle,
    Total,
}

/// Classified provider failure. The class is what the ledger records and
/// what the harness reasons about; the message is for humans.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "class", rename_all = "snake_case")]
pub enum ProviderError {
    #[error("provider timeout ({phase:?}) after {elapsed_ms} ms")]
    Timeout {
        phase: TimeoutPhase,
        elapsed_ms: u64,
    },
    #[error("network error: {message}")]
    Network { message: String },
    #[error("rate limited (429): {message}")]
    RateLimited {
        message: String,
        retry_after_secs: Option<u64>,
        rate_limit: RateLimitInfo,
    },
    #[error("provider overloaded (529): {message}")]
    Overloaded { message: String },
    #[error("provider server error ({status}): {message}")]
    Server { status: u16, message: String },
    #[error("authentication failed ({status}): {message}")]
    Auth { status: u16, message: String },
    #[error("invalid request ({status}): {message}")]
    InvalidRequest { status: u16, message: String },
    #[error("api error ({status}): {message}")]
    Api { status: u16, message: String },
    #[error("stream error ({kind}): {message}")]
    Stream { kind: String, message: String },
    #[error("stream ended without message_stop after {elapsed_ms} ms")]
    Truncated { elapsed_ms: u64 },
}

impl ProviderError {
    pub fn class(&self) -> &'static str {
        match self {
            ProviderError::Timeout { .. } => "timeout",
            ProviderError::Network { .. } => "network",
            ProviderError::RateLimited { .. } => "rate_limited",
            ProviderError::Overloaded { .. } => "overloaded",
            ProviderError::Server { .. } => "server",
            ProviderError::Auth { .. } => "auth",
            ProviderError::InvalidRequest { .. } => "invalid_request",
            ProviderError::Api { .. } => "api",
            ProviderError::Stream { .. } => "stream",
            ProviderError::Truncated { .. } => "truncated",
        }
    }

    /// Whether a later identical call could plausibly succeed. This informs a
    /// human or a future policy; Theseus never retries on its own here.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ProviderError::Timeout { .. }
                | ProviderError::Network { .. }
                | ProviderError::RateLimited { .. }
                | ProviderError::Overloaded { .. }
                | ProviderError::Server { .. }
                | ProviderError::Truncated { .. }
        )
    }

    /// Whether the provider may have consumed tokens we cannot see. Spec §3.13:
    /// such calls hold their reservation until reconciled.
    pub fn usage_unknown(&self) -> bool {
        matches!(
            self,
            ProviderError::Timeout {
                phase: TimeoutPhase::StreamIdle | TimeoutPhase::Total,
                ..
            } | ProviderError::Truncated { .. }
                | ProviderError::Stream { .. }
        )
    }

    pub fn from_status(status: u16, body: &str, headers: &reqwest::header::HeaderMap) -> Self {
        let message = api_error_message(body);
        match status {
            429 => ProviderError::RateLimited {
                message,
                retry_after_secs: headers
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok()),
                rate_limit: RateLimitInfo::from_headers(headers),
            },
            529 => ProviderError::Overloaded { message },
            401 | 403 => ProviderError::Auth { status, message },
            400 | 404 | 413 | 422 => ProviderError::InvalidRequest { status, message },
            500..=599 => ProviderError::Server { status, message },
            _ => ProviderError::Api { status, message },
        }
    }
}

/// `{"type":"error","error":{"type":"...","message":"..."}}` → the message.
fn api_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            let e = v.get("error")?;
            let t = e.get("type").and_then(Value::as_str).unwrap_or("error");
            let m = e.get("message").and_then(Value::as_str).unwrap_or("");
            Some(format!("{t}: {m}"))
        })
        .unwrap_or_else(|| body.chars().take(400).collect())
}

// ------------------------------------------------------------------ trait

/// What streams to the caller while a response is generated.
#[derive(Debug, Clone, Copy)]
pub enum Delta<'a> {
    Text(&'a str),
    Thinking(&'a str),
    ToolUseStart { id: &'a str, name: &'a str },
}

pub type DeltaSink<'a> = &'a mut (dyn FnMut(Delta<'_>) + Send);
pub type ProviderFuture<'a> = futures_util::future::BoxFuture<'a, Result<ModelResponse>>;

/// A model provider. The turn runner depends on this, never on Anthropic
/// directly, so tests run against a fake and a second provider is a new impl.
/// Errors are `ProviderError` wrapped in `anyhow`; callers downcast to classify.
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a>;
}

// --------------------------------------------------------------- anthropic

#[derive(Clone)]
pub struct Anthropic {
    http: reqwest::Client,
    api_base: String,
    /// The key, read from the board at each call: the daemon serves before
    /// it resolves (theseus-qa0), and a turn waits for it before calling.
    secrets: Arc<SecretBoard>,
    key_secret: String,
    timeouts: Timeouts,
}

impl Anthropic {
    pub fn new(
        api_base: &str,
        secrets: Arc<SecretBoard>,
        key_secret: &str,
        timeouts: Timeouts,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("theseus/{}", crate::VERSION))
            .connect_timeout(Duration::from_secs(timeouts.connect_secs))
            .build()?;
        Ok(Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            secrets,
            key_secret: key_secret.to_string(),
            timeouts,
        })
    }

    /// The key, or why there is none. Fail closed: no call goes out without it.
    fn key(&self) -> Result<Secret> {
        self.secrets.get(&self.key_secret).ok_or_else(|| {
            anyhow::anyhow!(
                "secret {} has not resolved; a turn waits for it before calling",
                self.key_secret
            )
        })
    }

    async fn stream_message_impl(
        &self,
        req: &ProviderRequest,
        on_delta: DeltaSink<'_>,
    ) -> Result<ModelResponse> {
        let key = self.key()?;
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.timeouts.total_secs);
        let elapsed = |s: Instant| s.elapsed().as_millis() as u64;

        let mut body = req.body();
        if let Value::Object(m) = &mut body {
            m.insert("stream".into(), Value::Bool(true));
        }
        let mut post = self
            .http
            .post(format!("{}/v1/messages", self.api_base))
            .header("x-api-key", key.expose())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json");
        if !req.betas.is_empty() {
            post = post.header("anthropic-beta", req.betas.join(","));
        }
        let send = post.json(&body).send();

        // First byte: response headers within the budget.
        let first_byte_budget =
            Duration::from_secs(self.timeouts.first_byte_secs).min(deadline - started);
        let resp = match tokio::time::timeout(first_byte_budget, send).await {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => {
                let phase = if e.is_connect() {
                    Some(TimeoutPhase::Connect)
                } else {
                    None
                };
                return Err(match phase {
                    Some(p) if e.is_timeout() => ProviderError::Timeout {
                        phase: p,
                        elapsed_ms: elapsed(started),
                    },
                    _ => ProviderError::Network {
                        message: e.to_string(),
                    },
                }
                .into());
            }
            Err(_) => {
                return Err(ProviderError::Timeout {
                    phase: TimeoutPhase::FirstByte,
                    elapsed_ms: elapsed(started),
                }
                .into())
            }
        };
        let first_byte_ms = elapsed(started);
        let headers = resp.headers().clone();
        let status = resp.status();
        let request_id = headers
            .get("request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::from_status(status.as_u16(), &text, &headers).into());
        }

        let mut out = ModelResponse {
            model: req.model.clone(),
            request_id,
            rate_limit: RateLimitInfo::from_headers(&headers),
            timing: CallTiming {
                first_byte_ms: Some(first_byte_ms),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut acc = Accumulator::default();
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        let idle = Duration::from_secs(self.timeouts.stream_idle_secs);
        let mut saw_stop = false;

        'outer: loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(ProviderError::Timeout {
                    phase: TimeoutPhase::Total,
                    elapsed_ms: elapsed(started),
                }
                .into());
            }
            let budget = idle.min(deadline - now);
            let chunk = match tokio::time::timeout(budget, stream.next()).await {
                Ok(Some(Ok(c))) => c,
                Ok(Some(Err(e))) => {
                    return Err(ProviderError::Network {
                        message: format!("reading stream: {e}"),
                    }
                    .into())
                }
                Ok(None) => break,
                Err(_) => {
                    let phase = if Instant::now() >= deadline {
                        TimeoutPhase::Total
                    } else {
                        TimeoutPhase::StreamIdle
                    };
                    return Err(ProviderError::Timeout {
                        phase,
                        elapsed_ms: elapsed(started),
                    }
                    .into());
                }
            };
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim_end_matches('\r').to_string();
                buf.drain(..=pos);
                let Some(ev) = parse_sse_line(&line) else {
                    continue;
                };
                if out.timing.first_token_ms.is_none() {
                    if let StreamEvent::ContentBlockDelta { .. } = &ev {
                        out.timing.first_token_ms = Some(elapsed(started));
                    }
                }
                match acc.apply(ev, &mut out, on_delta)? {
                    Flow::Continue => {}
                    Flow::Stop => {
                        saw_stop = true;
                        break 'outer;
                    }
                }
            }
        }
        let (content, invalid) = acc.finish();
        out.content = content;
        out.invalid_tool_inputs = invalid;
        out.text = text_of(&out.content);
        out.timing.total_ms = elapsed(started);
        if !saw_stop {
            return Err(ProviderError::Truncated {
                elapsed_ms: out.timing.total_ms,
            }
            .into());
        }
        Ok(out)
    }
}

impl Provider for Anthropic {
    fn name(&self) -> &str {
        "anthropic"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(self.stream_message_impl(req, on_delta))
    }
}

/// `data: {...}` → event. `event:` lines and blanks are ignored (the JSON
/// carries its own `type`). Unparseable data is skipped, not fatal.
pub fn parse_sse_line(line: &str) -> Option<StreamEvent> {
    let data = line.strip_prefix("data:")?.trim();
    if data.is_empty() {
        return None;
    }
    serde_json::from_str(data).ok()
}

enum Flow {
    Continue,
    Stop,
}

/// Assembles content blocks from start/delta/stop events, as JSON values in
/// the shape the API would return them non-streamed. Tool input arrives as
/// partial JSON and is parsed once, strictly, at block stop; input that does
/// not parse is recorded (the harness answers it with an `INVALID_JSON` error
/// result) and never dispatched.
#[derive(Default)]
struct Accumulator {
    blocks: BTreeMap<usize, Value>,
    partial_json: BTreeMap<usize, String>,
    invalid: BTreeMap<String, String>,
}

fn push_str(block: &mut Value, key: &str, s: &str) {
    if let Value::Object(m) = block {
        match m.get_mut(key) {
            Some(Value::String(t)) => t.push_str(s),
            _ => {
                m.insert(key.into(), Value::String(s.into()));
            }
        }
    }
}

impl Accumulator {
    fn apply(
        &mut self,
        ev: StreamEvent,
        out: &mut ModelResponse,
        on_delta: &mut (dyn FnMut(Delta<'_>) + Send),
    ) -> Result<Flow> {
        match ev {
            StreamEvent::MessageStart { message } => {
                if let Some(u) = message.get("usage") {
                    apply_usage(&mut out.usage, u);
                }
                if let Some(m) = message.get("model").and_then(Value::as_str) {
                    out.model = m.to_string();
                }
                if let Some(id) = message.get("id").and_then(Value::as_str) {
                    out.message_id = Some(id.to_string());
                }
                if let Some(t) = message.get("input_transformations") {
                    out.input_transformations = Some(t.clone());
                }
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                if content_block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    let id = content_block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let name = content_block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    on_delta(Delta::ToolUseStart { id, name });
                }
                self.blocks.insert(index, content_block);
            }
            StreamEvent::ContentBlockDelta { index, delta } => {
                let block = self
                    .blocks
                    .entry(index)
                    .or_insert_with(|| serde_json::json!({"type": "text", "text": ""}));
                match delta {
                    ContentDelta::TextDelta { text } => {
                        push_str(block, "text", &text);
                        on_delta(Delta::Text(&text));
                    }
                    ContentDelta::InputJsonDelta { partial_json } => {
                        self.partial_json
                            .entry(index)
                            .or_default()
                            .push_str(&partial_json);
                    }
                    ContentDelta::ThinkingDelta { thinking } => {
                        push_str(block, "thinking", &thinking);
                        on_delta(Delta::Thinking(&thinking));
                    }
                    ContentDelta::SignatureDelta { signature } => {
                        push_str(block, "signature", &signature);
                    }
                    ContentDelta::CitationsDelta { citation } => {
                        if let Value::Object(m) = block {
                            match m.get_mut("citations") {
                                Some(Value::Array(a)) => a.push(citation),
                                _ => {
                                    m.insert("citations".into(), Value::Array(vec![citation]));
                                }
                            }
                        }
                    }
                    ContentDelta::Unknown => {}
                }
            }
            StreamEvent::ContentBlockStop { index } => {
                if let Some(json) = self.partial_json.remove(&index) {
                    if let Some(block @ Value::Object(_)) = self.blocks.get_mut(&index) {
                        if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                            let parsed = if json.trim().is_empty() {
                                Ok(Value::Object(Default::default()))
                            } else {
                                serde_json::from_str::<Value>(&json)
                            };
                            match parsed {
                                Ok(v @ Value::Object(_)) => block["input"] = v,
                                _ => {
                                    let id = block
                                        .get("id")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string();
                                    block["input"] = Value::Object(Default::default());
                                    self.invalid.insert(id, json);
                                }
                            }
                        }
                    }
                }
            }
            StreamEvent::MessageDelta { delta, usage } => {
                if let Some(s) = delta.get("stop_reason").and_then(Value::as_str) {
                    out.stop_reason = Some(s.to_string());
                }
                if let Some(d) = delta.get("stop_details").filter(|d| !d.is_null()) {
                    out.stop_details = Some(d.clone());
                }
                if let Some(u) = usage {
                    apply_usage(&mut out.usage, &u);
                }
            }
            StreamEvent::MessageStop => return Ok(Flow::Stop),
            StreamEvent::Ping | StreamEvent::Unknown => {}
            StreamEvent::Error { error } => {
                return Err(ProviderError::Stream {
                    kind: error.kind,
                    message: error.message,
                }
                .into())
            }
        }
        Ok(Flow::Continue)
    }

    fn finish(self) -> (Vec<Value>, BTreeMap<String, String>) {
        (self.blocks.into_values().collect(), self.invalid)
    }
}

fn apply_usage(u: &mut Usage, v: &Value) {
    let g = |k: &str| v.get(k).and_then(Value::as_u64);
    if let Some(x) = g("input_tokens") {
        u.input_tokens = x;
    }
    if let Some(x) = g("output_tokens") {
        u.output_tokens = x;
    }
    if let Some(x) = g("cache_read_input_tokens") {
        u.cache_read_input_tokens = x;
    }
    if let Some(x) = g("cache_creation_input_tokens") {
        u.cache_creation_input_tokens = x;
    }
}

// -------------------------------------------------------------------- fake

/// One scripted provider response.
#[derive(Debug, Clone)]
pub enum Scripted {
    /// Return these blocks (text blocks stream as deltas) with this stop reason.
    Blocks {
        blocks: Vec<Value>,
        stop_reason: String,
    },
    Fail(ProviderError),
    /// `then`, billed with exactly this usage (cache reads and writes too).
    Billed {
        usage: Usage,
        then: Box<Scripted>,
    },
}

impl Scripted {
    pub fn text(t: &str) -> Self {
        Scripted::Blocks {
            blocks: vec![serde_json::json!({"type": "text", "text": t})],
            stop_reason: "end_turn".into(),
        }
    }
    /// One or more tool calls: `(id, wire name, input)`, optionally after some text.
    pub fn tools(text: &str, calls: &[(&str, &str, Value)]) -> Self {
        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(serde_json::json!({"type": "text", "text": text}));
        }
        for (id, name, input) in calls {
            blocks.push(
                serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input}),
            );
        }
        Scripted::Blocks {
            blocks,
            stop_reason: "tool_use".into(),
        }
    }
}

/// A provider for tests: scripted responses consumed in order, then `reply`
/// forever; or `fail_with` on every call. Every request is recorded.
pub struct FakeProvider {
    pub reply: String,
    pub chunk: usize,
    pub stop_reason: String,
    pub fail_with: Option<ProviderError>,
    pub script: std::sync::Mutex<std::collections::VecDeque<Scripted>>,
    pub requests: std::sync::Mutex<Vec<ProviderRequest>>,
    /// Artificial latency before the response, for concurrency tests.
    pub delay_ms: u64,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self {
            reply: "fake reply".into(),
            chunk: 4,
            stop_reason: "end_turn".into(),
            fail_with: None,
            script: Default::default(),
            requests: Default::default(),
            delay_ms: 0,
        }
    }
}

impl FakeProvider {
    pub fn scripted(script: Vec<Scripted>) -> Self {
        Self {
            script: std::sync::Mutex::new(script.into()),
            ..Default::default()
        }
    }
    pub fn requests(&self) -> Vec<ProviderRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl Provider for FakeProvider {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: &'a ProviderRequest,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(req.clone());
            if self.delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
            }
            if let Some(e) = &self.fail_with {
                return Err(e.clone().into());
            }
            let mut next = self.script.lock().unwrap().pop_front();
            let mut billed = None;
            if let Some(Scripted::Billed { usage, then }) = next {
                billed = Some(usage);
                next = Some(*then);
            }
            let (blocks, stop_reason) = match next {
                Some(Scripted::Fail(e)) => return Err(e.into()),
                Some(Scripted::Blocks {
                    blocks,
                    stop_reason,
                }) => (blocks, stop_reason),
                Some(Scripted::Billed { .. }) => unreachable!("one bill per response"),
                None => (
                    vec![serde_json::json!({"type": "text", "text": self.reply})],
                    self.stop_reason.clone(),
                ),
            };
            for b in &blocks {
                match b.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let t = b.get("text").and_then(Value::as_str).unwrap_or_default();
                        let chars: Vec<char> = t.chars().collect();
                        for piece in chars.chunks(self.chunk.max(1)) {
                            let s: String = piece.iter().collect();
                            on_delta(Delta::Text(&s));
                            tokio::task::yield_now().await;
                        }
                    }
                    Some("tool_use") => {
                        let id = b.get("id").and_then(Value::as_str).unwrap_or_default();
                        let name = b.get("name").and_then(Value::as_str).unwrap_or_default();
                        on_delta(Delta::ToolUseStart { id, name });
                    }
                    _ => {}
                }
            }
            let text = text_of(&blocks);
            Ok(ModelResponse {
                usage: billed.unwrap_or_else(|| Usage {
                    input_tokens: req.estimate_tokens(),
                    output_tokens: (text.split_whitespace().count() as u64).max(1),
                    ..Default::default()
                }),
                text,
                content: blocks,
                stop_reason: Some(stop_reason),
                model: req.model.clone(),
                message_id: Some(crate::new_id("msg_fake")),
                request_id: Some("req_fake".into()),
                timing: CallTiming {
                    first_byte_ms: Some(1),
                    first_token_ms: Some(2),
                    total_ms: 3,
                },
                ..Default::default()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(lines: &[&str]) -> (ModelResponse, Vec<String>, Option<ProviderError>) {
        let mut out = ModelResponse::default();
        let mut acc = Accumulator::default();
        let mut deltas = Vec::new();
        let mut sink = |d: Delta<'_>| {
            if let Delta::Text(t) = d {
                deltas.push(t.to_string())
            }
        };
        let mut err = None;
        for l in lines {
            if let Some(ev) = parse_sse_line(l) {
                match acc.apply(ev, &mut out, &mut sink) {
                    Ok(Flow::Stop) => break,
                    Ok(Flow::Continue) => {}
                    Err(e) => {
                        err = e.downcast::<ProviderError>().ok();
                        break;
                    }
                }
            }
        }
        let (content, invalid) = acc.finish();
        out.content = content;
        out.invalid_tool_inputs = invalid;
        (out, deltas, err)
    }

    #[test]
    fn assembles_text_and_usage_from_sse() {
        let (out, deltas, err) = run(&[
            "event: message_start",
            r#"data: {"type":"message_start","message":{"model":"claude-x","usage":{"input_tokens":12,"cache_read_input_tokens":4}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
            r#"data: {"type":"ping"}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}"#,
            r#"data: {"type":"message_stop"}"#,
        ]);
        assert!(err.is_none());
        assert_eq!(deltas, vec!["Hel", "lo"]);
        assert_eq!(
            out.content,
            vec![serde_json::json!({"type": "text", "text": "Hello"})]
        );
        assert_eq!(out.model, "claude-x");
        assert_eq!(out.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(out.usage.input_tokens, 12);
        assert_eq!(out.usage.output_tokens, 2);
        assert_eq!(out.usage.cache_read_input_tokens, 4);
    }

    #[test]
    fn assembles_tool_use_input_from_partial_json() {
        let (out, _, err) = run(&[
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"bash","input":{}}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\": \"ls"}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":" -la\"}"}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}"#,
            r#"data: {"type":"message_stop"}"#,
        ]);
        assert!(err.is_none());
        let tu = out.tool_uses();
        assert_eq!(tu.len(), 1);
        assert_eq!(tu[0].name, "bash");
        assert_eq!(tu[0].input["cmd"], "ls -la");
        assert_eq!(out.stop_reason.as_deref(), Some("tool_use"));
    }

    #[test]
    fn thinking_blocks_keep_their_signature_verbatim_and_bad_tool_json_is_recorded() {
        let (out, _, err) = run(&[
            r#"data: {"type":"message_start","message":{"id":"msg_1","model":"claude-y","usage":{"input_tokens":3}}}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Plan."}}"#,
            r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"c2ln"}}"#,
            r#"data: {"type":"content_block_stop","index":0}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_9","name":"fs_read","input":{}}}"#,
            r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\": \"/x"}}"#,
            r#"data: {"type":"content_block_stop","index":1}"#,
            r#"data: {"type":"message_delta","delta":{"stop_reason":"max_tokens"},"usage":{"output_tokens":7}}"#,
            r#"data: {"type":"message_stop"}"#,
        ]);
        assert!(err.is_none());
        assert_eq!(out.message_id.as_deref(), Some("msg_1"));
        assert_eq!(
            out.content[0],
            serde_json::json!({"type": "thinking", "thinking": "Plan.", "signature": "c2ln"})
        );
        assert_eq!(out.content[1]["input"], serde_json::json!({}));
        assert_eq!(
            out.invalid_tool_inputs.get("toolu_9").map(String::as_str),
            Some("{\"path\": \"/x")
        );
        assert_eq!(thinking_of(&out.content), "Plan.");
    }

    #[test]
    fn request_body_digest_is_stable_and_betas_stay_out_of_the_body() {
        let mut r = ProviderRequest::simple("m", 10, "hi");
        r.betas = vec!["b1".into()];
        r.cache_control = Some(serde_json::json!({"type": "ephemeral"}));
        let b = r.body();
        assert!(b.get("betas").is_none());
        assert_eq!(b["cache_control"]["type"], "ephemeral");
        assert_eq!(r.clone().digest(), r.digest());
        // The digest the replaced sorted-key serializer gave (at 8a1e41d): a
        // stored `request_digest` must still match its reconstruction.
        assert_eq!(
            r.digest(),
            "6af8d83bc2dad527fe40c29da569472410ddfb97c810fc3fdb43dd4c99eba80f"
        );
        let mut r2 = r.clone();
        r2.max_tokens = 11;
        assert_ne!(r.digest(), r2.digest());
    }

    #[test]
    fn stream_error_event_is_classified() {
        let (_, _, err) = run(&[
            r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        ]);
        let e = err.expect("error");
        assert_eq!(e.class(), "stream");
        assert!(e.usage_unknown());
    }

    #[test]
    fn unknown_events_and_blocks_are_tolerated() {
        let (out, _, err) = run(&[
            r#"data: {"type":"some_future_event","x":1}"#,
            r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"server_tool_use","id":"x"}}"#,
            r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":"ok"}}"#,
            r#"data: {"type":"message_stop"}"#,
        ]);
        assert!(err.is_none());
        assert_eq!(out.content.len(), 2);
        assert_eq!(out.content[0]["type"], "server_tool_use");
    }

    #[test]
    fn status_classification() {
        let h = reqwest::header::HeaderMap::new();
        let body = r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#;
        let e = ProviderError::from_status(429, body, &h);
        assert_eq!(e.class(), "rate_limited");
        assert!(e.is_transient());
        assert!(!e.usage_unknown());
        assert!(e.to_string().contains("slow down"));
        assert_eq!(
            ProviderError::from_status(529, "", &h).class(),
            "overloaded"
        );
        assert_eq!(ProviderError::from_status(401, "", &h).class(), "auth");
        assert!(!ProviderError::from_status(401, "", &h).is_transient());
        assert_eq!(
            ProviderError::from_status(400, "", &h).class(),
            "invalid_request"
        );
        assert_eq!(ProviderError::from_status(503, "", &h).class(), "server");
        assert_eq!(ProviderError::from_status(418, "", &h).class(), "api");
    }

    #[test]
    fn timeout_phases_and_usage_knowledge() {
        let t = ProviderError::Timeout {
            phase: TimeoutPhase::FirstByte,
            elapsed_ms: 60_000,
        };
        assert!(t.is_transient());
        assert!(
            !t.usage_unknown(),
            "nothing streamed yet: no tokens consumed"
        );
        let t = ProviderError::Timeout {
            phase: TimeoutPhase::StreamIdle,
            elapsed_ms: 90_000,
        };
        assert!(t.usage_unknown(), "mid-stream: tokens may have been billed");
    }
}
