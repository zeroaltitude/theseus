//! The judge's facts (M5 23a, 23b, 24; design §2.5, §2.13): what
//! `crate::judge` records of the judgments it makes. Each is a ledger row and
//! its sentences (23b). The judge runs after the turn it judges, outside every
//! turn, so no fact here draws a span: the turn's trace carries the
//! dispatch's mark (`judge::mark`; the gate's under its call's span), and a
//! live judgment's span comes with the first live pack (26b). Judgments
//! stream on `ledger.tail`; the one notification is a notified call's score
//! (`judge.scored`, step 24: a notification and nothing else, its `judge.call`
//! row the record). Their rows ride in the judge's own batched frames
//! (`judge::sink`), never in a turn's; their sentences are said once the
//! frame is written, and their metrics recorded there
//! (`Telemetry::record_judgment`). The operator's labels (`judge.label`, 24)
//! ride in the press's frame.

use serde_json::{json, Value};
use theseus_judge::band::Top;
use theseus_judge::breaker::Transition;
use theseus_judge::{Judgment, Outcome, Verdict};
use theseus_protocol::{Event, LedgerKind, NarrativePart};

use super::{Fact, Say};
use crate::judge::mark::mode_str;

/// Dollars from micro-dollars, as the narrative says them.
fn usd(micros: u64) -> String {
    format!("${:.2}", micros as f64 / 1_000_000.0)
}

/// Whether a judgment disagrees with the baseline (23b's definition): it
/// was answered by the model its pack pins, and its pack's deciding
/// questions reach the act verdict (`theseus_judge::decide`), so Jev, sure
/// enough to act, would have done otherwise than the baseline did. For
/// `loop.v1` that is `announced_unfinished` leaning true in the act band: the
/// baseline ended a turn whose last message promised more. A verdict to ask
/// is not counted: the baseline asks no one, and an ask is not an act.
pub fn disagrees(j: &Judgment) -> bool {
    j.actionable()
        && theseus_judge::pack::by_name(&j.pack)
            .is_some_and(|p| theseus_judge::decide(&p, &j.answers).verdict == Verdict::Act)
}

/// The answer a judgment is said by (23b): its pack's deciding Choice or
/// Score (the pack's own verdict: `loop.v1`'s `work_state`), else a deciding
/// Noul, else any whole answer, each by question id; else the first answer.
pub fn headline(j: &Judgment) -> Option<&theseus_judge::judge::AnswerRecord> {
    use theseus_judge::client::Kind;
    let whole = |id: &str| j.answers.iter().find(|a| a.about.is_none() && a.def == id);
    theseus_judge::pack::by_name(&j.pack)
        .and_then(|p| {
            let by = |ok: &dyn Fn(&theseus_judge::pack::QuestionDef) -> bool| {
                p.questions
                    .iter()
                    .filter(|q| ok(q))
                    .find_map(|q| whole(&q.id))
            };
            by(&|q| q.decides && q.kind != Kind::Noul)
                .or_else(|| by(&|q| q.decides))
                .or_else(|| by(&|_| true))
        })
        .or_else(|| j.answers.first())
}

/// What a point judges, as a sentence names it.
fn judged_thing(point: &str) -> &'static str {
    match point {
        "loop_end" => "the stop",
        "gate" => "the call",
        "inbound" => "the message",
        "compile" => "the context",
        "exchange_end" => "the exchange",
        _ => "the probe",
    }
}

/// The mode as the narrative says it: `in shadow`, `in canary`, `live`.
fn mode_words(mode: theseus_judge::Mode) -> &'static str {
    match mode_str(mode) {
        "shadow" => "in shadow",
        "canary" => "in canary",
        _ => "live",
    }
}

/// An answer's lean, short: `progressing`, `level 2`, `yes`, `no`.
fn lean(top: &Top) -> String {
    match top {
        Top::Choice(c) => c.clone(),
        Top::Level(n) => format!("level {n}"),
        Top::Noul(true) => "yes".into(),
        Top::Noul(false) => "no".into(),
    }
}

