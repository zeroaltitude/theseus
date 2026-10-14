//! Self-exams' facts (theseus-pw1q.11): `self.exam.counted`,
//! `self.exam.rejected` and the night's `self.exam.pass`. Every `self.*` row
//! carries `what`, `why`, `numbers` and `undo` beside its own fields
//! (`theseus_protocol::rsi::SELF_ROWS`). A sealed item's row names its id,
//! its family and the reviewer, and nothing of its text.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Model;

use super::{Fact, Say};
use crate::rsi::exams::PassReport;

/// The undo every exam row has: none by the daemon.
const NO_UNDO: &str = "none: deleting or loosening an exam item is the owner's";

/// An item counted after its independent review.
pub struct ExamCounted<'a> {
    pub item: &'a str,
    pub family: &'a str,
    pub sealed: bool,
    pub reviewer: &'a str,
}

impl Fact for ExamCounted<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfExamCounted);

    fn row(&self) -> Value {
        json!({"item": self.item, "family": self.family, "sealed": self.sealed,
               "reviewer": self.reviewer,
               "what": format!("exam item {} counted in {} ({} half)", self.item, self.family,
                   if self.sealed { "sealed" } else { "open" }),
               "why": "a reviewer on another model called it fair", "numbers": null,
               "undo": NO_UNDO})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "Exam item {} counted in {} after {}'s review.",
                self.item, self.family, self.reviewer
            ),
        );
    }
}

/// A candidate the review did not call fair.
pub struct ExamRejected<'a> {
    pub item: &'a str,
    pub family: &'a str,
    pub sealed: bool,
    pub reviewer: &'a str,
    pub reason: &'a str,
}

impl Fact for ExamRejected<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfExamRejected);

    fn row(&self) -> Value {
        json!({"item": self.item, "family": self.family, "sealed": self.sealed,
               "reviewer": self.reviewer, "reason": self.reason,
               "what": format!("exam candidate {} dropped from {}", self.item, self.family),
               "why": self.reason, "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            format!(
                "Exam candidate {} for {} was dropped: {}.",
                self.item, self.family, self.reason
            ),
        );
    }
}

/// One night's pass, in numbers.
pub struct ExamPass {
    day: String,
    candidates: u64,
    counted: u64,
    rejected: u64,
    sealed_dropped: u64,
    unreviewed: u64,
    capped: u64,
    error: Option<String>,
}

impl ExamPass {
    pub fn of(r: &PassReport, set: &str) -> ExamPass {
        ExamPass {
            day: set.trim_start_matches("self-exams-").to_string(),
            candidates: r.candidates,
            counted: r.counted(),
            rejected: r.rejected(),
            sealed_dropped: r.sealed_dropped(),
            unreviewed: r.unreviewed(),
            capped: r.capped,
            error: None,
        }
    }

    pub fn failed(error: &str) -> ExamPass {
        ExamPass {
            day: String::new(),
            candidates: 0,
            counted: 0,
            rejected: 0,
            sealed_dropped: 0,
            unreviewed: 0,
            capped: 0,
            error: Some(error.chars().take(300).collect()),
        }
    }
}

impl Fact for ExamPass {
    const KIND: Option<LedgerKind> = Some(LedgerKind::SelfExamPass);

    fn row(&self) -> Value {
        let what = match &self.error {
            Some(_) => "the night's exam pass did not run".to_string(),
            None => format!(
                "exam pass: {} candidates, {} counted, {} rejected",
                self.candidates, self.counted, self.rejected
            ),
        };
        json!({"day": self.day, "candidates": self.candidates, "counted": self.counted,
               "rejected": self.rejected, "sealed_dropped": self.sealed_dropped,
               "unreviewed": self.unreviewed, "capped": self.capped, "error": self.error,
               "what": what, "why": self.error, "numbers": null, "undo": null})
    }

    fn narrate(&self, say: &mut Say<'_>) {
        say.line(
            Model,
            match &self.error {
                Some(e) => format!("The night's exam pass did not run: {e}."),
                None => format!(
                    "Exam pass: {} candidates, {} counted, {} rejected, {} sealed dropped, {} \
                     left for the cap.",
                    self.candidates, self.counted, self.rejected, self.sealed_dropped, self.capped
                ),
            },
        );
    }
}
