//! An extension graduating (theseus-pw1q.13): `self.graduate.proposed`,
//! written in the backlog night's frame that files its item. Its fields are
//! `SELF_ROWS`'s (`item`, `extension`, `digest`, `calls`, `failures`,
//! `window_days`, `keel`) beside every `self.*` row's `what`, `why`,
//! `numbers` and `undo`.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Approval;

use super::{Fact, Say};

/// A loaded extension used often and never failing became a self item.
pub struct GraduateProposed {
    pub item: String,
    pub extension: String,
    pub digest: String,
    pub calls: u64,
    pub failures: u64,
    pub window_days: u32,
    /// The keel class that holds the port for the owner, if any.
    pub keel: Option<String>,
}

impl Fact for GraduateProposed {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfGraduateProposed);

    fn row(&self) -> Value {
        json!({"item": self.item, "extension": self.extension, "digest": self.digest,
               "calls": self.calls, "failures": self.failures,
               "window_days": self.window_days, "keel": self.keel,
               "what": format!("extension {} proposed for a port into core", self.extension),
               "why": format!("{} calls in {} days, {} failures", self.calls, self.window_days, self.failures),
               "numbers": {"calls": self.calls, "failures": self.failures, "window_days": self.window_days},
               "undo": format!("theseus self drop {}", self.item)})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let held = self
            .keel
            .as_ref()
            .map(|k| format!(" It touches the keel ({k}), so it waits for the owner."))
            .unwrap_or_default();
        say.line(
            Approval,
            format!(
                "Extension {} was called {} times in {} days with {} failures: filed as a port \
                 into core.{held}",
                self.extension, self.calls, self.window_days, self.failures
            ),
        );
    }
}
