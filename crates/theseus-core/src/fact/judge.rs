//! The judge's facts (M5 23a; design §2.5): what `crate::judge` records of
//! the judgments it makes. Each is a ledger row and nothing else in 23a: the
//! judge runs after the turn it judges, outside every turn, and its
//! sentences and spans are step 23b's. Its rows ride in the judge's own
//! batched frames (`judge::sink`), never in a turn's. Step 24 adds a
//! notified call's score (`judge.scored`, a notification and nothing else:
//! its `judge.call` row is the record), and the operator's labels
//! (`judge.label`), which ride in the press's frame.

use serde_json::{json, Value};
use theseus_judge::breaker::Transition;
use theseus_judge::Judgment;
use theseus_protocol::{Event, LedgerKind};

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

/// A notified call's `security.v1` score landed, after its notice
/// (`judge.scored`; step 24, design §2.8b): told to the turn's clients, who
/// add `risk N% (shadow)` to the notice's line. No row: the judgment's
/// `judge.call` row is the record.
pub struct JudgeScored<'a> {
    pub scored: &'a theseus_protocol::judge::JudgeScored,
}

impl Fact for JudgeScored<'_> {
    const METHOD: Option<&'static str> = Some(theseus_protocol::notify::JUDGE_SCORED);

    fn event(&self) -> Option<Event> {
        Some(Event::JudgeScored(self.scored.clone()))
    }
}

/// A label on a judgment (`judge.label`, design §2.5): keyed `lbl_<id>` and
/// scoped `judge:<pack id>` by its writer. In step 24 the operator's "should
/// have asked" press on a call labels each of that call's `gate` judgments
/// `risky`, at weight 1.0, in the press's frame. Declines and approvals are
/// system labels the learning ledger derives nightly (§2.9, step 25c).
pub struct JudgeLabel<'a> {
    pub id: &'a str,
    pub judgment: &'a str,
    pub pack: &'a str,
    /// The question it labels, or `None` for all of them.
    pub question: Option<&'a str>,
    pub label: Value,
    /// `operator`, `system`, or `audit`.
    pub source: &'a str,
    pub who: &'a str,
    /// Through what: `cli`, `discord`, `web`.
    pub via: &'a str,
    pub weight: f64,
    pub note: &'a str,
    /// The call it was of, when it was a call's.
    pub correlation_id: Option<&'a str>,
}

impl Fact for JudgeLabel<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeLabel);

    fn row(&self) -> Value {
        json!({"id": self.id, "judgment": self.judgment, "pack": self.pack,
            "question": self.question, "label": self.label, "source": self.source,
            "who": self.who, "via": self.via, "weight": self.weight, "note": self.note,
            "correlation_id": self.correlation_id})
    }
}
