//! A task's limits that notify (theseus-usei): the spend limit and the loop
//! cap reached, each a notice to the session's place, its row, and its
//! narrative line, and the work going on. The row and the post ride in the
//! turn's next frame.

use serde_json::{json, Value};
use theseus_kernel::{micros_to_usd, Micros};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// `budget.reached`: the session's spend reached its limit, or a multiple
/// of it (`multiple`: 1 at the limit, 2 at twice it, …), under a limit that
/// notifies; the call went on.
pub struct SpendReached<'a> {
    pub spent: Micros,
    pub limit: Micros,
    pub multiple: u64,
    /// The session's lifetime cost, which no reset lowers.
    pub lifetime_usd: f64,
    /// Whether the notice was posted to a place (a session with none is
    /// heard in its clients and the cockpit alone).
    pub posted: bool,
    /// The notice's words.
    pub text: &'a str,
}

impl Fact for SpendReached<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::BudgetReached);

    fn row(&self) -> Value {
        json!({"mode": "notify", "spent_usd": micros_to_usd(self.spent),
               "limit_usd": micros_to_usd(self.limit), "multiple": self.multiple,
               "next_usd": micros_to_usd(self.limit.saturating_mul(self.multiple + 1)),
               "lifetime_usd": self.lifetime_usd, "posted": self.posted})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, self.text.to_string());
    }
}

/// `loop.cap_reached`: the turn reached its profile's `max_loops`, or a
/// multiple of it, under a cap that notifies; the turn went on.
pub struct LoopsReached<'a> {
    pub loops: u32,
    pub max_loops: u32,
    pub multiple: u32,
    pub posted: bool,
    pub text: &'a str,
}

impl Fact for LoopsReached<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LoopCapReached);

    fn row(&self) -> Value {
        json!({"mode": "notify", "loops": self.loops, "max_loops": self.max_loops,
               "multiple": self.multiple, "posted": self.posted})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, self.text.to_string());
    }
}
