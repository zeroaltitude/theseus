//! Thinking goes back only to the model that wrote it (M5 25e,
//! theseus-g1gl): an answer another model of the same provider wrote renders
//! without its thinking, while the writing model's own request, and a request
//! that names it as the refusal's fallback (`RequestSpec::fallback`, the
//! render's `also`), keep it. The fakes write no thinking, so only this test
//! holds `compiler.rs`'s `wrote_model != media.model`.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::Usage;

use crate::catalog::Catalog;
use crate::compiler::{compile, CompileInput, Compiled, RequestSpec};
use crate::config::{CacheTtl, ThinkingDisplay};
use crate::node::{Body, Node};

const WROTE: &str = "claude-sonnet-5-5";
const OTHER: &str = "claude-opus-5-5";

fn spec(model: &str, fallback: Option<&str>) -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: model.into(),
        max_tokens: 1000,
        system_text: "You keep the harbour's log.".into(),
        context_text: String::new(),
        context_files: vec![],
        persona: None,
        tools: vec![],
        effort: None,
        thinking_display: ThinkingDisplay::Summarized,
        refusal_fallbacks: true,
        fallback: fallback.map(|m| (m.to_string(), "cyber".to_string())),
        first_party: true,
        cache_ttl: CacheTtl::FiveMinutes,
        conversation_ttl: CacheTtl::FiveMinutes,
        walk: None,
        memberships: vec![],
        guidance: vec![],
    }
}

/// A user's question, Sonnet 5.5's answer with its thinking, and the next
/// question.
fn nodes() -> Vec<(u64, Arc<Node>)> {
    let answer = Node::assistant(
        "ses_quay",
        "turn_quay",
        0,
        Body::AssistantMessage {
            blocks: vec![
                json!({"type": "thinking", "thinking": "tides first", "signature": "sig-sonnet"}),
                json!({"type": "text", "text": "High water is at noon."}),
            ],
            model: WROTE.into(),
            provider: "anthropic".into(),
            stop_reason: Some("end_turn".into()),
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
        },
    );
    vec![
        (
            1,
            Node::user("ses_quay", None, "cli", "When is high water?"),
        ),
        (2, answer),
        (3, Node::user("ses_quay", None, "cli", "And low water?")),
    ]
    .into_iter()
    .map(|(p, n)| (p, Arc::new(n)))
    .collect()
}

fn run(spec: &RequestSpec) -> Compiled {
    let nodes = nodes();
    compile(CompileInput {
        session_id: "ses_quay",
        current: None,
        nodes: &nodes,
        last_position: 3,
        spec,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: None,
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
    })
}

/// The request's thinking blocks.
fn thinking(c: &Compiled) -> Vec<Value> {
    c.request
        .messages
        .iter()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter(|b| b["type"] == "thinking")
        .collect()
}

#[test]
fn thinking_another_model_wrote_is_left_out_and_its_writer_keeps_it() {
    let own = run(&spec(WROTE, None));
    assert_eq!(own.request.model, WROTE);
    assert_eq!(thinking(&own).len(), 1, "the writer's own request keeps it");
    assert_eq!(thinking(&own)[0]["signature"], "sig-sonnet");

    let other = run(&spec(OTHER, None));
    assert_eq!(other.request.model, OTHER);
    assert!(
        thinking(&other).is_empty(),
        "another model of the provider gets none: {:?}",
        other.request.messages
    );
    let text = serde_json::to_string(&other.request.messages).unwrap();
    assert!(text.contains("High water is at noon."), "{text}");

    let fallback = run(&spec(OTHER, Some(WROTE)));
    assert_eq!(
        thinking(&fallback).len(),
        1,
        "a request whose fallback wrote it keeps it"
    );
}
