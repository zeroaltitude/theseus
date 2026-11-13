//! The second wire protocol's shape (theseus-bq73s). Pure: every arm here
//! reads [`wire`] and sends nothing. What each one pins:
//!
//! - the request shape, field by field, for a call with tools and thinking;
//! - that an assistant's blocks **round-trip** — out as Responses items, back
//!   as the same blocks — because the transcript replays them;
//! - that the compiler's rendering is untouched, which is what keeps the
//!   cache-prefix invariants (`compiler/tests.rs`, append-only `messages[..3]`);
//! - the stream's events, the usage's double-counted cache tokens, the stop
//!   reasons and the rate-limit headers.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::super::{Delta, ProviderRequest};
use super::wire;

/// A request with a system of two layers, a tool, thinking on, and a
/// conversation that already holds a call and its result: the shapes a turn
/// past its first loop actually sends.
fn request() -> ProviderRequest {
    ProviderRequest {
        model: "gpt-mirror-1".into(),
        max_tokens: 2_048,
        system: vec![
            json!({"type": "text", "text": "the header"}),
            json!({"type": "text", "text": "the project", "cache_control": {"type": "ephemeral"}}),
        ],
        messages: vec![
            json!({"role": "user", "content": [{"type": "text", "text": "count the hulls"}]}),
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "two of them", "signature": "openai:rs_4210:ENCBLOB"},
                {"type": "text", "text": "looking"},
                {"type": "tool_use", "id": "call_8812", "name": "fs_read", "input": {"path": "hulls.txt"}},
            ]}),
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_8812",
                 "content": [{"type": "text", "text": "two"}]},
            ]}),
        ],
        tools: vec![json!({
            "name": "fs_read",
            "description": "read a file",
            "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}},
        })],
        thinking: Some(json!({"type": "enabled", "budget_tokens": 8_192})),
        ..Default::default()
    }
}

/// The request shape the bar asks for: a call with tool calls and thinking,
/// field by field.
#[test]
fn the_request_carries_the_whole_call_in_the_responses_shape() {
    let b = wire::body(&request(), true);
    assert_eq!(b["model"], "gpt-mirror-1");
    // `max_tokens` is an output cap on both protocols, under its own name here.
    assert_eq!(b["max_output_tokens"], 2_048);
    assert_eq!(b["stream"], true);
    // Nothing Theseus sends is retained on the provider's side.
    assert_eq!(b["store"], false);
    // The system's layers, in the compiler's order, as one string.
    assert_eq!(b["instructions"], "the header\n\nthe project");
    // Thinking on: the budget becomes an effort word, and the encrypted
    // reasoning is asked for, without which a thinking block cannot replay.
    assert_eq!(
        b["reasoning"],
        json!({"effort": "medium", "summary": "auto"})
    );
    assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
    // The tool: `input_schema` becomes `parameters`, and `strict` is false.
    assert_eq!(
        b["tools"],
        json!([{
            "type": "function",
            "name": "fs_read",
            "description": "read a file",
            "parameters": {"type": "object", "properties": {"path": {"type": "string"}}},
            "strict": false,
        }])
    );
    // The conversation, item by item and in order. A call's `arguments` is a
    // JSON **string** on this protocol, not an object.
    assert_eq!(
        b["input"],
        json!([
            {"type": "message", "role": "user",
             "content": [{"type": "input_text", "text": "count the hulls"}]},
            {"type": "reasoning", "id": "rs_4210", "encrypted_content": "ENCBLOB",
             "summary": [{"type": "summary_text", "text": "two of them"}]},
            {"type": "message", "role": "assistant",
             "content": [{"type": "output_text", "text": "looking"}]},
            {"type": "function_call", "call_id": "call_8812", "name": "fs_read",
             "arguments": "{\"path\":\"hulls.txt\"}"},
            {"type": "function_call_output", "call_id": "call_8812", "output": "two"},
        ])
    );
    // No key, no header and no `cache_control` reaches the body.
    let text = b.to_string();
    assert!(!text.contains("cache_control"), "{text}");
}

