//! What Theseus said first, and how it did (theseus-slvl.9): a reply that
//! promised a follow-up and did or did not schedule one in the same turn, a
//! message the daemon sent unprompted, and the owner's verdict on it. Rows
//! only; each rides the frame its site already writes.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Context;

use super::{Fact, Say};

/// `promise.kept`: a reply promised a follow-up and its turn scheduled one (a
/// wake, a task, a watch).
pub struct PromiseKept<'a> {
    /// The phrase of the matcher's table that found the promise.
    pub phrase: &'a str,
    /// Wakes, tasks and watches the turn set.
    pub scheduled: u32,
}

impl Fact for PromiseKept<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PromiseKept);

    fn row(&self) -> Value {
        json!({"phrase": self.phrase, "scheduled": self.scheduled})
    }
}

/// `promise.unkept`: a reply promised a follow-up and its turn scheduled
/// nothing, so nothing will wake to keep it.
pub struct PromiseUnkept<'a> {
    pub phrase: &'a str,
}

impl Fact for PromiseUnkept<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::PromiseUnkept);

    fn row(&self) -> Value {
        json!({"phrase": self.phrase})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Context,
            format!(
                "Context: the reply promised a follow-up (\"{}\") and the turn scheduled none.",
                self.phrase
            ),
        );
    }
}

/// `initiative.spoke`: a message Theseus sent first. Its `id` is what a
/// verdict names; `kind` says where it came from (`wake`, `report`,
/// `morning`, `consolidation`).
pub struct InitiativeSpoke<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub persona: &'a str,
    /// A voiced message's cost and model; none for a turn's own reply.
    pub cost_usd: Option<f64>,
    pub model: Option<&'a str>,
    /// What the voiced message said (a turn's reply is its node).
    pub text: Option<&'a str>,
    /// How the model's newest call stopped, and what it wrote, thinking
    /// included (theseus-050g2): a `max_tokens` stop is never posted. When
    /// the retry was refused (the day's line) or failed, the newest call
    /// that answered is the first, cut one; a stop of none is read as
    /// finished (a stand-in model's).
    pub stop_reason: Option<&'a str>,
    pub output_tokens: Option<u64>,
}

impl Fact for InitiativeSpoke<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::InitiativeSpoke);

    fn row(&self) -> Value {
        let mut v = json!({"id": self.id, "kind": self.kind, "persona": self.persona});
        if let Some(c) = self.cost_usd {
            v["cost_usd"] = json!(c);
        }
        if let Some(m) = self.model {
            v["model"] = json!(m);
        }
        if let Some(t) = self.text {
            v["text"] = json!(t);
        }
        if let Some(s) = self.stop_reason {
            v["stop_reason"] = json!(s);
        }
        if let Some(n) = self.output_tokens {
            v["output_tokens"] = json!(n);
        }
        v
    }
}

/// `initiative.rated`: the owner's 👍 or 👎 on a message Theseus sent first.
pub struct InitiativeRated<'a> {
    pub id: &'a str,
    pub rating: &'a str,
    pub via: &'a str,
    pub by: &'a str,
}

impl Fact for InitiativeRated<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::InitiativeRated);

    fn row(&self) -> Value {
        json!({"id": self.id, "rating": self.rating, "via": self.via, "by": self.by})
    }
}
