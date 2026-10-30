//! route.v10's state facts as the daemon computes them (theseus-9f72b): the
//! price period from the clock, the restricted-model rule from the session's
//! labels, the last reply's model and a task's models from its nodes.
//!
//! Nothing here asks Jev to compute: the hour is read from the instant given,
//! so a test passes the instant and no clock is mocked.

use theseus_judge::builders::{InboundInput, RestrictedModels, RouteFacts, TASK_MODELS};
use theseus_judge::{Input, Pack};

use crate::catalog::price_period_at;
use crate::node::{Body, Node};

/// The facts for an ask about the message after `before`, the session's nodes
/// up to it (a checkpoint's is the whole task session): `task` is a task's
/// session, whose every writing model is listed once, oldest first.
pub(crate) fn facts_of(
    before: &[Node],
    task: bool,
    restricted: RestrictedModels,
    now_ms: u64,
) -> RouteFacts {
    let models = || {
        before.iter().filter_map(|n| match &n.body {
            Body::AssistantMessage { model, .. } => Some(model.as_str()),
            _ => None,
        })
    };
    let mut task_models: Vec<String> = Vec::new();
    if task {
        for m in models() {
            if !task_models.iter().any(|t| t == m) && task_models.len() < TASK_MODELS {
                task_models.push(m.to_string());
            }
        }
    }
    RouteFacts {
        price_period: price_period_at(now_ms),
        restricted_models: restricted,
        last_reply_model: models().next_back().map(str::to_string),
        task_models,
        est_tokens: Some(est_tokens(before)),
    }
}

/// How many tokens the conversation holds: the last request's prompt as the
/// provider counted it (new, cache-read and cache-written input), else a
/// quarter of the text's bytes. The table's "a long context" and "above 100K"
/// are read against it; Jev never works it out.
fn est_tokens(before: &[Node]) -> u64 {
    let counted = before.iter().rev().find_map(|n| match &n.body {
        Body::AssistantMessage { usage, .. }
            if usage.input_tokens
                + usage.cache_read_input_tokens
                + usage.cache_creation_input_tokens
                > 0 =>
        {
            Some(
                usage.input_tokens
                    + usage.cache_read_input_tokens
                    + usage.cache_creation_input_tokens,
            )
        }
        _ => None,
    });
    counted.unwrap_or_else(|| {
        let bytes: usize = before
            .iter()
            .map(|n| match &n.body {
                Body::UserMessage { text, .. } => text.len(),
                Body::AssistantMessage { blocks, .. } => crate::provider::text_of(blocks).len(),
                _ => 0,
            })
            .sum();
        (bytes / 4) as u64
    })
}

/// Whether the pack asks the `model` question, so reads the facts. A pack that
/// does not (every one before route.v10) is sent the state it always was, byte
/// for byte, so the facts change no live decision until one does.
pub(crate) fn asks_model(pack: &Pack) -> bool {
    pack.questions.iter().any(|q| q.id == "model")
}

/// The inbound input as each kind of pack reads it: the route pack's with no
/// onto fields (and, when it asks `model`, with the facts), and the shared one
/// the other packs read, which carries no facts.
pub(crate) struct Carried {
    pub(crate) plain: Input,
    facts: Input,
    pub(crate) shared: Input,
}

impl Carried {
    pub(crate) fn of(input: InboundInput) -> Self {
        let plain = Input::Inbound(InboundInput {
            onto: None,
            route: None,
            ..input.clone()
        });
        let facts = Input::Inbound(InboundInput {
            onto: None,
            ..input.clone()
        });
        let shared = Input::Inbound(InboundInput {
            route: None,
            ..input
        });
        Self {
            plain,
            facts,
            shared,
        }
    }

    /// What the route pack reads: the facts when it asks `model`.
    pub(crate) fn for_route(&self, pack: &Pack) -> &Input {
        if asks_model(pack) {
            &self.facts
        } else {
            &self.plain
        }
    }
}