/// The compiler renders one shape for every provider, so the translation must
/// read the request and never ask for a different one: the request is
/// unchanged by being translated, and `messages[..3]` — the cached prefix the
/// compiler appends to and never rewrites — is still the same three messages
/// in the same order after the body is built.
#[test]
fn translating_changes_neither_the_request_nor_its_prefix() {
    let req = request();
    let before = serde_json::to_string(&req).unwrap();
    let body = wire::body(&req, true);
    assert_eq!(serde_json::to_string(&req).unwrap(), before);
    // The prefix the compiler owns is still exactly what it rendered.
    assert_eq!(req.messages.len(), 3);
    assert_eq!(req.messages[0]["content"][0]["text"], "count the hulls");
    assert_eq!(req.digest(), request().digest());
    // Two translations of one request are byte-identical, so a reconstruction
    // of a request reproduces the body that was sent.
    assert_eq!(body, wire::body(&req, true));
}

/// A thinking budget picks an effort word, at the bands and on both sides of
/// each; `disabled` says nothing about reasoning at all, and neither does a
/// request with no thinking.
#[test]
fn the_thinking_budget_becomes_an_effort_and_disabled_says_nothing() {
    let effort = |budget: u64| {
        let mut r = request();
        r.thinking = Some(json!({"type": "enabled", "budget_tokens": budget}));
        wire::body(&r, true)["reasoning"]["effort"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(effort(1_024), "low");
    assert_eq!(effort(4_095), "low");
    assert_eq!(effort(4_096), "medium");
    assert_eq!(effort(16_383), "medium");
    assert_eq!(effort(16_384), "high");
    assert_eq!(effort(64_000), "high");
    for off in [
        Some(json!({"type": "disabled"})),
        Some(json!({"type": "disabled", "budget_tokens": 0})),
        None,
    ] {
        let mut r = request();
        r.thinking = off;
        let b = wire::body(&r, true);
        assert!(b.get("reasoning").is_none(), "{b}");
        assert!(b.get("include").is_none(), "{b}");
    }
}

/// Only reasoning this provider produced goes back to it: another vendor's
/// signature, and a redacted block, are left out rather than sent as
/// something they are not. The positive count is asserted beside the absence,
/// so an empty input cannot pass this.
#[test]
fn only_this_providers_own_reasoning_is_replayed() {
    let mut r = request();
    r.messages = vec![json!({"role": "assistant", "content": [
        {"type": "thinking", "thinking": "mine", "signature": "openai:rs_1:A"},
        {"type": "thinking", "thinking": "another vendor's", "signature": "EqoBCkYIBBgCKkA"},
        {"type": "redacted_thinking", "data": "OPAQUE"},
        {"type": "text", "text": "done"},
    ]})];
    let items = wire::body(&r, true)["input"].as_array().unwrap().clone();
    // Exactly two items: the one replayable reasoning, and the message.
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!(items[0]["type"], "reasoning");
    assert_eq!(items[0]["encrypted_content"], "A");
    assert_eq!(items[1]["type"], "message");
    let text = Value::Array(items).to_string();
    assert!(
        !text.contains("EqoBCkYIBBgCKkA") && !text.contains("OPAQUE"),
        "{text}"
    );
}

/// A finished output item becomes the Anthropic-shaped block the transcript
/// stores, and that block translates back into the item it came from: the
/// round trip the replay rules require (§4.4).
#[test]
fn an_assistants_blocks_round_trip_through_the_wire() {
    let items = [
        json!({"type": "reasoning", "id": "rs_77", "encrypted_content": "BLOB",
               "summary": [{"type": "summary_text", "text": "a thought"}]}),
        json!({"type": "message", "role": "assistant", "status": "completed",
               "content": [{"type": "output_text", "text": "the answer"}]}),
        json!({"type": "function_call", "call_id": "call_9", "name": "fs_read",
               "arguments": "{\"path\":\"a\"}"}),
    ];
    let mut invalid = BTreeMap::new();
    let blocks: Vec<Value> = items
        .iter()
        .filter_map(|i| wire::block_of(i, &mut invalid))
        .collect();
    assert!(invalid.is_empty(), "{invalid:?}");
    assert_eq!(
        blocks,
        vec![
            json!({"type": "thinking", "thinking": "a thought", "signature": "openai:rs_77:BLOB"}),
            json!({"type": "text", "text": "the answer"}),
            json!({"type": "tool_use", "id": "call_9", "name": "fs_read", "input": {"path": "a"}}),
        ]
    );
    // Back out: the same three items, the message's part rendered as output.
    let r = ProviderRequest {
        messages: vec![json!({"role": "assistant", "content": blocks})],
        ..Default::default()
    };
    let out = wire::body(&r, false)["input"].as_array().unwrap().clone();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0], items[0]);
    assert_eq!(
        out[1]["content"],
        json!([{"type": "output_text", "text": "the answer"}])
    );
    assert_eq!(out[2], items[2]);
}

