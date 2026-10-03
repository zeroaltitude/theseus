//! Labels' facts (M4 19a; design §2.11): who can view a guild channel, as
//! the binding read it. The compile's own (`label.withheld`) is a turn's, in
//! `turn.rs`.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};
use crate::labels::PlaceViewers;

/// A place's viewers changed (`label.audience`): its sessions' audience, so
/// their next turn recompiles for it. Recorded only on a change.
pub struct AudienceRead<'a> {
    /// `discord:<channel id>`.
    pub place: &'a str,
    pub read: &'a PlaceViewers,
    /// Why its viewers cannot be read, when they cannot.
    pub why: Option<&'a str>,
}

impl Fact for AudienceRead<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LabelAudience);

    fn row(&self) -> Value {
        json!({
            "place": self.place,
            "name": self.read.name,
            "viewers": self.read.viewers.as_ref().map(|v| v.len()),
            "why": self.why,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let at = self
            .read
            .name
            .as_deref()
            .map_or_else(|| format!("channel {}", self.place), |n| format!("#{n}"));
        let line = match (&self.read.viewers, self.why) {
            (Some(v), _) => format!(
                "Labels: {} can view {at}; its sessions admit only what they all may read.",
                crate::narrative::count(v.len() as u64, "person", "people")
            ),
            (None, Some(why)) => {
                format!("Labels: who can view {at} cannot be read ({why}), so it counts as public.")
            }
            (None, None) => {
                format!("Labels: who can view {at} cannot be read, so it counts as public.")
            }
        };
        say.line(Session, line);
    }
}
