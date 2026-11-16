//! The provider's binding of a thinking block to what came before it, as the
//! stand-in models it, and the one repeat that answers its refusal.
//!
//! A thinking block the model wrote is bound to the system, the tools, every
//! message before it, and the blocks before it in its own message. A request
//! that replays the block after any of that changed is not the conversation
//! the block was written in: an older model's API reports it
//! (`thinking_mismatch_allowed`, the canary in `BETA_THINKING_BINDING`) and
//! still shows the block to the model; `claude-fable-5-2` answers 400. So
//! what a turn sent once, it must send again exactly, and anything new goes
//! after it (`docs/design/thinking-binding.md`).
//!
//! `FakeProvider` carries the same rule (`Binding`): it stamps each thinking
//! block it answers with a hash of everything before it, and checks the
//! blocks a request replays against it. A test then reads a drift as the
//! provider would, over a whole turn, with no real call.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::{DeltaSink, ModelResponse, Provider, ProviderError, ProviderRequest};

/// What the stand-in does with a replayed block whose prefix changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// The older models': the block reaches the model, and the answer
    /// reports it (`thinking_mismatch_allowed`).
    Report,
    /// `claude-fable-5-2`'s: the request is refused with a 400, unless it
    /// names `drop_block`, which removes the block and reports that
    /// (`thinking_dropped`).
    Refuse,
}

/// The stamp on a block the stand-in answered: the hash it is bound to.
const STAMP: &str = "bound:";

/// The remedy the provider names for a refused prefix, as it sits in
/// `thinking`.
pub const REMEDY: &str = "drop_block";

/// The request's thinking config with the remedy added: what the one repeat
/// sends in place of the config the compiler rendered.
pub fn with_remedy(thinking: &Value) -> Value {
    let mut t = thinking.clone();
    t["block_binding"] = json!({"prefix_mismatch_behavior": REMEDY});
    t
}

/// Whether `e` is the provider's refusal of a drifted prefix, and no other
/// 400: narrow on purpose, so a different refusal is never answered by a
/// repeat that cannot help it.
pub fn is_prefix_refusal(e: &anyhow::Error) -> bool {
    match e.downcast_ref::<ProviderError>() {
        Some(ProviderError::InvalidRequest {
            status: 400,
            message,
        }) => {
            message.contains("`signature` in `thinking` block")
                && message.contains("prefix_mismatch_behavior")
        }
        _ => false,
    }
}

/// `v` with every `cache_control` mark taken out: where a cache breakpoint
/// sits is not content, and it moves between a turn's steps by design.
pub fn unmarked(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.iter()
                .filter(|(k, _)| *k != "cache_control")
                .map(|(k, v)| (k.clone(), unmarked(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(unmarked).collect()),
        other => other.clone(),
    }
}

/// The hash a block at `messages[i].content[j]` is bound to: the system, the
/// tools, the messages before `i`, and the blocks before `j` of its own.
fn bound(req: &ProviderRequest, messages: &[Value], i: usize, j: usize) -> String {
    let own = messages[i]["content"]
        .as_array()
        .map_or(Vec::new(), |b| b[..j].to_vec());
    let prefix = json!([
        &req.system,
        &req.tools,
        &messages[..i],
        &messages[i]["role"],
        own
    ]);
    hex::encode(Sha256::digest(
        serde_json::to_vec(&unmarked(&prefix)).unwrap_or_default(),
    ))
}

fn is_thinking(b: &Value) -> bool {
    b.get("type").and_then(Value::as_str) == Some("thinking")
}

/// The replayed blocks the stand-in bound to a prefix this request no longer
/// has, as `messages.{i}.content.{j}`.
pub fn drifted(req: &ProviderRequest) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, m) in req.messages.iter().enumerate() {
        for (j, b) in m["content"].as_array().into_iter().flatten().enumerate() {
            let stamped = b["signature"].as_str().and_then(|s| s.strip_prefix(STAMP));
            if let (true, Some(h)) = (is_thinking(b), stamped) {
                if h != bound(req, &req.messages, i, j) {
                    out.push((i, j));
                }
            }
        }
    }
    out
}

