//! The core overage (`context_overage`), whatever built the request: when it passes the model's window by
//! the estimate with nothing left to drop (the transcript's ring, or the view's own room-making,
//! `view_step::room`), the turn fails before any call, with the window, the estimate and the overage, and
//! nothing retries it (`session::Failing`): the same request would not fit again.

use super::{Failure, Turn, TurnRunner};
use crate::compiler::Compiled;
use crate::fact::compaction::{overage_words, Overage};

/// The failure class of a turn whose request does not fit the model's window, by the estimate, before
/// anything is sent (30c).
pub const OVERAGE_CLASS: &str = "context_overage";

impl TurnRunner {
    /// The core overage: the compile made all the room it could, and its estimate still passes the window.
    /// The turn fails before any call.
    pub(super) fn overage(t: &mut Turn<'_>, compiled: &Compiled, i: u32) -> Option<Failure> {
        compiled.budget.overage.as_ref()?;
        let model = t.target.model.clone();
        let window = compiled.compilation.manifest.context_window;
        let o = Overage {
            model: &model,
            window,
            estimate: compiled.estimate.tokens,
            upper: compiled.estimate.upper,
            budget: &compiled.budget,
            loop_index: i,
        };
        let message = overage_words(&o);
        t.record(&o);
        Some(Failure {
            class: OVERAGE_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{OVERAGE_CLASS}: {message}"),
            source: anyhow::anyhow!("{message} Send something smaller, or start a new session."),
        })
    }
}
