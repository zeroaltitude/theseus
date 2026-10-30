//! route.v10's state facts (theseus-9f72b, design §1.4 step 3): what the daemon
//! knows and Jev is not asked to work out. The four of them ride the shared
//! `inbound` state beside the message, and a state without them is byte for byte
//! the one before.
//!
//! - **price_period**: `peak` or `off_peak`, read from the clock by the daemon
//!   (`catalog::price_period_at`). Jev is weak at times and arithmetic, so it
//!   never works the hour out.
//! - **restricted_models**: `allowed` or `not_allowed`, the data rule's voice in
//!   the state. The daemon refuses a restricted pick anyway; the state says so
//!   to save the ask.
//! - **last_reply_model**: the model that answered the previous turn.
//! - **task_models**: every model that has written in the task (a checkpoint's).

use serde::{Deserialize, Serialize};

use serde_json::json;

use super::{Clipper, InboundInput};
use crate::state::StateBuilder;

/// The most models `task_models` names. A task has five option models and
/// whatever the operator pinned; the cut keeps the state's size fixed.
pub const TASK_MODELS: usize = 8;

/// A model id in the state.
const MODEL_CHARS: usize = 48;

/// Whether the model's provider charges its peak or its off-peak price now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PricePeriod {
    Peak,
    OffPeak,
}

impl PricePeriod {
    pub fn as_str(self) -> &'static str {
        match self {
            PricePeriod::Peak => "peak",
            PricePeriod::OffPeak => "off_peak",
        }
    }
}

/// Whether a model of restricted (China or unknown) origin may be picked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestrictedModels {
    Allowed,
    /// The default: where the daemon has not read the labels, it fails closed.
    #[default]
    NotAllowed,
}

impl RestrictedModels {
    pub fn as_str(self) -> &'static str {
        match self {
            RestrictedModels::Allowed => "allowed",
            RestrictedModels::NotAllowed => "not_allowed",
        }
    }

    /// `Allowed` only when `allowed`: the fail-closed constructor.
    pub fn allowed_if(allowed: bool) -> Self {
        if allowed {
            RestrictedModels::Allowed
        } else {
            RestrictedModels::NotAllowed
        }
    }
}

/// The facts, as the daemon computes them for one ask.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteFacts {
    pub price_period: PricePeriod,
    pub restricted_models: RestrictedModels,
    /// The model that answered the previous turn; left out when none did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reply_model: Option<String>,
    /// Every model that has written in this task, oldest first; left out
    /// outside a task.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub task_models: Vec<String>,
    /// How many tokens the conversation holds, as far as the daemon knows at
    /// the ask (its last request's prompt, else its text): the number the
    /// table's "a long context" and "above 100K" are read against. Left out
    /// where there is none (a job).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est_tokens: Option<u64>,
}

/// The fields of a route ask's state beyond a message's own dynamic ones
/// (`previous_human_message`, `last_reply`, `minutes_since_last_message`),
/// for a plain message in a conversation that has had a reply and is no
/// task: the one list the daemon's state and a replay case's state are both
/// held to (`tests_route_state`).
pub const ROUTE_STATE_KEYS: [&str; 9] = [
    "author",
    "current_role",
    "est_tokens",
    "last_reply_model",
    "live_tasks",
    "message",
    "place_kind",
    "price_period",
    "restricted_models",
];

/// A replay case's input: the message and the facts, as the daemon would send
/// them for a plain message in `place_kind`, and nothing else. A replay builds
/// its state with [`super::inbound`] over this, never beside it, so what Jev is
/// shown in a replay is what the daemon shows it.
pub fn route_input(message: &str, place_kind: &str, facts: RouteFacts) -> InboundInput {
    InboundInput {
        message: message.into(),
        author: "operator".into(),
        place_kind: place_kind.into(),
        route: Some(facts),
        ..InboundInput::default()
    }
}

/// The facts in the inbound state, when the input carries them. Every one is
/// a scalar, which the state keeps until everything else is gone, and each is
/// bounded here: two enum words, one model id and at most [`TASK_MODELS`] more.
pub(super) fn state(b: &mut StateBuilder, c: &Clipper, i: &InboundInput) {
    let Some(f) = &i.route else {
        return;
    };
    b.scalar("price_period", f.price_period.as_str());
    b.scalar("restricted_models", f.restricted_models.as_str());
    if let Some(n) = f.est_tokens {
        b.scalar("est_tokens", n);
    }
    if let Some(m) = &f.last_reply_model {
        b.scalar("last_reply_model", c.clip(m, MODEL_CHARS))
            .cut_if("last_reply_model", c.cut());
    }
    if !f.task_models.is_empty() {
        let models: Vec<String> = f
            .task_models
            .iter()
            .take(TASK_MODELS)
            .map(|m| c.clip(m, MODEL_CHARS))
            .collect();
        b.scalar("task_models", json!(models))
            .cut_if("task_models", c.cut());
    }
}
