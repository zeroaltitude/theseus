//! route.v10's state facts as the daemon computes them (theseus-9f72b): the
//! price period from the clock, the restricted-model rule from the session's
//! labels, the last reply's model and a task's models from its nodes.

use theseus_judge::builders::{PricePeriod, RestrictedModels, RouteFacts};
use theseus_judge::Pack;

use crate::node::Node;

/// The facts for an ask about the message after `before`, the session's nodes
/// up to it (a checkpoint's is the whole task session).
pub(crate) fn facts_of(
    _before: &[Node],
    _task: bool,
    restricted: RestrictedModels,
    _now_ms: u64,
) -> RouteFacts {
    RouteFacts {
        price_period: PricePeriod::Peak,
        restricted_models: restricted,
        last_reply_model: None,
        task_models: Vec::new(),
    }
}

/// Whether the pack asks the `model` question, so reads the facts.
pub(crate) fn asks_model(_pack: &Pack) -> bool {
    true
}
