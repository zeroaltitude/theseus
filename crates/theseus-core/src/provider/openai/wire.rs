//! The translation between Theseus's Anthropic-shaped [`ProviderRequest`] and
//! OpenAI's Responses API, both ways (theseus-bq73s). Pure: no clock, no
//! network, no secret. Every decision here is a shape decision, and the tests
//! beside it pin each one.
//!
//! **Why the translation lives below the compiler.** The compiler renders one
//! request shape for every provider, with its cache breakpoints on the system
//! layers and the messages' automatic one (the layered request, theseus-h4a7).
//! Translating here, inside the provider, leaves that rendering untouched: the
//! `ProviderRequest` a profile on OpenAI is given is byte-for-byte the one an
//! Anthropic profile would be given, so the append-only `messages[..3]`
//! invariant and every cached-prefix digest hold with no change to
//! `compiler.rs`. What OpenAI cannot carry (the `cache_control` marks; it
//! caches by prefix on its own) is dropped on the way out and nothing asks
//! the compiler to render it differently.
//!
//! **What round-trips.** An assistant's blocks go back into the next request,
//! so each kind must survive the trip out and back:
//!
//! | Anthropic block        | Responses item                                  |
//! |------------------------|-------------------------------------------------|
//! | `text`                 | `output_text` part of a `message`                |
//! | `tool_use`             | `function_call` (`arguments` a JSON **string**)  |
//! | `tool_result`          | `function_call_output`                           |
//! | `thinking`             | `reasoning`, its `signature` the encrypted blob  |
//! | `image`                | `input_image` with a data URL                    |
//! | `redacted_thinking`    | dropped: nothing on the far side holds it        |
//!
//! A `thinking` block whose `signature` did not come from OpenAI is dropped
//! too: replaying another vendor's signature would be refused, and a request
//! that cannot be replayed must not be sent at all.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use theseus_protocol::Usage;

// No `ContentDelta` here: the deltas are plain strings on this wire and go
// straight to the sink, so the Anthropic accumulator's type has no reader.
use super::super::{Delta, DeltaSink, ModelResponse, ProviderRequest};

/// The marker a `thinking` block's `signature` carries when its reasoning came
/// from this provider. Anthropic's signatures are opaque base64 with no
/// prefix, so the two can never be mistaken for one another, and a block from
/// another provider is left out of an OpenAI request rather than refused by it.
pub const SIG: &str = "openai:";

/// A reasoning item's own id, kept after the marker so the item can be replayed
/// whole: `openai:<id>:<encrypted_content>`. An id is required by the API when
/// an encrypted blob is sent back.
fn signature(id: &str, encrypted: &str) -> String {
    format!("{SIG}{id}:{encrypted}")
}

/// The id and the encrypted content inside a signature this provider wrote,
/// or `None` for any other signature.
fn in_signature(sig: &str) -> Option<(&str, &str)> {
    sig.strip_prefix(SIG)?.split_once(':')
}

/// How much reasoning a thinking budget asks for. OpenAI takes a word, not a
/// token count, so the bands are named here and nowhere else: the thresholds
/// are the catalog's own notion of a small, a working and a long think.
fn effort_of(budget: u64) -> &'static str {
    match budget {
        0..=4_095 => "low",
        4_096..=16_383 => "medium",
        _ => "high",
    }
}

/// The reasoning setting, and whether the encrypted content must be asked for.
/// `None`: thinking is off (or `disabled`), and the request says nothing about
/// reasoning at all.
fn reasoning(req: &ProviderRequest) -> Option<Value> {
    let t = req.thinking.as_ref()?;
    if t.get("type").and_then(Value::as_str) == Some("disabled") {
        return None;
    }
    let budget = t.get("budget_tokens").and_then(Value::as_u64).unwrap_or(0);
    Some(json!({"effort": effort_of(budget), "summary": "auto"}))
}

