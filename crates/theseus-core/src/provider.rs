//! Direct Anthropic Messages API client with streaming (spec §3.6).
//!
//! Typed content blocks and stream events, classified errors with the
//! provider's rate-limit headers, and four timeouts (connect, first byte,
//! stream idle, total). No automatic retry: an interrupted or refused call is
//! a classified error the harness ledgers and decides about. The shapes here
//! were checked against the community `claude-sdk` crate's `types` and
//! `streaming` modules and the Messages API reference; nothing is imported.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::Usage;

use crate::secrets::Secret;

pub const API_VERSION: &str = "2023-06-01";

// ------------------------------------------------------------------ request

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// One provider call's inputs, as the toolchain manager compiled them.
pub struct ProviderRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub system: Option<&'a str>,
    pub messages: &'a [Message],
    pub tools: Vec<ToolDef>,
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

/// A content block as the API defines it. Unknown kinds are kept, not dropped.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    Thinking {
        thinking: String,
        #[serde(default)]
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    #[serde(other)]
    Unknown,
}

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
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageDelta {
    #[serde(default)]
    pub stop_reason: Option<String>,
    #[serde(default)]
    pub stop_sequence: Option<String>,
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
        content_block: ContentBlock,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentDelta,
    },
    ContentBlockStop {
        index: usize,
    },
    MessageDelta {
        delta: MessageDelta,
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
    pub content: Vec<ContentBlock>,
    pub stop_reason: Option<String>,
    pub usage: Usage,
    pub model: String,
    pub request_id: Option<String>,
    pub rate_limit: RateLimitInfo,
    pub timing: CallTiming,
}

impl ModelResponse {
    pub fn tool_calls(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect()
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

pub type DeltaSink<'a> = &'a mut (dyn FnMut(&str) + Send);
pub type ProviderFuture<'a> = futures_util::future::BoxFuture<'a, Result<ModelResponse>>;

/// A model provider. The turn runner depends on this, never on Anthropic
/// directly, so tests run against a fake and a second provider is a new impl.
/// Errors are `ProviderError` wrapped in `anyhow`; callers downcast to classify.
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn stream_message<'a>(
        &'a self,
        req: ProviderRequest<'a>,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a>;
}

// --------------------------------------------------------------- anthropic

#[derive(Clone)]
pub struct Anthropic {
    http: reqwest::Client,
    api_base: String,
    key: Secret,
    timeouts: Timeouts,
}

#[derive(Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a str>,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolDef>,
}

