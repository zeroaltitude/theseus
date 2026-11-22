//! The core overage, whatever built the request: when it passes the model's window by the estimate with
//! nothing left to drop (the view's own room-making, `view_step::room`), the turn fails before any call,
//! with the window, the estimate and the overage, and nothing retries it (`session::Failing`): the same
//! request would not fit again. Its class is the window's (`WINDOW_CLASS`): one name for a person, whether
//! the estimate or the provider found the request too large. Rows written before carry `context_overage`,
//! which readers still take as the window's (`routing::effort::previous`).

use super::{Failure, Turn, TurnRunner, WINDOW_CLASS};
use crate::compiler::Compiled;
use crate::fact::context::{overage_words, Overage};

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
            class: WINDOW_CLASS.into(),
            transient: false,
            usage_unknown: false,
            reason: format!("{WINDOW_CLASS}: {message}"),
            source: anyhow::anyhow!("{message} Send something smaller, or start a new session."),
        })
    }
}
