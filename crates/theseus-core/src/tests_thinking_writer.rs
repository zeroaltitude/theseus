//! Thinking goes back only to the model that wrote it (M5 25e,
//! theseus-g1gl): an answer another model of the same provider wrote renders
//! without its thinking, while the writing model's own request, and a request
//! that names it as the refusal's fallback (`RequestSpec::fallback`, the
//! render's `also`), keep it, as does the one pair the provider documents as
//! readable (`compiler/readable.rs`). The fakes write no thinking, so only
//! these tests hold `compiler.rs`'s `wrote_model != media.model`.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::Usage;

use crate::catalog::Catalog;
use crate::compiler::{compile, CompileInput, Compiled, RequestSpec};
use crate::config::{CacheTtl, ThinkingDisplay};
use crate::node::{Body, Node};
use crate::stub::Stub;

const WROTE: &str = "claude-sonnet-5-5";
const OTHER: &str = "claude-haiku-5-5";
/// The one model the provider documents as reading `WROTE`'s blocks
/// (`compiler/readable.rs`).
const READER: &str = "claude-opus-5-5";

fn spec(model: &str, fallback: Option<&str>) -> RequestSpec {
    RequestSpec {
        profile: "p".into(),
        provider: "anthropic".into(),
        model: model.into(),
        max_tokens: 1000,
        system_text: "You keep the harbour's log.".into(),
        context_text: String::new(),
        session_text: String::new(),
        persona_text: String::new(),
        card: None,
        cold: None,
        context_files: vec![],
        persona: None,
        defer: None,
        tools: Vec::new().into(),
        effort: None,
        turn_effort: None,
        effort_refused: vec![],
        thinking_display: ThinkingDisplay::Summarized,
        refusal_fallbacks: true,
        fallback: fallback.map(|m| (m.to_string(), Some("cyber".to_string()))),
        first_party: true,
        cache_ttl: CacheTtl::FiveMinutes,
        conversation_ttl: CacheTtl::FiveMinutes,
        walk: None,
        memberships: vec![],
        guidance: vec![],
        prefix: None,
        prefix_used: None,
    }
}

/// A user's question, Sonnet 5.5's answer with its thinking on `provider`,
/// and the next question.
fn nodes(provider: &str) -> Vec<(u64, Stub)> {
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
            provider: provider.into(),
            stop_reason: Some("end_turn".into()),
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
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
    .map(|(p, n)| (p, Stub::hydrated(p, Arc::new(n))))
    .collect()
}

fn run(spec: &RequestSpec) -> Compiled {
    let nodes = nodes(&spec.provider);
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
        situation: &crate::compiler::situation::Situation::ConversationStart,
        kept: None,
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

/// The render path `compiler.rs`'s own gate does not cover: a plain compile,
/// no prefix and so no switch trigger, where `render.rs`'s clause alone
/// decides. It asks `readable` what `compiler.rs` asks it — the request's
/// `first_party` — and not whether the provider's key is spelled
/// `anthropic`. A key is the operator's word for a host; only its `api_base`
/// says whose API answers (`provider.rs`, `is_first_party`), and a key named
/// `anthropic` pointing at a company proxy is a documented case.
#[test]
fn the_readable_pair_follows_first_party_and_not_the_providers_key() {
    // The control: Anthropic's own API under its own key reads the pair.
    let own_key = run(&spec(READER, None));
    assert_eq!(
        thinking(&own_key).len(),
        1,
        "the documented pair keeps its blocks: {:?}",
        own_key.request.messages
    );

    // Anthropic's own API under another key reads it too.
    let other_key = run(&RequestSpec {
        provider: "anthropic-eu".into(),
        ..spec(READER, None)
    });
    assert_eq!(
        thinking(&other_key).len(),
        1,
        "a first-party provider keyed otherwise still reads the pair: {:?}",
        other_key.request.messages
    );

    // A proxy keyed `anthropic` is not Anthropic's API and answers no beta
    // of ours, so the writer's signature goes nowhere.
    let proxied = run(&RequestSpec {
        first_party: false,
        ..spec(READER, None)
    });
    assert!(
        thinking(&proxied).is_empty(),
        "a provider keyed `anthropic` that is not first party gets none: {:?}",
        proxied.request.messages
    );
    let text = serde_json::to_string(&proxied.request.messages).unwrap();
    assert!(text.contains("High water is at noon."), "{text}");
}
