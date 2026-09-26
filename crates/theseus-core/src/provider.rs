//! Direct Anthropic Messages API client with streaming (spec §3.6).

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use theseus_protocol::Usage;

use crate::secrets::Secret;

/// One provider call's inputs, as the toolchain manager compiled them.
pub struct ProviderRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub system: Option<&'a str>,
    pub messages: &'a [Message],
    pub tools: Vec<ToolDef>,
}

pub type DeltaSink<'a> = &'a mut (dyn FnMut(&str) + Send);
pub type ProviderFuture<'a> = futures_util::future::BoxFuture<'a, Result<ModelResponse>>;

/// A model provider. The turn runner depends on this, never on Anthropic
/// directly, so tests run against a fake and a second provider is a new impl.
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn stream_message<'a>(
        &'a self,
        req: ProviderRequest<'a>,
        on_delta: DeltaSink<'a>,
    ) -> ProviderFuture<'a>;
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

/// A scripted provider for tests: emits `reply` in chunks, reports usage.
pub struct FakeProvider {
    pub reply: String,
    pub chunk: usize,
    pub stop_reason: String,
}

impl Default for FakeProvider {
    fn default() -> Self {
        Self {
            reply: "fake reply".into(),
            chunk: 4,
            stop_reason: "end_turn".into(),
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
                stop_reason: Some(self.stop_reason.clone()),
                tool_calls: Vec::new(),
                usage: Usage {
                    input_tokens,
                    output_tokens: self.reply.split_whitespace().count() as u64,
                    ..Default::default()
                },
                model: req.model.to_string(),
            })
        })
    }
}

pub const API_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ModelResponse {
    pub text: String,
    pub stop_reason: Option<String>,
    pub tool_calls: Vec<Value>,
    pub usage: Usage,
    pub model: String,
}

#[derive(Clone)]
pub struct Anthropic {
    http: reqwest::Client,
    api_base: String,
    key: Secret,
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

#[derive(Deserialize)]
struct Event {
    #[serde(rename = "type")]
    ty: String,
    #[serde(default)]
    message: Option<Value>,
    #[serde(default)]
    delta: Option<Value>,
    #[serde(default)]
    usage: Option<Value>,
    #[serde(default)]
    content_block: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

impl Anthropic {
    pub fn new(api_base: &str, key: Secret) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("theseus/{}", crate::VERSION))
            .build()?;
        Ok(Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            key,
        })
    }

    /// One provider call. `on_delta` receives streamed text as it arrives.
    async fn stream_message_impl(
        &self,
        req: ProviderRequest<'_>,
        on_delta: DeltaSink<'_>,
    ) -> Result<ModelResponse> {
        let model = req.model;
        let body = MessagesRequest {
            model,
            max_tokens: req.max_tokens,
            stream: true,
            system: req.system,
            messages: req.messages,
            tools: req.tools,
        };
        let resp = self
            .http
            .post(format!("{}/v1/messages", self.api_base))
            .header("x-api-key", self.key.expose())
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("sending request to Anthropic")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!(
                "Anthropic returned {status}: {}",
                text.chars().take(600).collect::<String>()
            );
        }

        let mut out = ModelResponse {
            model: model.to_string(),
            ..Default::default()
        };
        let mut buf = String::new();
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("reading stream")?;
            buf.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(pos) = buf.find('\n') {
                let line = buf[..pos].trim_end_matches('\r').to_string();
                buf.drain(..=pos);
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                let ev: Event = match serde_json::from_str(data) {
                    Ok(e) => e,
                    Err(_) => continue,
                };
                match ev.ty.as_str() {
                    "message_start" => {
                        if let Some(u) = ev.message.as_ref().and_then(|m| m.get("usage")) {
                            apply_usage(&mut out.usage, u);
                        }
                        if let Some(m) = ev
                            .message
                            .as_ref()
                            .and_then(|m| m.get("model"))
                            .and_then(Value::as_str)
                        {
                            out.model = m.to_string();
                        }
                    }
                    "content_block_start" => {
                        if let Some(cb) = &ev.content_block {
                            if cb.get("type").and_then(Value::as_str) == Some("tool_use") {
                                out.tool_calls.push(cb.clone());
                            }
                        }
                    }
                    "content_block_delta" => {
                        if let Some(d) = &ev.delta {
                            if d.get("type").and_then(Value::as_str) == Some("text_delta") {
                                if let Some(t) = d.get("text").and_then(Value::as_str) {
                                    out.text.push_str(t);
                                    on_delta(t);
                                }
                            }
                        }
                    }
                    "message_delta" => {
                        if let Some(d) = &ev.delta {
                            if let Some(s) = d.get("stop_reason").and_then(Value::as_str) {
                                out.stop_reason = Some(s.to_string());
                            }
                        }
                        if let Some(u) = &ev.usage {
                            apply_usage(&mut out.usage, u);
                        }
                    }
                    "error" => {
                        bail!(
                            "Anthropic stream error: {}",
                            ev.error.unwrap_or(Value::Null)
                        );
                    }
                    _ => {}
                }
            }
        }
        Ok(out)
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
