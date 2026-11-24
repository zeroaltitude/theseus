//! route.v10's state facts as the daemon computes them (theseus-9f72b): the
//! price period from the clock, the last reply's model and a task's models
//! from its nodes.
//!
//! `restricted_models` is always `allowed`: no model is kept off a session
//! for its origin since 2026-11-21, and the switch's price reads every
//! candidate. Since route.v12 (issue 46) the builder writes it into no state;
//! the field stays on the facts, so a stored input still reads.
//!
//! Nothing here asks Jev to compute: the hour is read from the instant given,
//! so a test passes the instant and no clock is mocked.

use theseus_judge::builders::{InboundInput, RestrictedModels, RouteFacts, TASK_MODELS};
use theseus_judge::{Input, Pack};

use crate::catalog::{price_period_at, switch_cost, Catalog};
use crate::node::{Body, Node};

/// The facts for an ask about the message after `before`, the session's nodes
/// up to it (a checkpoint's is the whole task session): `task` is a task's
/// session, whose every writing model is listed once, oldest first.
pub(crate) fn facts_of(before: &[Node], task: bool, now_ms: u64) -> RouteFacts {
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
    let last = models().next_back();
    let est = est_tokens(before.iter());
    let period = price_period_at(now_ms);
    // The warm model is the one that answered last: its cache holds the prefix.
    // What a switch would cost this message, and which model the cheaper one
    // is, is the catalog's arithmetic at this hour's prices, not Jev's
    // (theseus-h66v4.1, theseus-h66v4.8).
    let switch = switch_cost(builtin(), last, est, period);
    RouteFacts {
        price_period: period,
        restricted_models: RestrictedModels::Allowed,
        last_reply_model: last.map(Box::from),
        task_models,
        est_tokens: Some(est),
        current_model: last.map(Box::from),
        switch_cost: Some(switch.cost),
        cheaper_model: switch.cheaper_model,
        previous_turn: None,
    }
}

/// The built-in catalog, built once: the facts are asked for on every message.
fn builtin() -> &'static Catalog {
    static BUILTIN: std::sync::OnceLock<Catalog> = std::sync::OnceLock::new();
    BUILTIN.get_or_init(Catalog::builtin)
}

/// How many tokens the conversation holds (the thread router reads a thread's
/// size the same way, `rpc::thread_flow`): the last request's prompt as the
/// provider counted it (new, cache-read and cache-written input), else a
/// quarter of the text's bytes. The table's "a long context" and "above 100K"
/// are read against it; Jev never works it out.
pub(crate) fn est_tokens<'a>(before: impl DoubleEndedIterator<Item = &'a Node> + Clone) -> u64 {
    let counted = before.clone().rev().find_map(|n| match &n.body {
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

    /// What the effort pack reads: its own slice of the input, which takes the facts' `previous_turn`
    /// and nothing else of them.
    pub(crate) fn for_effort(&self) -> &Input {
        &self.facts
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