impl Anthropic {
    pub fn new(api_base: &str, key: Secret, timeouts: Timeouts) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("theseus/{}", crate::VERSION))
            .connect_timeout(Duration::from_secs(timeouts.connect_secs))
            .build()?;
        Ok(Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            key,
            timeouts,
        })
    }

    async fn stream_message_impl(
        &self,
        req: ProviderRequest<'_>,
        on_delta: DeltaSink<'_>,
    ) -> Result<ModelResponse> {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(self.timeouts.total_secs);
        let elapsed = |s: Instant| s.elapsed().as_millis() as u64;

        let body = MessagesRequest {
            model: req.model,
            max_tokens: req.max_tokens,
            stream: true,
            system: req.system,
            messages: req.messages,
            tools: req.tools,
        };
        let send = self
            .http
            .post(format!("{}/v1/messages", self.api_base))
            .header("x-api-key", self.key.expose())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send();

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
            model: req.model.to_string(),
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
        out.content = acc.finish();
        out.text = out
            .content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
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
        req: ProviderRequest<'a>,
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

/// Assembles content blocks from start/delta/stop events. Tool input arrives
/// as partial JSON; it is parsed once, at block stop, never dispatched early.
#[derive(Default)]
struct Accumulator {
    blocks: BTreeMap<usize, ContentBlock>,
    partial_json: BTreeMap<usize, String>,
}

impl Accumulator {
    fn apply(
        &mut self,
        ev: StreamEvent,
        out: &mut ModelResponse,
        on_delta: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Flow> {
        match ev {
            StreamEvent::MessageStart { message } => {
                if let Some(u) = message.get("usage") {
                    apply_usage(&mut out.usage, u);
                }
                if let Some(m) = message.get("model").and_then(Value::as_str) {
                    out.model = m.to_string();
                }
            }
            StreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                self.blocks.insert(index, content_block);
            }
            StreamEvent::ContentBlockDelta { index, delta } => match delta {
                ContentDelta::TextDelta { text } => {
                    if let Some(ContentBlock::Text { text: t }) = self.blocks.get_mut(&index) {
                        t.push_str(&text);
                    } else {
                        self.blocks
                            .insert(index, ContentBlock::Text { text: text.clone() });
                    }
                    on_delta(&text);
                }
                ContentDelta::InputJsonDelta { partial_json } => {
                    self.partial_json
                        .entry(index)
                        .or_default()
                        .push_str(&partial_json);
                }
                ContentDelta::ThinkingDelta { thinking } => {
                    if let Some(ContentBlock::Thinking { thinking: t, .. }) =
                        self.blocks.get_mut(&index)
                    {
                        t.push_str(&thinking);
                    }
                }
                ContentDelta::SignatureDelta { signature } => {
                    if let Some(ContentBlock::Thinking { signature: s, .. }) =
                        self.blocks.get_mut(&index)
                    {
                        s.push_str(&signature);
                    }
                }
                ContentDelta::Unknown => {}
            },
            StreamEvent::ContentBlockStop { index } => {
                if let Some(json) = self.partial_json.remove(&index) {
                    if let Some(ContentBlock::ToolUse { input, .. }) = self.blocks.get_mut(&index) {
                        if !json.trim().is_empty() {
                            *input =
                                serde_json::from_str(&json).map_err(|e| ProviderError::Stream {
                                    kind: "invalid_tool_input".into(),
                                    message: format!("tool input was not valid JSON: {e}"),
                                })?;
                        }
                    }
                }
            }
            StreamEvent::MessageDelta { delta, usage } => {
                if let Some(s) = delta.stop_reason {
                    out.stop_reason = Some(s);
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

    fn finish(self) -> Vec<ContentBlock> {
        self.blocks.into_values().collect()
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

/// A scripted provider for tests: emits `reply` in chunks, reports usage, or
/// fails with a chosen error.
pub struct FakeProvider {
    pub reply: String,
    pub chunk: usize,
    pub stop_reason: String,
    pub fail_with: Option<ProviderError>,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self {
            reply: "fake reply".into(),
            chunk: 4,
            stop_reason: "end_turn".into(),
            fail_with: None,
        }
    }
}

impl Provider for FakeProvider {
    fn name(&self) -> &str {
        "fake"
    }
    fn stream_message<'a>(
        &'a self,
        req: ProviderRequest<'a>,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a> {
        Box::pin(async move {
            if let Some(e) = &self.fail_with {
                return Err(e.clone().into());
            }
            let chars: Vec<char> = self.reply.chars().collect();
            for piece in chars.chunks(self.chunk.max(1)) {
                let s: String = piece.iter().collect();
                on_delta(&s);
                tokio::task::yield_now().await;
            }
            let input_tokens = req
                .messages
                .iter()
                .map(|m| m.content.split_whitespace().count() as u64)
                .sum();
            Ok(ModelResponse {
                text: self.reply.clone(),
                content: vec![ContentBlock::Text {
                    text: self.reply.clone(),
                }],
                stop_reason: Some(self.stop_reason.clone()),
                usage: Usage {
                    input_tokens,
                    output_tokens: self.reply.split_whitespace().count() as u64,
                    ..Default::default()
                },
                model: req.model.to_string(),
                request_id: Some("req_fake".into()),
                rate_limit: RateLimitInfo::default(),
                timing: CallTiming {
                    first_byte_ms: Some(1),
                    first_token_ms: Some(2),
                    total_ms: 3,
                },
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
        let mut sink = |t: &str| deltas.push(t.to_string());
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
        out.content = acc.finish();
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
            vec![ContentBlock::Text {
                text: "Hello".into()
            }]
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
        assert_eq!(out.tool_calls().len(), 1);
        match &out.content[0] {
            ContentBlock::ToolUse { name, input, .. } => {
                assert_eq!(name, "bash");
                assert_eq!(input["cmd"], "ls -la");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(out.stop_reason.as_deref(), Some("tool_use"));
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
        assert_eq!(out.content[0], ContentBlock::Unknown);
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