/// What the provider says of a refused prefix: its own words.
pub fn refusal(drift: &[(usize, usize)]) -> ProviderError {
    let (i, j) = drift[0];
    ProviderError::InvalidRequest {
        status: 400,
        message: format!(
            "messages.{i}.content.{j}: Invalid `signature` in `thinking` block. The block is \
             bound to a different conversation. Remove the block, or set \
             `thinking.block_binding.prefix_mismatch_behavior` to \"{REMEDY}\". Content that \
             preceded this block when it was created is missing from this request, starting at \
             `messages.0.content.0`."
        ),
    }
}

/// The `input_transformations` entries for `drift`: of `kind`
/// (`thinking_mismatch_allowed` or `thinking_dropped`), each with its path.
pub fn reported(kind: &str, drift: &[(usize, usize)]) -> Value {
    Value::Array(
        drift
            .iter()
            .map(|(i, j)| {
                json!({"type": kind, "path": format!("messages.{i}.content.{j}"),
                       "reason": "prefix_binding_mismatch"})
            })
            .collect(),
    )
}

/// `req` with the blocks at `drift` removed, as `drop_block` does (and the
/// rest of their message's thinking, which goes with them: not modelled, a
/// message holds one thinking block in the stand-in's turns).
pub fn without(req: &ProviderRequest, drift: &[(usize, usize)]) -> ProviderRequest {
    let mut out = req.clone();
    for (i, j) in drift.iter().rev() {
        if let Some(b) = out.messages[*i]["content"].as_array_mut() {
            b.remove(*j);
        }
    }
    out
}

/// Stamp the thinking blocks of an answer to `req` with the prefix each is
/// bound to, so a later request that replays them can be checked.
pub fn sign(req: &ProviderRequest, blocks: &mut [Value]) {
    let mut messages = req.messages.clone();
    messages.push(json!({"role": "assistant", "content": blocks.to_vec()}));
    let at = messages.len() - 1;
    for (j, b) in blocks.iter_mut().enumerate() {
        // A block that came stamped is a planted drift, and is left as it is.
        let stamped = b["signature"]
            .as_str()
            .is_some_and(|s| s.starts_with(STAMP));
        if is_thinking(b) && !stamped {
            b["signature"] = Value::String(format!("{STAMP}{}", bound(req, &messages, at, j)));
        }
    }
}

/// What the stand-in makes of `req` under `mode`: the request its answer is
/// bound to, and the `input_transformations` to report; or the refusal.
pub fn check(
    mode: Binding,
    req: &ProviderRequest,
) -> Result<(ProviderRequest, Option<Value>), ProviderError> {
    let drift = drifted(req);
    if drift.is_empty() {
        return Ok((req.clone(), None));
    }
    let asked = req
        .thinking
        .as_ref()
        .and_then(|t| t["block_binding"]["prefix_mismatch_behavior"].as_str())
        == Some(REMEDY);
    match (mode, asked) {
        (Binding::Report, _) => Ok((
            req.clone(),
            Some(reported("thinking_mismatch_allowed", &drift)),
        )),
        (Binding::Refuse, true) => Ok((
            without(req, &drift),
            Some(reported("thinking_dropped", &drift)),
        )),
        (Binding::Refuse, false) => Err(refusal(&drift)),
    }
}

/// One call, with the net under it: when the provider refuses the request for
/// a thinking block whose prefix changed, and for no other reason, the call is
/// made once more with the remedy the provider names (`drop_block`), and the
/// answer says so (`prefix_repeated`). Never sent on a first call, so a model
/// that reports a drift and goes on keeps its thinking, and the canary keeps
/// counting. The request itself is left as the compiler rendered it, so
/// what is recorded and replayed is not changed.
pub async fn stream_with_net(
    provider: &dyn Provider,
    req: &ProviderRequest,
    on_delta: DeltaSink<'_>,
) -> anyhow::Result<ModelResponse> {
    let refused = match provider.stream_message(req, &mut *on_delta).await {
        Err(e) if is_prefix_refusal(&e) => e,
        other => return other,
    };
    let Some(thinking) = req.thinking.as_ref() else {
        return Err(refused);
    };
    tracing::warn!(
        model = %req.model,
        error = %refused,
        "the provider refused a thinking block for a changed prefix; the call is made again with the block dropped"
    );
    let mut again = req.clone();
    again.thinking = Some(with_remedy(thinking));
    let mut resp = provider.stream_message(&again, on_delta).await?;
    resp.prefix_repeated = true;
    Ok(resp)
}
