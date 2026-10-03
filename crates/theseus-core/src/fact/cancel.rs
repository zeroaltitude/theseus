//! A cancel's verdicts (M4 18a; design §2.3, §2.11): how each call a cancel
//! or a stop reached is known to have stopped (`crate::cancel`).

use serde_json::{json, Value};
use theseus_kernel::{Action, Verdict, VerifiedBy};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Job;

use super::{Fact, Say};
use crate::narrative;

/// A call a stop verified gone (`action.cancel_verified`), with how and the
/// counts.
pub struct CancelVerified<'a> {
    pub action: &'a Action,
    pub verdict: &'a Verdict,
}

/// The row both a verdict's facts share.
fn row(a: &Action, v: &Verdict) -> Value {
    json!({"correlation_id": a.correlation_id, "execution_id": a.execution_id, "tool": a.tool,
        "verified_by": v.verified_by, "killed": v.killed, "survivors": v.survivors,
        "scope": v.scope, "ms": v.ms, "why": v.why})
}

/// "job a1b2c3 (proc.run)", or "call a1b2c3 (http.fetch)" for one with no
/// processes.
fn named(a: &Action, v: &Verdict) -> String {
    let what = if v.killed.is_some() { "job" } else { "call" };
    format!(
        "{what} {} ({})",
        narrative::short(&a.correlation_id),
        a.tool
    )
}

impl Fact for CancelVerified<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionCancelVerified);

    fn row(&self) -> Value {
        row(self.action, self.verdict)
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let v = self.verdict;
        let means = match v.verified_by {
            VerifiedBy::Pidns => "pid namespace",
            VerifiedBy::Cgroup => "cgroup",
            VerifiedBy::Tree => "process tree",
            VerifiedBy::Group => "process group",
            VerifiedBy::Task | VerifiedBy::None => "task",
        };
        let gone = match v.killed {
            Some(n) => format!(
                "its {means} ({}) is gone",
                narrative::count(u64::from(n), "process", "processes")
            ),
            None => format!("its {means} was aborted, and has ended"),
        };
        say.line(
            Job,
            format!(
                "Stopped {}: {gone}, {} after the stop.",
                named(self.action, v),
                narrative::duration(v.ms)
            ),
        );
    }
}

/// A call the cancel could not reach (`action.cancel_unsupported`): it runs
/// to its end, and its real outcome is recorded then.
pub struct CancelUnsupported<'a> {
    pub action: &'a Action,
    pub verdict: &'a Verdict,
}

impl Fact for CancelUnsupported<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionCancelUnsupported);

    fn row(&self) -> Value {
        row(self.action, self.verdict)
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "The {} cannot be stopped once started: {}; its real outcome is recorded when it \
                 ends.",
                named(self.action, self.verdict),
                self.verdict.why.as_deref().unwrap_or("nothing reaches it")
            ),
        );
    }
}

/// A call told to stop whose end was not verified
/// (`action.cancel_uncertain`), with why.
pub struct CancelUncertain<'a> {
    pub action: &'a Action,
    pub verdict: &'a Verdict,
}

impl Fact for CancelUncertain<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::ActionCancelUncertain);

    fn row(&self) -> Value {
        row(self.action, self.verdict)
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Job,
            format!(
                "The {} was told to stop, and its end was not verified: {}. It may still run.",
                named(self.action, self.verdict),
                self.verdict.why.as_deref().unwrap_or("no verdict came")
            ),
        );
    }
}
