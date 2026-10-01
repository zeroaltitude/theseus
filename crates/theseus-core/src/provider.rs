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

use crate::catalog::TokenRates;
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

    /// The prompt's size in tokens at its model's built-in figures
    /// ([`TokenRates::of`]), from its bytes by class ([`Census`]), images by
    /// their tiles (theseus-9g2). The compiler estimates with the catalog's
    /// figures, a config's included, and from a compilation's second call
    /// on adds to the provider's own count of the last request instead
    /// (`compiler::Estimate`, theseus-f5hf).
    pub fn estimate_tokens(&self) -> u64 {
        self.census().tokens(TokenRates::of(&self.model)) + self.image_tokens
    }

    /// The request's bytes by class (theseus-f5hf): the tools, the system,
    /// and the messages.
    pub fn census(&self) -> Census {
        let mut c = Census::of_messages(&self.messages);
        for t in &self.tools {
            c.json += json_len(t);
        }
        for b in &self.system {
            c.messages += 1;
            c.text += str_len(&b["text"]);
        }
        c
    }

    /// The bytes of the system, the tools, and the messages as JSON, base64
    /// image data left out: what the estimate before theseus-f5hf divided by
    /// four, kept so a `context.compiled` row can be read against it.
    pub fn json_bytes(&self) -> u64 {
        let data: usize = self
            .messages
            .iter()
            .map(|m| base64_chars(&m["content"]))
            .sum();
        (json_len(&self.system) + json_len(&self.tools) + json_len(&self.messages))
            .saturating_sub(data as u64)
    }
}

/// Tokens of framing a message costs, its role and turn markers. In Eddie's
/// DM an answer and a short message cost 6 more than the answer's output
/// tokens and the message's text (the tokens lane, 2026-10-01).
pub const MESSAGE_TOKENS: u64 = 3;
/// Tokens of framing a content block costs.
pub const BLOCK_TOKENS: u64 = 1;
/// Tokens a tool call's id costs, in its `tool_use` block and again in its
/// `tool_result`: the provider assigns it, so an answer's output tokens
/// leave it out. Fitted to Eddie's DM, where each call and result after a
/// counted request cost about 15 tokens more than their content.
pub const ID_TOKENS: u64 = 15;
/// Bytes a token of a thinking block's signature, or of redacted thinking:
/// the provider counts the thinking they carry, not their bytes. A
/// signature runs about 3 to 16 bytes for each token of the thinking it
/// carries (Eddie's DM), so a fourth over-counts it, the safe side. Only a
/// request with no counted part estimates thinking at all.
pub const OPAQUE_BYTES_PER_TOKEN: f64 = 4.0;
/// The densest figure a catalog row may set: a config's 0 would divide by
/// nothing.
const DENSEST_BYTES_PER_TOKEN: f64 = 0.5;

/// A request's bytes by how densely a tokenizer reads them (theseus-f5hf).
/// Base64 image data is left out: an image counts by its tiles
/// (`ProviderRequest::image_tokens`, theseus-9g2). JSON's own punctuation is
/// left out too: the provider frames messages and blocks with a few tokens
/// each, whatever their JSON.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Census {
    /// Tool schemas, tool inputs, and tool results, in bytes.
    pub json: u64,
    /// The system's text and the messages' text and thinking, in bytes.
    pub text: u64,
    /// Thinking signatures and redacted thinking, in bytes.
    pub opaque: u64,
    /// Messages, and the system's blocks.
    pub messages: u64,
    /// Content blocks.
    pub blocks: u64,
    /// Tool call ids, in `tool_use` and `tool_result` blocks.
    pub ids: u64,
}

impl Census {
    /// The bytes of `messages`, a request's or the end of one.
    pub fn of_messages(messages: &[Value]) -> Census {
        let mut c = Census::default();
        for m in messages {
            c.messages += 1;
            match &m["content"] {
                Value::String(s) => {
                    c.blocks += 1;
                    c.text += s.len() as u64;
                }
                Value::Array(blocks) => blocks.iter().for_each(|b| c.block(b)),
                _ => {}
            }
        }
        c
    }

