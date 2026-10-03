//! Labels' facts (M4 19a; design §2.11): who can view a guild channel, as
//! the binding read it; a graduation, a held post, and its answer (19c). The
//! compile's own (`label.withheld`) is a turn's, in `turn.rs`.

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

/// The operator graduated a node (`label.graduated`, M4 19c): a new node with
/// its content, wider readers, and the warrant, in the frame that writes it.
pub struct Graduated<'a> {
    /// The graduated node.
    pub node: &'a crate::node::Node,
    pub source: &'a crate::node::Node,
    /// The process that asked, when the connection knew one.
    pub asker: Value,
}

impl Fact for Graduated<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LabelGraduated);

    fn row(&self) -> Value {
        let label = self.node.label.as_ref();
        let w = label.and_then(|l| l.warrant.as_ref());
        let mut row = json!({
            "node_id": self.node.id,
            "graduated_from": self.source.id,
            "from_readers": self.source.label.as_ref().map(|l| l.readers.describe()),
            "readers": label.map(|l| &l.readers),
            "integrity": label.map(|l| l.integrity),
            "who": w.map(|w| &w.who),
            "how": w.map(|w| &w.how),
            "why": w.map(|w| &w.why),
        });
        if !self.asker.is_null() {
            row["asker"] = self.asker.clone();
        }
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let (Some(l), Some(w)) = (
            self.node.label.as_ref(),
            self.node.label.as_ref().and_then(|l| l.warrant.as_ref()),
        ) else {
            return;
        };
        say.line(
            Session,
            format!(
                "Graduated {} by {}: a copy readable {} ({}), which the next compile admits. \
                 Why: {}",
                self.source.id,
                w.who,
                l.readers.describe(),
                self.node.id,
                w.why
            ),
        );
    }
}

/// The outbox held a post for the owner (`label.held_post`, M4 19c): its
/// place's audience no longer fits what it draws on. In the frame that plans
/// its question and its card.
pub struct PostHeld<'a> {
    pub post: &'a theseus_kernel::Action,
    pub question: &'a str,
    pub readers: &'a theseus_protocol::Readers,
    /// Who can view its place now.
    pub audience: &'a theseus_protocol::Audience,
    /// How long the binding's read of that took.
    pub read_ms: f64,
}

impl Fact for PostHeld<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LabelHeldPost);

    fn row(&self) -> Value {
        json!({
            "post": self.post.correlation_id,
            "kind": crate::outbox::kind_of(self.post),
            "place": crate::outbox::target_of(self.post),
            "question": self.question,
            "readers": self.readers,
            "audience": self.audience,
            "read_ms": self.read_ms,
        })
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Held a reply for the owner: it draws on material labeled {}, and who can view \
                 its place changed since it was written (now {}). It waits for an answer \
                 ({}).",
                self.readers.describe(),
                self.audience.describe(),
                self.question
            ),
        );
    }
}

/// The owner answered a held post's question (`label.held_post_answered`, M4
/// 19c): approved, it goes; declined, its place gets the note. In the frame
/// that settles the question.
pub struct HeldPostAnswered<'a> {
    pub question: &'a theseus_kernel::Action,
    pub post: &'a str,
    pub approve: bool,
    pub by: &'a str,
    pub via: &'a str,
    pub asker: Value,
}

impl Fact for HeldPostAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::LabelHeldPostAnswered);

    fn row(&self) -> Value {
        let mut row = json!({
            "question": self.question.correlation_id,
            "post": self.post,
            "approved": self.approve,
            "by": self.by,
            "via": self.via,
        });
        if !self.asker.is_null() {
            row["asker"] = self.asker.clone();
        }
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let then = if self.approve {
            "it goes to its place now"
        } else {
            "its place gets \"a reply was held back\" instead"
        };
        say.line(
            Session,
            format!(
                "The held reply was {} by {} through {}: {then}.",
                if self.approve { "approved" } else { "declined" },
                self.by,
                self.via
            ),
        );
    }
}
