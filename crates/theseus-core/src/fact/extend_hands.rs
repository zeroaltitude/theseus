//! An extension's hands (theseus-pw1q.14; `extend/broker.rs`): a group its
//! broker launched, the group settled, and a request refused with its
//! reason. Each is a ledger row on the extension's hands session, and a
//! line the owner reads.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Job;

use super::{Fact, Say};
use crate::narrative::{count, dollars};

/// A group the broker launched for extension `name`.
pub struct HandsLaunched<'a> {
    pub name: &'a str,
    pub group: &'a str,
    pub backend: &'a str,
    pub hands: usize,
    /// The group's worst case, reserved on the extension's day line.
    pub worst_micros: u64,
    /// What the day's line leaves after it.
    pub left_micros: u64,
}

impl Fact for HandsLaunched<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendHandsLaunched);

    fn row(&self) -> Value {
        json!({"name": self.name, "group": self.group, "backend": self.backend,
            "hands": self.hands, "worst_usd": self.worst_micros as f64 / 1e6,
            "left_usd": self.left_micros as f64 / 1e6})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "Extension {} launched {} on {} (group {}, worst case {}, {} left today).",
                self.name,
                count(self.hands as u64, "hand", "hands"),
                self.backend,
                self.group,
                dollars(self.worst_micros),
                dollars(self.left_micros)
            ),
        );
    }
}

/// An extension's group settled: met or not, or cancelled, and its cost.
pub struct HandsSettled<'a> {
    pub name: &'a str,
    pub group: &'a str,
    /// `met`, `not_met`, or `cancelled`.
    pub settled: &'a str,
    pub cost_micros: u64,
}

impl Fact for HandsSettled<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendHandsSettled);

    fn row(&self) -> Value {
        json!({"name": self.name, "group": self.group, "settled": self.settled,
            "cost_usd": self.cost_micros as f64 / 1e6})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "Extension {}'s hands group {} ended {} at {}.",
                self.name,
                self.group,
                self.settled.replace('_', " "),
                dollars(self.cost_micros)
            ),
        );
    }
}

/// A request the broker refused: its method, the reason's word, and the
/// words the extension was answered.
pub struct HandsRefused<'a> {
    pub name: &'a str,
    pub method: &'a str,
    /// `backend`, `group_size`, `cap`, `runaway`, `from_hand`, `revoked`,
    /// `not_granted`, `not_its_group`, `invalid`, or `unavailable`.
    pub reason: &'a str,
    pub why: &'a str,
    /// The refusals of the minutes before it past the bound, left unwritten
    /// (`broker::REFUSALS_A_MINUTE`).
    pub unledgered: u32,
}

impl Fact for HandsRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ExtendHandsRefused);

    fn row(&self) -> Value {
        json!({"name": self.name, "method": self.method, "reason": self.reason, "why": self.why,
            "unledgered": self.unledgered})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "Extension {}'s {} was refused ({}): {}",
                self.name, self.method, self.reason, self.why
            ),
        );
    }
}
