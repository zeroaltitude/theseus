//! The answers' facts: an answer to a call that waited or to a budget
//! question, an act that did not count, and a session following the spend
//! limit (`rpc/confirms.rs`).

use serde_json::{json, Value};
use theseus_kernel::{Action, LimitFollowed, Micros};
use theseus_protocol::NarrativePart::{Approval, Session};
use theseus_protocol::{by_words, notify, ConfirmResolved, Event, LedgerKind};

use super::{Fact, Say};
use crate::approval::Refusal;
use crate::narrative;
use crate::rpc::Act;

/// The operator answered a call that waited (`action.confirm_answered`,
/// `confirm.resolved`): its row rides in the answer's transaction
/// (theseus-jj9f), and its clients hear it once that frame is written.
pub struct CallAnswered<'a> {
    pub action: &'a Action,
    pub approve: bool,
    pub note: Option<&'a str>,
    /// Who answered, and through what.
    pub by: &'a str,
    pub via: &'a str,
    /// The option it is (`approve`, `decline`, `accept`, `ack`, ...), as
    /// `confirm.resolved` carries it; none for an expiry (theseus-ds5os).
    pub option: Option<&'a str>,
}

impl Fact for CallAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionConfirmAnswered);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        json!({"correlation_id": self.action.correlation_id, "approved": self.approve, "note": self.note, "by": self.by, "via": self.via,
               "option": self.option})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.action.session_id.clone(),
            correlation_id: self.action.correlation_id.clone(),
            approved: self.approve,
            by: Some(self.by.into()),
            option: self.option.map(str::to_string),
            ..Default::default()
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "{} {} by {}{}.",
                self.action.tool,
                if self.approve { "approved" } else { "declined" },
                by_words(self.by),
                if self.note.is_some_and(|n| !n.trim().is_empty()) {
                    ", with a note"
                } else {
                    ""
                }
            ),
        );
    }
}

/// The answer woke its execution: the driver resumes the turn.
pub struct WokenByAnswer {
    pub approve: bool,
}

impl Fact for WokenByAnswer {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Session,
            format!(
                "Woken by the {}; the driver resumes the turn.",
                if self.approve { "approval" } else { "decline" }
            ),
        );
    }
}

/// The operator answered a budget question (theseus-0sg;
/// `action.confirm_answered`, `confirm.resolved`): its row rides in the
/// answer's transaction.
pub struct BudgetAnswered<'a> {
    pub question: &'a Action,
    pub approve: bool,
    pub note: Option<&'a str>,
    pub by: &'a str,
    pub via: &'a str,
    /// `reset` or `decline` (theseus-ds5os).
    pub option: Option<&'a str>,
}

impl Fact for BudgetAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionConfirmAnswered);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        let q = self.question;
        json!({"correlation_id": q.correlation_id, "approved": self.approve, "note": self.note, "by": self.by, "via": self.via, "tool": q.tool, "option": self.option})
    }

    fn event(&self) -> Option<Event> {
        let q = self.question;
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: q.session_id.clone(),
            correlation_id: q.correlation_id.clone(),
            approved: self.approve,
            by: Some(self.by.into()),
            option: self.option.map(str::to_string),
            ..Default::default()
        }))
    }
}

/// The spend went back to $0, and the waiting call proceeds; the session's
/// lifetime cost is untouched.
pub struct SpendReset<'a> {
    pub by: &'a str,
    /// What it had spent, and its limit.
    pub before: Micros,
    pub limit: Micros,
    /// The question was a burn-rate pause's (theseus-9e02): the session is
    /// resumed and its spend stays.
    pub resumed: bool,
}

impl Fact for SpendReset<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        if self.resumed {
            say.line(
                Approval,
                format!(
                    "Resumed by {} after the burn-rate pause; its ten-minute window starts \
                     empty and its spend stays at {}; continuing.",
                    by_words(self.by),
                    narrative::dollars(self.before)
                ),
            );
            return;
        }
        say.line(
            Approval,
            format!(
                "Spend reset to $0 by {} (it was {} of the {} limit); continuing.",
                by_words(self.by),
                narrative::dollars(self.before),
                narrative::dollars(self.limit)
            ),
        );
    }
}

/// The reset was declined: the session keeps waiting on its budget, and its
/// next message asks again.
pub struct ResetDeclined<'a> {
    pub by: &'a str,
}

impl Fact for ResetDeclined<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "The budget reset was declined by {}; the session keeps waiting, and a new \
                 message asks again.",
                by_words(self.by)
            ),
        );
    }
}

/// An approval-like act did not count (theseus-sgh; `approval.refused`):
/// who, through what, and why.
pub struct ActRefused<'a> {
    pub(crate) act: Act<'a>,
    pub refusal: &'a Refusal,
    /// The label of who answered.
    pub by: &'a str,
}