    fn block(&mut self, b: &Value) {
        self.blocks += 1;
        match b.get("type").and_then(Value::as_str) {
            Some("text") => self.text += str_len(&b["text"]),
            Some("thinking") => {
                self.text += str_len(&b["thinking"]);
                self.opaque += str_len(&b["signature"]);
            }
            Some("redacted_thinking") => self.opaque += str_len(&b["data"]),
            Some("image") => {}
            Some("tool_use") => {
                self.ids += 1;
                self.json += str_len(&b["name"]) + json_len(&b["input"]);
            }
            Some("tool_result") => {
                self.ids += 1;
                match &b["content"] {
                    Value::String(s) => self.json += s.len() as u64,
                    Value::Array(inner) => {
                        for x in inner {
                            self.blocks += 1;
                            match x.get("type").and_then(Value::as_str) {
                                Some("text") => self.json += str_len(&x["text"]),
                                Some("image") => {}
                                _ => self.json += json_len(x),
                            }
                        }
                    }
                    Value::Null => {}
                    other => self.json += json_len(other),
                }
            }
            // A block Theseus does not model: its JSON.
            _ => self.json += json_len(b),
        }
    }

    /// The tokens these bytes come to at `rates`, framing included.
    pub fn tokens(&self, rates: TokenRates) -> u64 {
        let at =
            |bytes: u64, per: f64| (bytes as f64 / per.max(DENSEST_BYTES_PER_TOKEN)).ceil() as u64;
        at(self.json, rates.json)
            + at(self.text, rates.text)
            + at(self.opaque, OPAQUE_BYTES_PER_TOKEN)
            + self.messages * MESSAGE_TOKENS
            + self.blocks * BLOCK_TOKENS
            + self.ids * ID_TOKENS
    }
}

fn str_len(v: &Value) -> u64 {
    v.as_str().map_or(0, |s| s.len() as u64)
}

