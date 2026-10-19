//! A message that reached for the operator's personal details (theseus-gasov):
//! route.v7's `personal` answer at the act band. A warning to the owner, by DM,
//! and this row; the turn goes on.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// `privacy.warned`: Jev read the message as a reach for the operator's
/// personal details (`elicit`) or as opening a conversation with others where
/// they could come out (`expose`).
pub struct PrivacyWarned<'a> {
    pub turn: &'a str,
    /// `elicit` or `expose`.
    pub choice: &'a str,
    pub confidence: f64,
    /// The place rule's class of the place: `private` or `shared`.
    pub place: &'a str,
    /// The route judgment that carried the answer.
    pub judgment: &'a str,
    /// Whether the warning was posted to the owner's DM.
    pub posted: bool,
    /// Not posted: this place's warning in the window already was.
    pub folded: bool,
}

impl Fact for PrivacyWarned<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PrivacyWarned);

    fn row(&self) -> Value {
        json!({"turn": self.turn, "choice": self.choice, "confidence": self.confidence,
               "place": self.place, "judgment": self.judgment, "posted": self.posted,
               "folded": self.folded})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let after = match (self.posted, self.folded) {
            (true, _) => "the owner was warned by DM",
            (false, true) => {
                "a warning for this place went to the owner minutes ago, so it is recorded only"
            }
            (false, false) => "no DM with the owner is bound, so it is recorded only",
        };
        say.line(
            Session,
            format!(
                "Privacy: Jev read this message as `{}` ({:.2}); {after}.",
                self.choice, self.confidence
            ),
        );
    }
}