impl ActRefused<'_> {
    /// Whose it is: the answered call's session, or the unlabelled one's.
    pub fn session(&self) -> Option<&str> {
        match self.act {
            Act::Answer { action: a, .. } => Some(a.session_id.as_str()),
            Act::Tighten { .. } | Act::Untighten { .. } => None,
            Act::Unlabel { session, .. } => Some(session),
            Act::Publish { .. }
            | Act::Ontology { .. }
            | Act::Label { .. }
            | Act::JudgeLabel { .. }
            | Act::RouteCorrect { .. }
            | Act::Revoke { .. }
            | Act::JudgeRun { .. }
            | Act::Ladder { .. }
            | Act::Import { .. }
            | Act::Session { .. }
            | Act::SelfResume { .. }
            | Act::SelfBacklog { .. }
            | Act::SelfNarrow { .. }
            | Act::SelfRevert { .. }
            | Act::Retract { .. }
            | Act::Task { .. }
            | Act::Stop { .. }
            | Act::Standing { .. }
            | Act::Routine { .. }
            | Act::Watch { .. }
            | Act::Thread { .. } => None,
        }
    }
}

impl Fact for ActRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ApprovalRefused);

    fn row(&self) -> Value {
        let (r, act) = (self.refusal, self.act);
        match act {
            Act::Answer { action: a, approve } => {
                json!({"correlation_id": a.correlation_id, "tool": a.tool, "approve": approve,
                       "who": r.who, "via": r.via, "why": r.why, "by": self.by})
            }
            Act::Tighten { tool } | Act::Untighten { tool } => {
                json!({"act": act.method(), "tool": tool, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::Publish { place } => {
                json!({"act": act.method(), "place": place, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::Unlabel { session, what } => {
                json!({"act": act.method(), "session_id": session, "what": what, "who": r.who,
                       "via": r.via, "why": r.why, "by": self.by})
            }
            Act::Ontology { what, .. }
            | Act::Label { what }
            | Act::Retract { what }
            | Act::Task { what }
            | Act::JudgeLabel { what }
            | Act::RouteCorrect { what }
            | Act::JudgeRun { what, .. }
            | Act::Ladder { what, .. }
            | Act::Import { what, .. }
            | Act::Session { what, .. }
            | Act::Standing { what, .. }
            | Act::Routine { what, .. }
            | Act::Watch { what, .. }
            | Act::Thread { what, .. } => {
                json!({"act": act.method(), "what": what, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::Stop { job } => {
                json!({"act": act.method(), "job": job, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::Revoke { name } => {
                json!({"act": act.method(), "name": name, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::SelfResume { from_job } => {
                json!({"act": act.method(), "from_job": from_job, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::SelfBacklog { what, from_job, .. }
            | Act::SelfNarrow { what, from_job, .. }
            | Act::SelfRevert { what, from_job } => {
                json!({"act": act.method(), "what": what, "from_job": from_job, "who": r.who,
                       "via": r.via, "why": r.why, "by": self.by})
            }
        }
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let (who, via, why) = (&self.refusal.who, &self.refusal.via, &self.refusal.why);
        let line = match self.act {
            Act::Answer { action: a, .. } => format!(
                "An answer to {} from {} through {} did not count: {}. It keeps waiting.",
                a.tool, who, via, why
            ),
            Act::Tighten { tool } => format!(
                "\"Should have asked\" for {tool} from {who} through {via} did not count: \
                 {why}. It keeps its posture."
            ),
            Act::Untighten { tool } => format!(
                "An undo of {tool}'s tightening from {who} through {via} did not count: {why}. \
                 It keeps asking first."
            ),
            Act::Publish { place } => format!(
                "Publishing into {place}, from {who} through {via}, did not count: {why}. \
                 Nothing was published."
            ),
            Act::Ontology { what, .. } => format!(
                "Writing the ontology's {what}, from {who} through {via}, did not count: \
                 {why}. Nothing was written."
            ),
            Act::Label { what } => format!(
                "A memory label, {what}, from {who} through {via}, did not count: {why}. \
                 Nothing was written."
            ),
            Act::Ladder { what, .. } => format!(
                "A move on the ladder, {what}, from {who} through {via}, did not count: {why}. \
                 Nothing was written."
            ),
            Act::Import { what, .. } => format!(
                "The import's {what}, from {who} through {via}, did not count: {why}. Nothing \
                 was written."
            ),
            Act::Session { what, .. } | Act::Unlabel { what, .. } => {
                format!(
                    "{what}, from {} through {}, did not count: {}. The session is as it was.",
                    who, via, why
                )
            }
            Act::Task { what } => format!(
                "{what}, from {who} through {via}, did not count: {why}. No task was started \
                 and nothing was carved."
            ),
            Act::Stop { job } => format!(
                "Stopping job {job}, from {who} through {via}, did not count: {why}. The job \
                 runs on."
            ),
            Act::Retract { what } => format!(
                "{what}, from {who} through {via}, did not count: {why}. Nothing was labeled \
                 or written, and no session was told."
            ),
            Act::Thread { what, .. } => format!(
                "{what}, from {} through {}, did not count: {}. The threads are as they were.",
                who, via, why
            ),
            Act::Routine { what, .. } => format!(
                "{what}, from {who} through {via}, did not count: {why}. The routines are as \
                 they were."
            ),
            Act::Watch { what, .. } => format!(
                "{what}, from {who} through {via}, did not count: {why}. No watch was made."
            ),
            Act::Standing { what, .. } => format!(
                "{what}, from {who} through {via}, did not count: {why}. The owner's standing \
                 records are as they were."
            ),
            Act::JudgeLabel { what } => format!(
                "A judgment's label, {what}, from {who} through {via}, did not count: {why}. \
                 Nothing was written."
            ),
            Act::RouteCorrect { what } => format!(
                "A correction of routing, {what}, from {who} through {via}, did not count: \
                 {why}. Nothing was written, and the session runs where it ran."
            ),
            a @ (Act::SelfResume { .. }
            | Act::SelfNarrow { .. }
            | Act::SelfBacklog { .. }
            | Act::SelfRevert { .. }) => self_line(a, who, via, why),
            Act::Revoke { name } => format!(
                "Revoking extension {name}, from {who} through {via}, did not count: {why}. It \
                 stays loaded."
            ),
            Act::JudgeRun { what, .. } => format!(
                "A {what}, from {who} through {via}, did not count: {why}. Nothing was sent or \
                 written."
            ),
        };
        say.line(Approval, line);
    }
}

/// A refused self-improvement act's line (`ActRefused::narrate`'s).
fn self_line(act: Act<'_>, who: &str, via: &str, why: &str) -> String {
    match act {
        Act::SelfResume { .. } => format!(
            "A resume of self-improvement, from {who} through {via}, did not count: {why}. \
             It stays halted."
        ),
        Act::SelfNarrow { what, .. } => format!(
            "A change to a hand role's policy ({what}), from {who} through {via}, did not \
             count: {why}. The policy is as it was."
        ),
        Act::SelfBacklog { method, what, .. } => format!(
            "{method} of {what} on the self backlog, from {who} through {via}, did not \
             count: {why}. The backlog is as it was."
        ),
        Act::SelfRevert { what, .. } => format!(
            "A veto of self-join {what}, from {who} through {via}, did not count: {why}. \
             Nothing was reverted."
        ),
        _ => unreachable!("only the self acts come here"),
    }
}

/// An open session follows the config's spend limit (theseus-3pj).
pub struct LimitChanged<'a> {
    pub followed: &'a LimitFollowed,
    /// The place whose ceiling chose the limit (step 38a); none: the config.
    pub place: Option<&'a str>,
}

impl Fact for LimitChanged<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        let f = self.followed;
        let then = if f.proceeds {
            "; the call that waited at the old limit proceeds"
        } else if f.to_micros < f.from_micros {
            "; its next call that does not fit asks"
        } else {
            ""
        };
        say.line(
            Session,
            format!(
                "Session {} follows {}: {} before, {} now{then}.",
                narrative::short(&f.session_id),
                match self.place {
                    Some(p) =>
                        format!("{p}'s spend limit, the lower of its ceiling and the config's"),
                    None => "the config's spend limit".into(),
                },
                narrative::dollars(f.from_micros),
                narrative::dollars(f.to_micros)
            ),
        );
    }
}

/// A raised limit withdrew a budget question: the call proceeds, and its
/// clients hear the question closed (`confirm.resolved`).
pub struct QuestionWithdrawn<'a> {
    pub session_id: &'a str,
    pub question: &'a str,
}

impl Fact for QuestionWithdrawn<'_> {
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.session_id.into(),
            correlation_id: self.question.into(),
            by: Some("config".into()),
            withdrawn: true,
            ..Default::default()
        }))
    }
}

/// Nobody answered a call's question by the time its card said it expires
/// (theseus-830; `action.expired`, `confirm.resolved` expired): the call is
/// declined in the same frame, its execution woken, and the model reads that
/// it was not run. Its row rides in that frame.
pub struct QuestionExpired<'a> {
    pub action: &'a Action,
    /// How long it waited: `[kernel] confirm_ttl_secs`.
    pub waited_ms: u64,
}

impl Fact for QuestionExpired<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionExpired);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        let a = self.action;
        json!({"correlation_id": a.correlation_id, "tool": a.tool, "asked_at_ms": a.planned_at_ms,
               "waited_ms": self.waited_ms})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.action.session_id.clone(),
            correlation_id: self.action.correlation_id.clone(),
            expired: true,
            ..Default::default()
        }))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let asked = self.action.tool == crate::ask::OWNER_ASK;
        say.line(
            Approval,
            format!(
                "{} expired: nobody answered within {}, so {}; the driver resumes the turn.",
                self.action.tool,
                within(self.waited_ms),
                match asked {
                    true => "its default applies (or the model decides)",
                    false => "it was not run",
                }
            ),
        );
    }
}

/// A question's wait, as its expiry says it: `15 minutes`, `90 seconds`.
pub fn within(ms: u64) -> String {
    let plural = |n: u64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    if ms.is_multiple_of(60_000) {
        plural(ms / 60_000, "minute")
    } else if ms.is_multiple_of(1000) {
        plural(ms / 1000, "second")
    } else {
        format!("{ms} ms")
    }
}
