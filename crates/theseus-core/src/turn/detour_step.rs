//! A detour's request, said once (theseus-v4u66, theseus-r5ydi,
//! theseus-3bs4e).
//!
//! The router sends a trivial or quick turn to a cheaper profile with a small
//! request built outside the session's compilation (`route_step`'s
//! `compile_detour`). That request is [`detour_spec`] of the routed profile's
//! spec: the fit of a named tool family is counted on it
//! (`defer_step::defer_tools`) and the compile is made from it, so the two
//! cannot drift. Its compile is never stored, so each loop records
//! `context.detoured` (names and counts) beside `loop.started`, in the same
//! frame, for `context.explain` to read.

use theseus_protocol::context::{ContextDetoured, DetourPart};

use super::*;
use crate::catalog::TokenRates;
use crate::context_parts::{json_tokens, text_tokens};

/// What the routed profile's spec sends as a detour: the profile's header,
/// the session's block and the tools, with no persona files, context files,
/// card, cold rewrite or ontology walk. The owner's standing records ride the
/// header layer, so they stay (theseus-a7vi).
pub(super) fn detour_spec(spec: &RequestSpec) -> RequestSpec {
    let mut detour = spec.clone();
    detour.context_text = String::new();
    detour.persona_text = String::new();
    (detour.card, detour.cold) = (None, None);
    detour.context_files = Vec::new();
    detour.walk = None;
    detour
}

/// What a detour's tools part is called: a trivial or commentary one's request holds
/// `tools_find` alone (theseus-d9je7, theseus-gezlj), and the row says so.
fn tools_name(tools: &[serde_json::Value], kind: &str) -> String {
    match tools {
        [one]
            if crate::config::routing::is_bare_detour(kind)
                && one["name"] == crate::tools_defer::FIND =>
        {
            format!("{kind}: tools_find only")
        }
        _ => format!("{} tools", tools.len()),
    }
}

/// The tokens each tail block added to a detour's request, measured as the
/// estimate each one moved.
#[derive(Default)]
pub(super) struct Tail {
    pub persona: u64,
    pub standing: u64,
}

impl TurnRunner {
    /// The `context.detoured` row for one loop's request: the parts of
    /// `compiled` by name and tokens, which sum to its estimate. The
    /// conversation's share (everything the header, tools and tail blocks
    /// leave) is split into the message and the exchanges before it.
    pub(super) fn detoured_row(
        &self,
        t: &Turn<'_>,
        sent: &RequestSpec,
        compiled: &Compiled,
        (nodes, tail, project): (&[(u64, crate::stub::Stub)], &Tail, u64),
        i: u32,
    ) -> ContextDetoured {
        let rates = self
            .catalog
            .get(&sent.model)
            .map_or_else(|| TokenRates::of(&sent.model), |e| e.bytes_per_token);
        let block = |text: &str| match text.is_empty() {
            true => 0,
            false => text_tokens(text.len(), rates) + crate::provider::MESSAGE_TOKENS,
        };
        let (header, session) = (block(&sent.system_text), block(&sent.session_text));
        let tools = json_tokens(
            crate::tools_defer::tools_bytes(&compiled.request.tools) as usize,
            rates,
        );
        let asks: Vec<_> = nodes
            .iter()
            .filter(|(_, n)| {
                n.origin == crate::node::Origin::Operator
                    && n.kind == crate::stub::Kind::UserMessage
            })
            .collect();
        let message = asks.last().map_or(0, |(_, n)| {
            text_tokens(crate::recall::text_of(n).len(), rates)
        });
        let recall: u64 = t
            .recall
            .pending
            .as_ref()
            .filter(|_| t.route.detour_recall)
            .map_or(0, |n| match &n.body {
                crate::node::Body::Recall { items, .. } => items.iter().map(|it| it.tokens).sum(),
                _ => 0,
            });
        let conversation = compiled
            .est_tokens
            .saturating_sub(header + session + tools + tail.persona + tail.standing + project);
        let exchanges = conversation.saturating_sub(message + recall);
        let earlier = asks.len().saturating_sub(1);
        let mut parts = vec![DetourPart {
            block: "header".into(),
            name: "the profile's header and the standing records".into(),
            tokens: header,
        }];
        let mut part = |block: &str, name: String, tokens: u64| {
            if tokens > 0 {
                parts.push(DetourPart {
                    block: block.into(),
                    name,
                    tokens,
                });
            }
        };
        part("session", "the session's directory".into(), session);
        part(
            "tools",
            tools_name(&compiled.request.tools, &t.route.detour_kind),
            tools,
        );
        part("persona", "the persona's tail line".into(), tail.persona);
        part(
            "standing",
            "this surface's standing records".into(),
            tail.standing,
        );
        let calls = if i > 0 { " and this turn's calls" } else { "" };
        part(
            "exchanges",
            format!(
                "{earlier} earlier exchange{}{calls}",
                if earlier == 1 { "" } else { "s" }
            ),
            exchanges,
        );
        part("message", "the message".into(), message);
        if recall > 0 {
            let id = t.recall.pending.as_ref().map(|n| n.id.clone());
            part("recall", id.unwrap_or_default(), recall);
        }
        part("project", "the project's slice".into(), project);
        ContextDetoured {
            session_id: t.tc.session_id.into(),
            turn_id: t.tc.turn_id.into(),
            loop_index: i,
            profile: t.target.profile.clone(),
            model: t.target.model.clone(),
            kind: t.route.detour_kind.clone(),
            recall: Some(t.route.detour_recall),
            class: t.tc.class.as_str().into(),
            est_tokens: compiled.est_tokens,
            parts,
            families: compiled
                .compilation
                .manifest
                .loaded
                .iter()
                .flatten()
                .cloned()
                .collect(),
        }
    }
}