/// A call whose `arguments` did not parse is recorded by id, as the Anthropic
/// path records one, and its block carries an empty input: the harness answers
/// it with an error rather than guessing.
#[test]
fn a_call_whose_arguments_do_not_parse_is_recorded_by_id() {
    let mut invalid = BTreeMap::new();
    let b = wire::block_of(
        &json!({"type": "function_call", "call_id": "call_3", "name": "fs_read",
                "arguments": "{\"path\": "}),
        &mut invalid,
    )
    .unwrap();
    assert_eq!(b["input"], json!({}));
    assert_eq!(
        invalid.get("call_3").map(String::as_str),
        Some("{\"path\": ")
    );
    // A reasoning item with no encrypted content has nothing to replay, so no
    // block is stored for it at all.
    assert!(wire::block_of(
        &json!({"type": "reasoning", "id": "rs_1", "summary": []}),
        &mut invalid
    )
    .is_none());
}

/// The stream: deltas are live progress and go to the sink alone, blocks come
/// from the finished items, and `response.completed` ends it.
#[test]
fn the_stream_sends_deltas_live_and_builds_blocks_from_finished_items() {
    let mut seen: Vec<String> = Vec::new();
    let mut invalid = BTreeMap::new();
    let events = [
        json!({"type": "response.created", "response": {"id": "resp_1"}}),
        json!({"type": "response.reasoning_summary_text.delta", "delta": "weighing"}),
        json!({"type": "response.output_text.delta", "delta": "the "}),
        json!({"type": "response.output_text.delta", "delta": "answer"}),
        json!({"type": "response.output_item.added",
               "item": {"type": "function_call", "call_id": "call_5", "name": "fs_read"}}),
        json!({"type": "response.function_call_arguments.delta", "delta": "{\"p\""}),
        // An event kind this version does not model: never an error.
        json!({"type": "response.some.future.event", "delta": "ignored"}),
        json!({"type": "response.output_item.done",
               "item": {"type": "message", "role": "assistant",
                        "content": [{"type": "output_text", "text": "the answer"}]}}),
        json!({"type": "response.output_item.done",
               "item": {"type": "function_call", "call_id": "call_5", "name": "fs_read",
                        "arguments": "{\"p\":1}"}}),
    ];
    let mut blocks = Vec::new();
    {
        let mut sink = |d: Delta<'_>| {
            seen.push(match d {
                Delta::Text(t) => format!("text:{t}"),
                Delta::Thinking(t) => format!("think:{t}"),
                Delta::ToolUseStart { id, name } => format!("call:{name}:{id}"),
            });
        };
        for ev in &events {
            match wire::step(ev, &mut invalid, &mut sink) {
                wire::Step::Nothing => {}
                wire::Step::Item(Some(b)) => blocks.push(b),
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!(
        seen,
        vec![
            "think:weighing",
            "text:the ",
            "text:answer",
            "call:fs_read:call_5"
        ]
    );
    assert_eq!(
        blocks,
        vec![
            json!({"type": "text", "text": "the answer"}),
            json!({"type": "tool_use", "id": "call_5", "name": "fs_read", "input": {"p": 1}}),
        ]
    );
    let done = json!({"type": "response.completed",
                      "response": {"id": "resp_1", "status": "completed"}});
    let mut quiet = |_: Delta<'_>| {};
    assert!(matches!(
        wire::step(&done, &mut invalid, &mut quiet),
        wire::Step::Done(_)
    ));
}

/// A failed response, by either of its two event names, comes back as a
/// failure carrying the far end's own message.
#[test]
fn a_failed_response_carries_the_far_ends_message() {
    let mut invalid = BTreeMap::new();
    let mut sink = |_: Delta<'_>| {};
    for ev in [
        json!({"type": "response.failed",
               "response": {"error": {"message": "the model is overloaded"}}}),
        json!({"type": "error", "error": {"message": "the model is overloaded"}}),
    ] {
        match wire::step(&ev, &mut invalid, &mut sink) {
            wire::Step::Failed(m) => assert_eq!(m, "the model is overloaded"),
            other => panic!("{other:?}"),
        }
    }
}

/// The stop reason, in the vocabulary every reader in the harness already
/// knows: a response that made calls, one that did not, and the cap.
#[test]
fn the_stop_reason_is_said_in_the_harnesss_own_words() {
    let completed = json!({"status": "completed"});
    assert_eq!(wire::stop_reason(&completed, false), "end_turn");
    assert_eq!(wire::stop_reason(&completed, true), "tool_use");
    let capped = json!({"status": "incomplete",
                        "incomplete_details": {"reason": "max_output_tokens"}});
    assert_eq!(wire::stop_reason(&capped, false), "max_tokens");
    // `hit_cap` is the one place the harness tests for a cut answer, and it
    // must read this arm's word too.
    assert!(super::super::hit_cap(Some(&wire::stop_reason(
        &capped, false
    ))));
    let filtered = json!({"status": "incomplete",
                          "incomplete_details": {"reason": "content_filter"}});
    assert_eq!(wire::stop_reason(&filtered, false), "refusal");
}

/// The usage: this provider counts its cache hits inside `input_tokens` where
/// Anthropic does not, so they are taken off and a reader that adds the
/// fields does not count them twice. No cache-write figure exists on this
/// protocol, so the write stays 0.
#[test]
fn a_cache_hit_is_counted_once() {
    let u = wire::usage(&json!({
        "input_tokens": 1_000,
        "output_tokens": 42,
        "input_tokens_details": {"cached_tokens": 800},
    }));
    assert_eq!(u.input_tokens, 200);
    assert_eq!(u.cache_read_input_tokens, 800);
    assert_eq!(u.output_tokens, 42);
    assert_eq!(u.cache_creation_input_tokens, 0);
    assert_eq!(
        u.input_tokens + u.cache_read_input_tokens,
        1_000,
        "the prompt's tokens must still add up to what was billed"
    );
    // No details at all: every token is an uncached input token.
    let plain = wire::usage(&json!({"input_tokens": 7, "output_tokens": 1}));
    assert_eq!((plain.input_tokens, plain.cache_read_input_tokens), (7, 0));
}

/// The rate-limit headers are this provider's names, not Anthropic's: read
/// from `x-ratelimit-*`, and an Anthropic-named header is read by neither arm
/// as the other's.
#[test]
fn the_rate_limit_headers_are_this_providers_own_names() {
    let mut h = reqwest::header::HeaderMap::new();
    for (k, v) in [
        ("x-ratelimit-limit-requests", "500"),
        ("x-ratelimit-remaining-requests", "499"),
        ("x-ratelimit-reset-requests", "6m0s"),
        ("x-ratelimit-limit-tokens", "30000"),
        ("x-ratelimit-remaining-tokens", "29000"),
        ("x-ratelimit-reset-tokens", "1s"),
        ("retry-after", "4"),
    ] {
        h.insert(k, v.parse().unwrap());
    }
    let r = wire::rate_limit(&h);
    assert_eq!(r.requests_limit, Some(500));
    assert_eq!(r.requests_remaining, Some(499));
    assert_eq!(r.requests_reset.as_deref(), Some("6m0s"));
    assert_eq!(r.tokens_limit, Some(30_000));
    assert_eq!(r.tokens_remaining, Some(29_000));
    assert_eq!(r.tokens_reset.as_deref(), Some("1s"));
    assert_eq!(r.retry_after_secs, Some(4));
    // Anthropic's own names are not read here.
    let mut other = reqwest::header::HeaderMap::new();
    other.insert("anthropic-ratelimit-requests-limit", "9".parse().unwrap());
    assert_eq!(wire::rate_limit(&other).requests_limit, None);
    // ...and this arm's names are not read by the Anthropic one.
    assert_eq!(
        super::super::RateLimitInfo::from_headers(&h).requests_limit,
        None
    );
}

/// A `data:` line yields its event; every other line of the stream yields
/// nothing, `[DONE]` included.
#[test]
fn only_a_data_line_is_an_event() {
    assert_eq!(
        super::data_of("data: {\"type\":\"response.completed\"}")
            .unwrap()
            .get("type")
            .and_then(Value::as_str),
        Some("response.completed")
    );
    for line in [
        "event: response.completed",
        "data: [DONE]",
        "data:",
        ": a comment",
        "",
        "data: not json",
    ] {
        assert!(super::data_of(line).is_none(), "{line}");
    }
}
