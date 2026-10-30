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
}

/// The facts in the inbound state, when the input carries them.
pub(super) fn state(_b: &mut StateBuilder, _c: &Clipper, _i: &InboundInput) {}
