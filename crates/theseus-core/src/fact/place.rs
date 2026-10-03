//! The place rule's facts (theseus-nbsh): who can view a guild channel bound
//! `private = true`, as the binding read it at its start.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};
use crate::places::Viewed;

/// A guild channel bound `private = true`, read at the binding's start
/// (`place.viewed`): who besides the owner can view it, or why that cannot
/// be read. Health warns while anyone can.
pub struct PlaceViewed<'a> {
    /// `discord:channel:<id>`.
    pub place: &'a str,
    /// `#lab`.
    pub name: &'a str,
    pub viewed: &'a Viewed,
}

impl Fact for PlaceViewed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PlaceViewed);

    fn row(&self) -> Value {
        match self.viewed {
            Viewed::Others(o) => json!({"place": self.place, "name": self.name, "others": o}),
            Viewed::Unread(why) => json!({"place": self.place, "name": self.name, "unread": why}),
        }
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let name = self.name;
        let line = match self.viewed {
            Viewed::Others(o) if o.is_empty() => {
                format!("Places: only the owner can view {name}, which is bound private.")
            }
            Viewed::Others(o) => format!(
                "Places: {name} is bound private, but {} besides the owner can view it: {}.",
                crate::narrative::count(o.len() as u64, "person", "people"),
                o.join(", ")
            ),
            Viewed::Unread(why) => {
                format!("Places: who can view {name}, bound private, cannot be read ({why}).")
            }
        };
        say.line(Session, line);
    }
}
