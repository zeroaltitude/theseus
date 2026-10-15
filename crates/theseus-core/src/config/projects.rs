//! `[projects]` (theseus-n5ui2): long-term work on the task graph. Every key
//! has a default, so an owner's config names none unless it differs.
//!
//! - `review_schedule`: when a project's weekly review runs when its creator
//!   names none, in a routine's schedule words (`mon 08:00`: Monday morning).
//! - `review_run_usd` / `review_month_usd`: a review run's budget, and the
//!   review routine's month.
//! - `overdue_after_minutes`: how long after a milestone's date, still open,
//!   it becomes an overdue notice (one Interrupt, no sound): a day.
//! - `max_milestones`: the most milestones one `task_create` makes.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectsConfig {
    #[serde(default = "default_review_schedule")]
    pub review_schedule: String,
    #[serde(default = "default_review_run_usd")]
    pub review_run_usd: f64,
    #[serde(default = "default_review_month_usd")]
    pub review_month_usd: f64,
    #[serde(default = "default_overdue_after_minutes")]
    pub overdue_after_minutes: u64,
    #[serde(default = "default_max_milestones")]
    pub max_milestones: usize,
}

fn default_review_schedule() -> String {
    theseus_protocol::projects::DEFAULT_REVIEW.into()
}
fn default_review_run_usd() -> f64 {
    0.75
}
fn default_review_month_usd() -> f64 {
    6.0
}
fn default_overdue_after_minutes() -> u64 {
    1440
}
fn default_max_milestones() -> usize {
    32
}

impl Default for ProjectsConfig {
    fn default() -> Self {
        Self {
            review_schedule: default_review_schedule(),
            review_run_usd: default_review_run_usd(),
            review_month_usd: default_review_month_usd(),
            overdue_after_minutes: default_overdue_after_minutes(),
            max_milestones: default_max_milestones(),
        }
    }
}

impl ProjectsConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// How long after a milestone's date it is overdue, in ms.
    pub fn overdue_after_ms(&self) -> u64 {
        self.overdue_after_minutes.saturating_mul(60_000)
    }

    pub fn validate(&self) -> Result<()> {
        if !(crate::routine::usd_ok(self.review_run_usd)
            && crate::routine::usd_ok(self.review_month_usd))
            || self.review_run_usd > self.review_month_usd
        {
            bail!(
                "projects.review_run_usd ({}) must be above 0 and no more than review_month_usd ({})",
                self.review_run_usd,
                self.review_month_usd
            );
        }
        if self.review_schedule.trim().is_empty() {
            bail!("projects.review_schedule is empty");
        }
        if self.max_milestones == 0 {
            bail!("projects.max_milestones is 0");
        }
        Ok(())
    }
}
