//! The private mark's decision (theseus-und4g; the design's section 5): when
//! `onto.v1`'s privacy answers say a message or its pictures carry private
//! matter, the session's node is to be marked private. Pure: a judgment and
//! the bands in, the reasons out; nothing is written here.
//!
//! The mark is a policy effect (it changes what later requests may send to
//! models outside the US allowlist), so `[ontology] private_mark` ships off
//! until the owner's yes, and a switched-off mark decides nothing
//! ([`decide`] returns none). The reasons, each at the act band:
//!
//! - `personal` at *discloses* or *sensitive*: the probability of those two
//!   levels together is at least `act`;
//! - `shows_people`, `shows_identifiers`: the Noul's probability is at least
//!   `act`.
//!
//! `shows_screen_or_document` and `image_subject` mark nothing by themselves:
//! they are filing, and a screen of code is not a private matter.

use theseus_judge::client::Answer;
use theseus_judge::Judgment;

use super::decide::Bands;

/// The score level where a private matter is stated, not named in passing:
/// `personal`'s levels are none, mentions, discloses, sensitive.
const DISCLOSES: usize = 2;

/// Why the node is to be marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The message states a person's private matter.
    Personal,
    /// A picture shows a person.
    ShowsPeople,
    /// A picture shows a readable identifier.
    ShowsIdentifiers,
}

impl Reason {
    pub fn word(self) -> &'static str {
        match self {
            Reason::Personal => "personal",
            Reason::ShowsPeople => "shows_people",
            Reason::ShowsIdentifiers => "shows_identifiers",
        }
    }
}

/// The reasons `j` marks its session's node private, in the order above;
/// none when the switch is off or no answer reaches the act band.
pub fn decide(j: &Judgment, on: bool, bands: &Bands) -> Vec<Reason> {
    if !on {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(Answer::Score { probabilities, .. }) = j.answer("personal").map(|a| &a.answer) {
        let mass: f64 = probabilities.iter().skip(DISCLOSES).sum();
        if mass >= bands.act {
            out.push(Reason::Personal);
        }
    }
    for (question, reason) in [
        ("shows_people", Reason::ShowsPeople),
        ("shows_identifiers", Reason::ShowsIdentifiers),
    ] {
        if let Some(Answer::Noul { noul }) = j.answer(question).map(|a| &a.answer) {
            if *noul >= bands.act {
                out.push(reason);
            }
        }
    }
    out
}
