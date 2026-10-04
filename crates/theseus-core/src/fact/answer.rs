//! The answers' facts: an answer to a call that waited or to a budget
//! question, an act that did not count, and a session following the spend
//! limit (`rpc/confirms.rs`).

use serde_json::{json, Value};
use theseus_kernel::{Action, LimitFollowed, Micros};
use theseus_protocol::NarrativePart::{Approval, Session};
use theseus_protocol::{notify, ConfirmResolved, Event, LedgerKind};

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
    /// The answer trusts the session again (theseus-9bp).
    pub trust: bool,
}

impl Fact for CallAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionConfirmAnswered);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        json!({"correlation_id": self.action.correlation_id, "approved": self.approve, "note": self.note, "by": self.by, "via": self.via,
               "trust": self.trust})
    }

    fn event(&self) -> Option<Event> {
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: self.action.session_id.clone(),
            correlation_id: self.action.correlation_id.clone(),
            approved: self.approve,
            by: Some(self.by.into()),
            trust: Some(self.trust),
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
                self.by,
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
}

impl Fact for BudgetAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionConfirmAnswered);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        let q = self.question;
        json!({"correlation_id": q.correlation_id, "approved": self.approve, "note": self.note, "by": self.by, "via": self.via, "tool": q.tool})
    }

    fn event(&self) -> Option<Event> {
        let q = self.question;
        Some(Event::ConfirmResolved(ConfirmResolved {
            session_id: q.session_id.clone(),
            correlation_id: q.correlation_id.clone(),
            approved: self.approve,
            by: Some(self.by.into()),
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
}

impl Fact for SpendReset<'_> {
    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Approval,
            format!(
                "Spend reset to $0 by {} (it was {} of the {} limit); continuing.",
                self.by,
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
                self.by
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
    /// Whose it is: the answered call's session, or the trusted one's.
    pub fn session(&self) -> Option<&str> {
        match self.act {
            Act::Answer { action: a, .. } => Some(a.session_id.as_str()),
            Act::Tighten { .. } | Act::Untighten { .. } => None,
            Act::Trust { session } => Some(session),
            Act::Publish { .. } | Act::Ontology { .. } | Act::Label { .. } => None,
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
            Act::Trust { session } => {
                json!({"act": act.method(), "session_id": session, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::Publish { place } => {
                json!({"act": act.method(), "place": place, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
            Act::Ontology { what, .. } | Act::Label { what } => {
                json!({"act": act.method(), "what": what, "who": r.who, "via": r.via,
                       "why": r.why, "by": self.by})
            }
        }
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let r = self.refusal;
        say.line(
            Approval,
            match self.act {
                Act::Answer { action: a, .. } => format!(
                    "An answer to {} from {} through {} did not count: {}. It keeps waiting.",
                    a.tool, r.who, r.via, r.why
                ),
                Act::Tighten { tool } => format!(
                    "\"Should have asked\" for {tool} from {} through {} did not count: {}. It \
                     keeps its posture.",
                    r.who, r.via, r.why
                ),
                Act::Untighten { tool } => format!(
                    "An undo of {tool}'s tightening from {} through {} did not count: {}. It \
                     keeps asking first.",
                    r.who, r.via, r.why
                ),
                Act::Trust { .. } => format!(
                    "Trusting this session again, from {} through {}, did not count: {}. Its \
                     calls that act keep waiting.",
                    r.who, r.via, r.why
                ),
                Act::Publish { place } => format!(
                    "Publishing into {place}, from {} through {}, did not count: {}. Nothing \
                     was published.",
                    r.who, r.via, r.why
                ),
                Act::Ontology { what, .. } => format!(
                    "Writing the ontology's {what}, from {} through {}, did not count: {}. \
                     Nothing was written.",
                    r.who, r.via, r.why
                ),
                Act::Label { what } => format!(
                    "A memory label, {what}, from {} through {}, did not count: {}. Nothing \
                     was written.",
                    r.who, r.via, r.why
                ),
            },
        );
    }
}

/// An open session follows the config's spend limit (theseus-3pj).
pub struct LimitChanged<'a> {
    pub followed: &'a LimitFollowed,
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
                "Session {} follows the config's spend limit: {} before, {} now{then}.",
                narrative::short(&f.session_id),
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
        say.line(
            Approval,
            format!(
                "{} expired: nobody answered within {}, so it was not run; the driver resumes \
                 the turn.",
                self.action.tool,
                within(self.waited_ms)
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
