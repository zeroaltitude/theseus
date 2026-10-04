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

/// What the binding's start found wrong with one place (`place.warned`,
/// theseus-ext.11): left unbound, a ceiling's unknown tool family, or a
/// spend limit below one call. Recorded once a start, as `place.viewed` is.
pub struct PlaceWarned<'a> {
    pub warning: &'a theseus_protocol::PlaceWarning,
}

impl Fact for PlaceWarned<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PlaceWarned);

    fn row(&self) -> Value {
        let w = self.warning;
        json!({"place": w.place, "name": w.name, "kind": w.kind, "detail": w.detail})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(Session, format!("Places: {}.", self.warning.detail));
    }
}

/// The owner published an item into a place (`place.published`): who, what
/// (its source, its digest, its size), and where, in the frame that writes it.
pub struct Published<'a> {
    pub who: &'a str,
    pub via: &'a str,
    /// `{"node_id": …}`, `{"path": …}`, or `{"text": true}`.
    pub source: &'a Value,
    pub what: &'a str,
    pub digest: &'a str,
    pub bytes: u64,
    /// `discord:channel:<id>`, and its name.
    pub place: &'a str,
    pub name: &'a str,
    /// The node written in the place's session.
    pub node_id: &'a str,
}

impl Fact for Published<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PlacePublished);

    fn row(&self) -> Value {
        json!({"who": self.who, "via": self.via, "source": self.source, "what": self.what,
               "digest": self.digest, "bytes": self.bytes, "place": self.place, "name": self.name,
               "node_id": self.node_id})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Places: {} published {} into {} ({} bytes, {}).",
                self.who, self.what, self.name, self.bytes, self.digest
            ),
        );
    }
}
