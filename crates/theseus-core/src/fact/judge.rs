//! The judge's facts (M5 23a; design §2.5): what `crate::judge` records of
//! the judgments it makes. Each is a ledger row and nothing else in 23a: the
//! judge runs after the turn it judges, outside every turn, and its
//! notifications, sentences, and spans are step 23b's. Its rows ride in the
//! judge's own batched frames (`judge::sink`), never in a turn's.

use serde_json::{json, Value};
use theseus_judge::breaker::Transition;
use theseus_judge::Judgment;
use theseus_protocol::LedgerKind;

use super::Fact;

/// One judgment, whatever became of it: answered, skipped, or failed. The
/// row is the judgment whole (pack and version, the state's digest and
/// size, every answer with its band, usage, cost, timing, the outcome and
/// its error class, and the core's context), keyed by its id and scoped
/// `judge:<pack id>` by the sink, with the budget that paid.
pub struct JudgeCall<'a> {
    pub judgment: &'a Judgment,
    /// `shadow`: the judge's own day budget (§2.6).
    pub budget: &'a str,
}

impl Fact for JudgeCall<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeCall);

    fn row(&self) -> Value {
        let mut v = serde_json::to_value(self.judgment).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("budget".into(), json!(self.budget));
        }
        v
    }
}

/// The shadow budget reached its day's limit: shadow pauses until local
/// midnight, said once a day.
pub struct JudgePaused<'a> {
    pub day: &'a str,
    pub limit_micros: u64,
    pub spent_micros: u64,
    pub needed_micros: u64,
}

impl Fact for JudgePaused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgePaused);

    fn row(&self) -> Value {
        json!({"day": self.day, "limit_micros": self.limit_micros, "spent_micros": self.spent_micros,
            "needed_micros": self.needed_micros})
    }
}

/// A start found today's block reserved past what its rows settled (a crash
/// lost up to a block's rest): the rest is booked as spent, conservatively.
pub struct JudgeBlockBooked<'a> {
    pub day: &'a str,
    pub reserved_micros: u64,
    pub settled_micros: u64,
}

impl Fact for JudgeBlockBooked<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeBlockBooked);

    fn row(&self) -> Value {
        json!({"day": self.day, "reserved_micros": self.reserved_micros,
            "settled_micros": self.settled_micros,
            "booked_micros": self.reserved_micros.saturating_sub(self.settled_micros)})
    }
}

/// The breaker opened, opened again after a failed probe, or closed.
pub struct JudgeCircuit<'a> {
    pub transition: &'a Transition,
    /// The judgment whose call moved it.
    pub judgment: &'a str,
}

impl Fact for JudgeCircuit<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeCircuit);

    fn row(&self) -> Value {
        json!({"transition": self.transition, "judgment": self.judgment})
    }
}

/// Shadow judgments shed for want of a free in-flight permit since the
/// last such row (one a minute at most).
pub struct JudgeShed {
    pub shed: u64,
}

impl Fact for JudgeShed {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeShed);

    fn row(&self) -> Value {
        json!({"shed": self.shed})
    }
}

/// The operator's label on a judgment (design §2.5's `judge.label`; M5
/// 28b): an accepted or rejected `categorize.v1` proposal. Keyed `lbl_<id>`
/// and scoped `judge:<pack id>` by its writer, in the frame that writes the
/// membership an accept makes. Labels are rows, never edits, so the
/// learning ledger (25c) takes them over as they are.
pub struct JudgeLabel<'a> {
    /// `lbl_<uuid v7>`.
    pub id: &'a str,
    pub judgment: &'a str,
    /// `categorize.v1`.
    pub pack: &'a str,
    /// The question labelled (`topic`).
    pub question: &'a str,
    /// `accepted` or `rejected`.
    pub label: &'a str,
    /// What the judgment answered (`harbor`, `new_topic`).
    pub answer: &'a str,
    /// The topic the session joined, on an accept.
    pub topic: Option<&'a str>,
    pub who: &'a str,
    pub via: &'a str,
    pub note: Option<&'a str>,
}

impl Fact for JudgeLabel<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeLabel);

    fn row(&self) -> Value {
        json!({"id": self.id, "judgment": self.judgment, "pack": self.pack,
               "question": self.question, "label": self.label, "answer": self.answer,
               "topic": self.topic, "source": "operator", "who": self.who, "via": self.via,
               "weight": 1.0, "note": self.note})
    }

    fn narrate(&self, say: &mut super::Say<'_>) {
        let what = match (self.label, self.topic) {
            ("accepted", Some(t)) => format!("accepted Jev's proposal, and the session joins {t}"),
            _ => format!("rejected Jev's proposal ({})", self.answer),
        };
        say.line(
            theseus_protocol::NarrativePart::Session,
            format!("Ontology: {} {what} ({}).", self.who, self.judgment),
        );
    }
}