/// What the baseline did, from the context the core gave the judgment.
fn baseline(j: &Judgment) -> &'static str {
    match j.context.get("decision").and_then(Value::as_str) {
        Some("no_tool_calls") => "The baseline ended the turn",
        _ => "The baseline decided",
    }
}

/// The judgment's sentence: `Jev, in shadow, judged the stop: progressing
/// (0.81, act band). The baseline ended the turn; recorded, not acted on.`
pub fn sentence(j: &Judgment) -> String {
    let point = serde_json::to_value(j.point)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    let (mode, thing) = (mode_words(j.mode), judged_thing(&point));
    match &j.outcome {
        Outcome::Answered => {
            let first = headline(j).map_or_else(
                || "no answer".to_string(),
                |a| {
                    let band = serde_json::to_value(a.band.band)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default();
                    format!("{} ({:.2}, {band} band)", lean(&a.band.top), a.band.value)
                },
            );
            let mut s = format!("Jev, {mode}, judged {thing}: {first}.");
            if j.model_drift {
                s.push_str(&format!(
                    " It was answered by {}, not the pinned {}, so nothing may act on it.",
                    j.answered_by.as_deref().unwrap_or("another model"),
                    j.model
                ));
            }
            let acted = match j.mode {
                theseus_judge::Mode::Shadow => "recorded, not acted on",
                _ => "recorded",
            };
            let against = if disagrees(j) { " Jev disagrees." } else { "" };
            s.push_str(&format!(" {}; {acted}.{against}", baseline(j)));
            s
        }
        Outcome::Skipped { reason } => {
            let why = serde_json::to_value(reason)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            format!("Jev, {mode}, did not judge {thing}: skipped ({why}).")
        }
        Outcome::Failed { class, .. } => {
            format!("Jev, {mode}, could not judge {thing}: the call failed ({class}).")
        }
    }
}

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

    /// The judgment whole, the budget that paid, and (23b) whether it
    /// disagrees with the baseline, as `disagrees` defines it, and the
    /// question it is said by (`headline`).
    fn row(&self) -> Value {
        let mut v = serde_json::to_value(self.judgment).unwrap_or(Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert("budget".into(), json!(self.budget));
            o.insert("disagrees".into(), json!(disagrees(self.judgment)));
            o.insert(
                "headline".into(),
                json!(headline(self.judgment).map(|a| a.question.clone())),
            );
        }
        v
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(NarrativePart::Turn, sentence(self.judgment));
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

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            NarrativePart::Session,
            format!(
                "Shadow judging paused: today's {} is spent. It resumes at local midnight.",
                usd(self.limit_micros)
            ),
        );
    }
}

/// Shadow judging resumed (23b): the first judgment of a new local day
/// after a day it paused, in this process (a restart forgets the pause, and
/// says nothing).
pub struct JudgeResumed<'a> {
    pub day: &'a str,
    pub paused_day: &'a str,
}

impl Fact for JudgeResumed<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::JudgeResumed);

    fn row(&self) -> Value {
        json!({"day": self.day, "paused_day": self.paused_day})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            NarrativePart::Session,
            format!(
                "Shadow judging resumed: a new day ({}) after {}'s pause.",
                self.day, self.paused_day
            ),
        );
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

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            NarrativePart::Session,
            format!(
                "The judge booked {} of today's reserved block as spent: a stop lost what it settled.",
                usd(self.reserved_micros.saturating_sub(self.settled_micros))
            ),
        );
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

    fn narrate(&self, say: &mut Say<'_>) {
        let text = match self.transition {
            Transition::Opened { failures, for_secs } => format!(
                "Jev's breaker opened after {failures} failures in a row: judgments skip for {for_secs} s, then one probe."
            ),
            Transition::Reopened { for_secs } => {
                format!("Jev's probe failed: the breaker opens again for {for_secs} s.")
            }
            Transition::Closed => "Jev answered the probe: the breaker closed.".into(),
        };
        say.line(NarrativePart::Session, text);
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

    fn narrate(&self, say: &mut Say<'_>) {
        let n = match self.shed {
            1 => "1 shadow judgment".to_string(),
            n => format!("{n} shadow judgments"),
        };
        say.line(
            NarrativePart::Session,
            format!("Jev shed {n}: every in-flight permit was taken."),
        );
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
