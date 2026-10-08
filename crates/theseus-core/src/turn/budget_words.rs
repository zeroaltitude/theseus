//! What a budget question says (theseus-0sg), and what a reset leaves held
//! (theseus-6g6): the wording the turn, `confirm.list`, and the Discord card
//! share. Moved out of turn.rs whole (theseus-usei).

use serde_json::Value;
use theseus_kernel::{Kernel, Micros};

use crate::narrative;

/// What a budget question says (theseus-0sg), here and wherever it is shown
/// again (`confirm.list`, the Discord card). `call` is the question's
/// `args.call`: the call's profile, model, and output cap, or `Null` for a
/// question asked before theseus-kks. A call that alone needs more than the
/// whole limit cannot fit after any reset (theseus-kks), nor can one that
/// needs more than the limit leaves once `kept` is taken, what a reset
/// leaves held (theseus-6g6), so its question says so, with the figures,
/// names the two remedies, and says what an approval then does: one more
/// try, and no second question.
pub(crate) fn budget_question(
    who: &str,
    spent: Micros,
    limit: Micros,
    needed: Micros,
    kept: Kept,
    call: &Value,
) -> String {
    if needed <= limit.saturating_sub(kept.total()) {
        return format!(
            "{who} has spent {} of its {} limit. Reset its spend to $0 and continue?",
            narrative::dollars(spent),
            narrative::dollars(limit)
        );
    }
    let model = call["model"].as_str().map_or_else(
        || "its next model call".to_string(),
        |m| format!("the call to {m}"),
    );
    let lower = match (call["profile"].as_str(), call["max_output_tokens"].as_u64()) {
        (Some(p), Some(n)) => lower_cap(p, n),
        _ => "lower the profile's `max_output_tokens`".to_string(),
    };
    if needed <= limit {
        return format!(
            "{who} is waiting on {model}, which reserves {}. Of its {} limit, {}, and a reset \
             leaves it held, so resetting its spend to $0 cannot make the call fit. Raise \
             `[kernel] spend_limit_usd` above {}, or {lower}. Approving resets the spend and \
             tries the call once more; if it still does not fit, the turn ends and does not \
             ask again.",
            narrative::dollars(needed),
            narrative::dollars(limit),
            kept.clause(),
            narrative::dollars(needed + kept.total()),
        );
    }
    format!(
        "{who} is waiting on {model}, which alone reserves {}: more than its whole {} limit, \
         so resetting its spend to $0 cannot make it fit. Raise `[kernel] spend_limit_usd` above \
         {}, or {lower}. Approving resets the spend and tries the call once more; if it still \
         does not fit, the turn ends and does not ask again.",
        narrative::dollars(needed),
        narrative::dollars(limit),
        narrative::dollars(needed),
    )
}

/// What an approved reset leaves held against an execution's limit
/// (theseus-6g6): the amounts reserved for calls in flight and held for calls
/// whose cost is unknown. A budget question and an over-limit failure name
/// them, since they are why a reset does not make a call fit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Kept {
    pub reserved: Micros,
    pub unknown: Micros,
}

impl Kept {
    /// The execution's now, or nothing held when it cannot be read.
    pub(crate) fn of(kernel: &Kernel, execution_id: &str) -> Self {
        kernel
            .execution(execution_id)
            .ok()
            .flatten()
            .map(|e| Self {
                reserved: e.budget.reserved_micros,
                unknown: e.budget.held_unknown_micros,
            })
            .unwrap_or_default()
    }

    pub(crate) fn total(self) -> Micros {
        self.reserved + self.unknown
    }

    /// What is held, as a clause that follows "of its limit,": `$1.00 is
    /// held for calls whose cost is unknown`.
    pub(crate) fn clause(self) -> String {
        let unknown = format!(
            "{} is held for calls whose cost is unknown",
            narrative::dollars(self.unknown)
        );
        let reserved = format!(
            "{} is reserved for calls in flight",
            narrative::dollars(self.reserved)
        );
        match (self.unknown > 0, self.reserved > 0) {
            (true, true) => format!("{unknown} and {reserved}"),
            (false, true) => reserved,
            _ => unknown,
        }
    }
}

/// The second remedy for a call over the whole limit (theseus-kks).
pub(crate) fn lower_cap(profile: &str, max_output_tokens: u64) -> String {
    format!(
        "lower `max_output_tokens` under `[profiles.{profile}]` (now {})",
        narrative::thousands(max_output_tokens)
    )
}
