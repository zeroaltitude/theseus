//! The place rule's facts (theseus-nbsh): who can view a guild channel bound
//! `private = true`, as the binding read it at its start; the owner's
//! publish; and a glide's post and read (38b).

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

/// Where a glide's words went (38b): its place, `discord:<place>` or none
/// for the CLI or the web UI, and the place's name.
#[derive(Clone, Copy)]
pub struct Where<'a> {
    pub place: Option<&'a str>,
    pub name: &'a str,
}

/// A glide's post (38b, `glide.posted`), in the frame that stages it: from
/// where to where, its characters, and how the place rule allowed it.
pub struct GlidePosted<'a> {
    pub session_id: &'a str,
    pub correlation_id: &'a str,
    pub from: Where<'a>,
    pub to: Where<'a>,
    pub chars: u64,
    /// `allowed`, or `approved` when the rule asked first.
    pub allowed: &'a str,
    /// The rule's words, when it asked.
    pub why: Option<&'a str>,
    /// The outbox post that carries it.
    pub post: &'a str,
}

impl Fact for GlidePosted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::GlidePosted);

    fn row(&self) -> Value {
        json!({"correlation_id": self.correlation_id, "from": self.from.place,
               "from_name": self.from.name, "to": self.to.place, "to_name": self.to.name,
               "chars": self.chars, "allowed": self.allowed, "why": self.why, "post": self.post})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "session {} posted to {} ({} chars, {}).",
                crate::task::short(self.session_id),
                self.to.name,
                crate::narrative::thousands(self.chars),
                how(self.allowed)
            ),
        );
    }
}

/// A glide's read (38b, `glide.read`), in the frame that writes the
/// borrowed node: from where into where, its messages and characters, how
/// the place rule allowed it, and whether it is outside text.
pub struct GlideRead<'a> {
    pub session_id: &'a str,
    pub correlation_id: &'a str,
    pub from: Where<'a>,
    pub to: Where<'a>,
    pub messages: usize,
    pub chars: u64,
    pub allowed: &'a str,
    pub why: Option<&'a str>,
    /// A shared place's: its people wrote it.
    pub outside: bool,
    /// The borrowed node, the call's result.
    pub node_id: &'a str,
}

impl Fact for GlideRead<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::GlideRead);

    fn row(&self) -> Value {
        json!({"correlation_id": self.correlation_id, "from": self.from.place,
               "from_name": self.from.name, "to": self.to.place, "to_name": self.to.name,
               "messages": self.messages, "chars": self.chars, "allowed": self.allowed,
               "why": self.why, "outside": self.outside, "node_id": self.node_id})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "session {} borrowed {} from {} ({} chars, {}{}).",
                crate::task::short(self.session_id),
                crate::narrative::count(self.messages as u64, "message", "messages"),
                self.from.name,
                crate::narrative::thousands(self.chars),
                how(self.allowed),
                if self.outside { "; outside text" } else { "" }
            ),
        );
    }
}

/// How the place rule allowed a glide, as its narrative line says it.
fn how(allowed: &str) -> &'static str {
    match allowed {
        "approved" => "asked first and approved",
        _ => "allowed",
    }
}
