//! A China-origin draft's measure (theseus-a1wvq.13; `draft.rs`): one
//! `task.draft` row when its review ends, or with the post that says no
//! reviewer could run. The config-guidance row's bench report reads these
//! keys by name, so they stay as they are.

use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_protocol::NarrativePart::Session;

use super::{Fact, Say};

/// A draft and its review, settled (`task.draft`).
pub struct TaskDraft<'a> {
    pub draft_task: &'a str,
    pub draft_profile: &'a str,
    pub draft_model: &'a str,
    pub draft_cost_usd: f64,
    /// `complete`, `failed`, `cancelled`, `stopped` or `not_admitted`.
    pub draft_outcome: &'a str,
    /// The review's task, profile, model and cost: none when no review ran.
    pub review: Option<Reviewed<'a>>,
    /// Added plus deleted lines from the draft commit to the final one, by
    /// `git diff --numstat`.
    pub lines_changed: u64,
    /// The reviewer's own count, from its last line: reported, not measured.
    pub tests_fixed_reported: Option<u64>,
    pub applied: bool,
    pub why: &'a str,
}

/// The review half of a `task.draft` row.
pub struct Reviewed<'a> {
    pub task: &'a str,
    pub profile: &'a str,
    pub model: &'a str,
    pub cost_usd: f64,
}

impl Fact for TaskDraft<'_> {
    const KIND: Option<LedgerKind> = Some(LedgerKind::TaskDraft);

    fn row(&self) -> Value {
        let r = self.review.as_ref();
        let mut row = json!({
            "draft_task": self.draft_task,
            "draft_profile": self.draft_profile,
            "draft_model": self.draft_model,
            "draft_cost_usd": self.draft_cost_usd,
            "draft_outcome": self.draft_outcome,
            "review_task": r.map(|r| r.task),
            "review_profile": r.map(|r| r.profile),
            "review_model": r.map(|r| r.model),
            "review_cost_usd": r.map(|r| r.cost_usd),
            "lines_changed": self.lines_changed,
            "applied": self.applied,
            "why": self.why,
        });
        if let Some(n) = self.tests_fixed_reported {
            row["tests_fixed_reported"] = json!(n);
        }
        row
    }

    fn narrate(&self, say: &mut Say<'_>) {
        let reviewed = self
            .review
            .as_ref()
            .map_or("not reviewed".to_string(), |r| {
                format!("reviewed by {} (${:.4})", r.profile, r.cost_usd)
            });
        let applied = if self.applied {
            "applied"
        } else {
            "not applied"
        };
        say.line(
            Session,
            format!(
                "Draft {} by {} ({}, ${:.4}) {reviewed}: {} lines changed in review, {applied} ({}).",
                crate::task::short(self.draft_task),
                self.draft_profile,
                self.draft_outcome,
                self.draft_cost_usd,
                self.lines_changed,
                self.why
            ),
        );
    }
}