/// The system layers as one `instructions` string. The compiler's layers are
/// separate blocks so each can carry a cache breakpoint; OpenAI takes one
/// string and caches the prefix itself, so they are joined in order with a
/// blank line, which keeps the bytes and the order the compiler chose.
fn instructions(req: &ProviderRequest) -> Option<String> {
    let joined = req
        .system
        .iter()
        .filter_map(|b| b.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n\n");
    (!joined.is_empty()).then_some(joined)
}

/// A tool definition, Anthropic's `input_schema` becoming `parameters`.
/// `strict` is false: a strict schema must close every object, which the
/// harness's own tools do not all do, and a refusal at the far end would read
/// as a model failure.
fn tool(t: &Value) -> Option<Value> {
    let name = t.get("name").and_then(Value::as_str)?;
    let mut m = Map::new();
    m.insert("type".into(), json!("function"));
    m.insert("name".into(), json!(name));
    if let Some(d) = t.get("description").and_then(Value::as_str) {
        m.insert("description".into(), json!(d));
    }
    m.insert(
        "parameters".into(),
        t.get("input_schema")
            .cloned()
            .unwrap_or_else(|| json!({"type": "object", "properties": {}})),
    );
    m.insert("strict".into(), json!(false));
    Some(Value::Object(m))
}

/// A `tool_result`'s content as the one string OpenAI takes. A result is text
/// blocks, or a bare string; anything else is rendered as its JSON so nothing
/// is silently dropped from a result the model must read.
fn result_text(b: &Value) -> String {
    match b.get("content") {
        None => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|p| match p.get("text").and_then(Value::as_str) {
                Some(t) => t.to_string(),
                None => p.to_string(),
            })
            .collect::<Vec<_>>()
            .join(""),
        Some(other) => other.to_string(),
    }
}

/// An image block as a data URL part.
fn image_part(b: &Value) -> Option<Value> {
    let src = b.get("source")?;
    let media = src.get("media_type").and_then(Value::as_str)?;
    let data = src.get("data").and_then(Value::as_str)?;
    Some(json!({"type": "input_image", "image_url": format!("data:{media};base64,{data}")}))
}

/// One message's blocks as Responses items, appended to `out` in order.
///
/// A message mixes kinds: a user turn carries tool results and then text, an
/// assistant turn thinking and then a call. Each kind that is an item of its
/// own (`function_call`, `function_call_output`, `reasoning`) is pushed where
/// it stood, and the parts that belong to a message item are gathered into one
/// item that keeps their order too.
fn message_items(role: &str, blocks: &[Value], out: &mut Vec<Value>) {
    let assistant = role == "assistant";
    let mut parts: Vec<Value> = Vec::new();
    let flush = |parts: &mut Vec<Value>, out: &mut Vec<Value>| {
        if !parts.is_empty() {
            out.push(json!({
                "type": "message",
                "role": role,
                "content": std::mem::take(parts),
            }));
        }
    };
    for b in blocks {
        match b.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = b.get("text").and_then(Value::as_str).unwrap_or_default();
                let kind = if assistant {
                    "output_text"
                } else {
                    "input_text"
                };
                parts.push(json!({"type": kind, "text": text}));
            }
            Some("image") => {
                if let Some(p) = image_part(b) {
                    parts.push(p);
                }
            }
            Some("tool_use") => {
                flush(&mut parts, out);
                let args = b
                    .get("input")
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "{}".into());
                out.push(json!({
                    "type": "function_call",
                    "call_id": b.get("id").and_then(Value::as_str).unwrap_or_default(),
                    "name": b.get("name").and_then(Value::as_str).unwrap_or_default(),
                    "arguments": args,
                }));
            }
            Some("tool_result") => {
                flush(&mut parts, out);
                out.push(json!({
                    "type": "function_call_output",
                    "call_id": b.get("tool_use_id").and_then(Value::as_str).unwrap_or_default(),
                    "output": result_text(b),
                }));
            }
            Some("thinking") => {
                // Only reasoning this provider produced can be replayed to it.
                let Some((id, enc)) = b
                    .get("signature")
                    .and_then(Value::as_str)
                    .and_then(in_signature)
                else {
                    continue;
                };
                flush(&mut parts, out);
                let summary = b
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let mut m = Map::new();
                m.insert("type".into(), json!("reasoning"));
                m.insert("id".into(), json!(id));
                m.insert("encrypted_content".into(), json!(enc));
                m.insert(
                    "summary".into(),
                    json!([{"type": "summary_text", "text": summary}]),
                );
                out.push(Value::Object(m));
            }
            // `redacted_thinking`, and any block kind the harness does not
            // model, has no Responses item: it is left out rather than sent
            // as something it is not.
            _ => {}
        }
    }
    flush(&mut parts, out);
}

/// Every message as the `input` array.
fn input(req: &ProviderRequest) -> Vec<Value> {
    let mut out = Vec::new();
    for m in &req.messages {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        match m.get("content") {
            Some(Value::Array(blocks)) => message_items(role, blocks, &mut out),
            Some(Value::String(s)) => {
                message_items(role, &[json!({"type": "text", "text": s})], &mut out)
            }
            _ => {}
        }
    }
    out
}

