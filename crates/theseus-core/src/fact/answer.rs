//! The answers' facts: an answer to a call that waited or to a budget
//! question, an act that did not count, and a session following the spend
//! limit (`rpc/confirms.rs`).

use serde_json::{json, Value};
use theseus_kernel::{Action, LimitFollowed, Micros};
use theseus_protocol::NarrativePart::{Approval, Session};
use theseus_protocol::{notify, ApprovalRefused, ConfirmResolved, Event, LedgerKind};

use super::{Fact, Say};
use crate::approval::Refusal;
use crate::narrative;
use crate::peer::Traced;
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
    /// The process that answered, as traced (theseus-6qy).
    pub asker: Value,
    /// The answer trusts the session again (theseus-9bp).
    pub trust: bool,
}

impl Fact for CallAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionConfirmAnswered);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        json!({"correlation_id": self.action.correlation_id, "approved": self.approve, "note": self.note, "by": self.by, "via": self.via,
               "asker": self.asker, "trust": self.trust})
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
    pub asker: Value,
}

impl Fact for BudgetAnswered<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionConfirmAnswered);
    const METHOD: Option<&'static str> = Some(notify::CONFIRM_RESOLVED);

    fn row(&self) -> Value {
        let q = self.question;
        json!({"correlation_id": q.correlation_id, "approved": self.approve, "note": self.note, "by": self.by, "via": self.via, "tool": q.tool,
               "asker": self.asker})
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
/// who, through what, why, and the process that asked. One from a Theseus
/// job's process is the security event `JobActRefused` says.
pub struct ActRefused<'a> {
    pub(crate) act: Act<'a>,
    pub refusal: &'a Refusal,
    /// The label of who answered.
    pub by: &'a str,
    pub traced: &'a Traced,
}

impl ActRefused<'_> {
    /// Whose it is: the answered call's session, or the trusted one's.
    pub fn session(&self) -> Option<&str> {
        match self.act {
            Act::Answer { action: a, .. } => Some(a.session_id.as_str()),
            Act::Tighten { .. } | Act::Untighten { .. } => None,
            Act::Trust { session } => Some(session),
        }
    }

    fn by_a_job(&self) -> bool {
        self.traced.refusal().is_some()
    }
}

impl Fact for ActRefused<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ApprovalRefused);

    fn row(&self) -> Value {
        let (r, act) = (self.refusal, self.act);
        let mut data = match act {
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
        };
        if *self.traced != Traced::NoProcess {
            data["asker"] = self.traced.json();
        }
        if self.by_a_job() {
            data["from_job"] = json!(true);
        }
        data
    }

    fn narrate(&self, say: &mut Say<'_>) {
        if self.by_a_job() {
            return;
        }
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
            },
        );
    }
}

/// A Theseus job's process, or one that cannot be traced, tried an
/// approval-like act (theseus-6qy): a security event, said in the narrative
/// and to every connection (`approval.refused`, with the row's fields).
pub struct JobActRefused<'a> {
    pub(crate) act: Act<'a>,
    pub refusal: &'a Refusal,
    pub params: &'a ApprovalRefused,
}

impl Fact for JobActRefused<'_> {
    const METHOD: Option<&'static str> = Some(notify::APPROVAL_REFUSED);

    fn event(&self) -> Option<Event> {
        Some(Event::ApprovalRefused(self.params.clone()))
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let what = match self.act {
            Act::Answer { action: a, .. } => format!("an answer to {}", a.tool),
            Act::Untighten { tool } => format!("the undo of {tool}'s tightening"),
            Act::Tighten { tool } => format!("\"should have asked\" for {tool}"),
            Act::Trust { session } => {
                format!("trusting session {} again", narrative::short(session))
            }
        };
        let then = match self.act {
            Act::Answer { .. } => "It keeps waiting for the operator's answer.",
            Act::Untighten { .. } => "It keeps asking first.",
            Act::Tighten { .. } => "Nothing changed.",
            Act::Trust { .. } => "It still holds external text, and its calls that act wait.",
        };
        say.line(
            Approval,
            format!(
                "Refused {what} {} through {}: a job's process cannot answer an approval. {then}",
                self.refusal.why, self.refusal.via
            ),
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