/// The length of `v` as compact JSON, without building the string.
fn json_len<T: Serialize + ?Sized>(v: &T) -> u64 {
    struct Count(u64);
    impl std::io::Write for Count {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0 += buf.len() as u64;
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut n = Count(0);
    serde_json::to_writer(&mut n, v).map_or(0, |_| n.0)
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

    /// Whether a later identical call could plausibly succeed: the classes
    /// that pass with time, which the driver retries with its backoff for as
    /// long as they last (theseus-ljr). A stream the provider broke off with
    /// an error event (an `overloaded_error` mid-stream) is one. Every other
    /// class gets one retry, then waits on the operator's next message.
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            ProviderError::Timeout { .. }
                | ProviderError::Network { .. }
                | ProviderError::RateLimited { .. }
                | ProviderError::Overloaded { .. }
                | ProviderError::Server { .. }
                | ProviderError::Stream { .. }
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

    /// The provider refused a prompt that alone passes the model's window
    /// (theseus-9p88): a 400 whose message says "prompt is too long", with
    /// its count and the window when it gives them. Any other error is
    /// none, a 400 for another reason included.
    pub fn overflow(&self) -> Option<Overflow> {
        match self {
            ProviderError::InvalidRequest {
                status: 400,
                message,
            } => Overflow::from_message(message),
            _ => None,
        }
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

/// The stop reason of an answer cut at the model's context window, short of
/// its `max_tokens` (theseus-9p88): on Anthropic's 4.5-and-later models a
/// prompt plus `max_tokens` that passes the window is not refused, and the
/// answer stops where the window ends.
pub const WINDOW_EXCEEDED: &str = "model_context_window_exceeded";

/// What a provider's refusal of a prompt past the window says
/// (theseus-9p88). Anthropic's message is "prompt is too long: 213402
/// tokens > 200000 maximum"; either number may be missing from another
/// provider's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overflow {
    /// The provider's count of the prompt.
    pub tokens: Option<u64>,
    /// The window it measured the prompt against.
    pub maximum: Option<u64>,
}

impl Overflow {
    const PHRASE: &'static str = "prompt is too long";

    /// The refusal in `message`, or none when it is not one.
    pub fn from_message(message: &str) -> Option<Overflow> {
        let lower = message.to_ascii_lowercase();
        let at = lower.find(Self::PHRASE)?;
        let rest = &lower[at + Self::PHRASE.len()..];
        // "N tokens > M maximum": a number before "tokens", then one after
        // the `>` and before "maximum". Commas are read as separators.
        let number = |s: &str| -> Option<u64> {
            let digits: String = s.chars().filter(|c| *c != ',').collect();
            (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
                .then(|| digits.parse().ok())
                .flatten()
        };
        let words: Vec<&str> = rest
            .split(|c: char| c.is_whitespace() || c == ':')
            .filter(|w| !w.is_empty())
            .collect();
        let mut tokens = None;
        let mut maximum = None;
        for (i, w) in words.iter().enumerate() {
            let next = words.get(i + 1).copied();
            if tokens.is_none() && next.is_some_and(|n| n.starts_with("token")) {
                tokens = number(w);
            }
            if maximum.is_none() && next.is_some_and(|n| n.starts_with("maximum")) {
                maximum = number(w);
            }
        }
        Some(Overflow { tokens, maximum })
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
        let mut lines = SseLines::default();
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
            lines.push(&chunk);
            while let Some(line) = lines.next_line() {
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

/// The event stream's lines, from its chunks as they arrive. The network cuts
/// a chunk anywhere, inside a character too, so the bytes wait here until a
/// newline ends their line, and each line is decoded whole (theseus-s68: a
/// chunk decoded on its own turned a character it split into U+FFFDs, found
/// by the property test in `tests_outside_text`).
#[derive(Default)]
pub(crate) struct SseLines {
    buf: Vec<u8>,
}

impl SseLines {
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// The next whole line, without its `\r\n`.
    pub(crate) fn next_line(&mut self) -> Option<String> {
        let pos = self.buf.iter().position(|&b| b == b'\n')?;
        let line = String::from_utf8_lossy(&self.buf[..pos])
            .trim_end_matches('\r')
            .to_string();
        self.buf.drain(..=pos);
        Some(line)
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
    // The writes split by TTL (theseus-ev1): a 1-hour write is priced apart.
    // Z.ai reports no split, and its writes stay 5-minute ones.
    if let Some(x) = v
        .get("cache_creation")
        .and_then(|c| c.get("ephemeral_1h_input_tokens"))
        .and_then(Value::as_u64)
    {
        u.cache_creation_1h_input_tokens = x;
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

    /// theseus-9p88: the provider's refusal of a prompt past the window is
    /// read with its count and its maximum, from the body as the API sends
    /// it; any other 400, and the same words in another class, are not one.
    #[test]
    fn a_prompt_past_the_window_is_read_with_its_count_and_maximum() {
        let h = reqwest::header::HeaderMap::new();
        let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 213402 tokens > 200000 maximum"},"request_id":"req_1"}"#;
        let e = ProviderError::from_status(400, body, &h);
        assert_eq!(
            e.overflow(),
            Some(Overflow {
                tokens: Some(213_402),
                maximum: Some(200_000)
            })
        );
        let commas =
            Overflow::from_message("Prompt is too long: 1,048,577 tokens > 1,000,000 maximum");
        assert_eq!(
            commas,
            Some(Overflow {
                tokens: Some(1_048_577),
                maximum: Some(1_000_000)
            })
        );
        // Another wording still overflows, with what it does not say unknown.
        assert_eq!(
            Overflow::from_message("prompt is too long"),
            Some(Overflow {
                tokens: None,
                maximum: None
            })
        );
        let other = ProviderError::from_status(
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages: roles must alternate"}}"#,
            &h,
        );
        assert_eq!(other.overflow(), None, "a 400 for another reason");
        let not_400 = ProviderError::Api {
            status: 418,
            message: "prompt is too long: 3 tokens > 2 maximum".into(),
        };
        assert_eq!(not_400.overflow(), None);
        let large = ProviderError::InvalidRequest {
            status: 413,
            message: "request_too_large: Request exceeds the maximum size".into(),
        };
        assert_eq!(large.overflow(), None, "the body's size is not the window");
    }

    /// The property test's find (theseus-s68): a character the network cut
    /// between two chunks arrives whole, not as U+FFFDs.
    #[test]
    fn a_character_split_between_chunks_arrives_whole() {
        let body = "data: {\"text\":\"中🌀\"}\r\n\n".as_bytes();
        for at in 1..body.len() {
            let mut r = SseLines::default();
            r.push(&body[..at]);
            r.push(&body[at..]);
            assert_eq!(r.next_line().as_deref(), Some("data: {\"text\":\"中🌀\"}"));
            assert_eq!(r.next_line().as_deref(), Some(""));
            assert_eq!(r.next_line(), None);
        }
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