/// The body of `POST /v1/responses`.
///
/// `store` is always false: nothing Theseus sends is retained on the
/// provider's side, which is the posture the data rule takes for every
/// endpoint and is not a knob. `include` asks for the encrypted reasoning
/// whenever reasoning is on, because without it a thinking block cannot be
/// replayed and the next call of the turn would be refused.
pub fn body(req: &ProviderRequest, stream: bool) -> Value {
    let mut m = Map::new();
    m.insert("model".into(), json!(req.model));
    m.insert("max_output_tokens".into(), json!(req.max_tokens));
    if let Some(i) = instructions(req) {
        m.insert("instructions".into(), json!(i));
    }
    m.insert("input".into(), Value::Array(input(req)));
    let tools: Vec<Value> = req.tools.iter().filter_map(tool).collect();
    if !tools.is_empty() {
        m.insert("tools".into(), Value::Array(tools));
    }
    if let Some(r) = reasoning(req) {
        m.insert("reasoning".into(), r);
        m.insert("include".into(), json!(["reasoning.encrypted_content"]));
    }
    m.insert("store".into(), json!(false));
    if stream {
        m.insert("stream".into(), json!(true));
    }
    Value::Object(m)
}

// ------------------------------------------------------------------ response

/// Why a response ended, in Anthropic's vocabulary, which is what every reader
/// in the harness (`hit_cap`, the advancer, the refusal fallback) already
/// knows. A Responses `status` of `incomplete` with the reason `max_output_tokens`
/// is Anthropic's `max_tokens`; a completed response that made calls is
/// `tool_use`, and one that did not is `end_turn`.
pub fn stop_reason(response: &Value, made_calls: bool) -> String {
    let status = response.get("status").and_then(Value::as_str).unwrap_or("");
    let why = response
        .get("incomplete_details")
        .and_then(|d| d.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("");
    match (status, why) {
        ("incomplete", "max_output_tokens") => "max_tokens".into(),
        ("incomplete", "content_filter") => "refusal".into(),
        ("incomplete", other) if !other.is_empty() => other.into(),
        _ if made_calls => "tool_use".into(),
        _ => "end_turn".into(),
    }
}

/// A Responses `usage` as the harness's. OpenAI reports its cache hits under
/// `input_tokens_details.cached_tokens`, and those tokens are **also** counted
/// in `input_tokens`, where Anthropic's are not: they are taken off so a reader
/// that adds the fields does not count them twice. There is no cache-write
/// figure on this protocol (the provider writes its own prefixes and bills
/// none), so the write stays 0 and the ledger prices no write for this kind.
pub fn usage(u: &Value) -> Usage {
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let cached = u
        .get("input_tokens_details")
        .and_then(|d| d.get("cached_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    Usage {
        input_tokens: n("input_tokens").saturating_sub(cached),
        output_tokens: n("output_tokens"),
        cache_read_input_tokens: cached,
        cache_creation_input_tokens: 0,
        cache_creation_1h_input_tokens: 0,
    }
}

/// One finished output item as the Anthropic-shaped content block the
/// transcript stores. `None`: an item with no block of its own.
///
/// `invalid` takes a call whose `arguments` did not parse, keyed by its id, as
/// the Anthropic path records one: the harness answers it with an
/// `INVALID_JSON` error rather than guessing what the model meant.
pub fn block_of(item: &Value, invalid: &mut BTreeMap<String, String>) -> Option<Value> {
    match item.get("type").and_then(Value::as_str)? {
        "message" => {
            let text: String = item
                .get("content")?
                .as_array()?
                .iter()
                .filter(|p| p.get("type").and_then(Value::as_str) == Some("output_text"))
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect();
            Some(json!({"type": "text", "text": text}))
        }
        "function_call" => {
            let id = item
                .get("call_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let raw = item
                .get("arguments")
                .and_then(Value::as_str)
                .unwrap_or("{}");
            let input = match serde_json::from_str::<Value>(raw) {
                Ok(v) if v.is_object() => v,
                _ => {
                    invalid.insert(id.to_string(), raw.to_string());
                    json!({})
                }
            };
            Some(json!({
                "type": "tool_use",
                "id": id,
                "name": item.get("name").and_then(Value::as_str).unwrap_or_default(),
                "input": input,
            }))
        }
        "reasoning" => {
            // No encrypted content (the operator's key is not allowed it, or
            // the field was not asked for): there is nothing to replay, so no
            // block is stored and the next call simply carries no thinking.
            let enc = item.get("encrypted_content").and_then(Value::as_str)?;
            let id = item.get("id").and_then(Value::as_str).unwrap_or_default();
            let summary: String = item
                .get("summary")
                .and_then(Value::as_array)
                .map(|s| {
                    s.iter()
                        .filter_map(|p| p.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            Some(json!({
                "type": "thinking",
                "thinking": summary,
                "signature": signature(id, enc),
            }))
        }
        _ => None,
    }
}

/// What a stream event did.
#[derive(Debug, PartialEq)]
pub enum Step {
    /// Nothing the caller must act on (a progress event, or one this version
    /// does not model: the protocol adds event kinds, and an unknown one is
    /// never an error).
    Nothing,
    /// A finished output item: its block, when it has one.
    Item(Option<Value>),
    /// The response is complete; its object carries the usage and the status.
    Done(Value),
    /// The far end reported a failure.
    Failed(String),
}

/// One `data:` payload from the event stream, applied: the live deltas go to
/// `on_delta` and the finished items come back as [`Step`].
///
/// Blocks are built from the **finished items alone**, never from the deltas.
/// The protocol sends each output item again, whole, when it is done, so
/// assembling partial JSON here would be a second, weaker copy of what the far
/// end already did; the deltas are live progress, which is best effort
/// (AGENTS.md: what must be seen is durable, live progress is best effort).
pub fn step(ev: &Value, invalid: &mut BTreeMap<String, String>, on_delta: DeltaSink<'_>) -> Step {
    let kind = ev.get("type").and_then(Value::as_str).unwrap_or("");
    let delta = || ev.get("delta").and_then(Value::as_str).unwrap_or_default();
    match kind {
        "response.output_text.delta" => {
            on_delta(Delta::Text(delta()));
            Step::Nothing
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            on_delta(Delta::Thinking(delta()));
            Step::Nothing
        }
        "response.output_item.added" => {
            let item = ev.get("item");
            if item.and_then(|i| i.get("type")).and_then(Value::as_str) == Some("function_call") {
                let item = item.unwrap_or(&Value::Null);
                on_delta(Delta::ToolUseStart {
                    id: item.get("call_id").and_then(Value::as_str).unwrap_or(""),
                    name: item.get("name").and_then(Value::as_str).unwrap_or(""),
                });
            }
            Step::Nothing
        }
        "response.output_item.done" => {
            Step::Item(ev.get("item").and_then(|i| block_of(i, invalid)))
        }
        "response.completed" | "response.incomplete" => {
            Step::Done(ev.get("response").cloned().unwrap_or(Value::Null))
        }
        "response.failed" | "error" => Step::Failed(
            ev.get("response")
                .and_then(|r| r.get("error"))
                .or_else(|| ev.get("error"))
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("the provider reported a failed response")
                .to_string(),
        ),
        _ => Step::Nothing,
    }
}

/// A finished response's fields on the [`ModelResponse`] being assembled.
pub fn settle(out: &mut ModelResponse, response: &Value) {
    if let Some(m) = response.get("model").and_then(Value::as_str) {
        out.model = m.to_string();
    }
    if let Some(id) = response.get("id").and_then(Value::as_str) {
        out.message_id = Some(id.to_string());
    }
    if let Some(u) = response.get("usage") {
        out.usage = usage(u);
    }
    let made_calls = out
        .content
        .iter()
        .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_use"));
    out.stop_reason = Some(stop_reason(response, made_calls));
    if out.stop_reason.as_deref() == Some("refusal") {
        out.stop_details = response.get("incomplete_details").cloned();
    }
}

/// The rate-limit headers, which are OpenAI's names, not Anthropic's
/// (`x-ratelimit-*`). The reset fields are durations ("1s", "6m0s") where
/// Anthropic's are timestamps; both are carried as the strings they are, since
/// every reader of them prints them.
pub fn rate_limit(h: &reqwest::header::HeaderMap) -> super::super::RateLimitInfo {
    let s = |k: &str| h.get(k).and_then(|v| v.to_str().ok()).map(str::to_string);
    let n = |k: &str| s(k).and_then(|v| v.parse::<u64>().ok());
    super::super::RateLimitInfo {
        requests_limit: n("x-ratelimit-limit-requests"),
        requests_remaining: n("x-ratelimit-remaining-requests"),
        requests_reset: s("x-ratelimit-reset-requests"),
        tokens_limit: n("x-ratelimit-limit-tokens"),
        tokens_remaining: n("x-ratelimit-remaining-tokens"),
        tokens_reset: s("x-ratelimit-reset-tokens"),
        input_tokens_remaining: None,
        output_tokens_remaining: None,
        retry_after_secs: n("retry-after"),
    }
}
