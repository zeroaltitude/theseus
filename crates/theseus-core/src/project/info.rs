//! A project as a list shows it (theseus-n5ui2): the record, its progress,
//! and its review's next run. Pure but for the review's lookup, which the
//! caller passes.

use theseus_protocol::projects::ProjectInfo;
use theseus_protocol::tasks::TaskRecord;

use super::progress::{milestones, progress};

/// `p`'s info at `now_ms`. `review_next` gives the next run of a review
/// routine (by its id), when it has one.
pub fn info(
    tasks: &[TaskRecord],
    p: &TaskRecord,
    now_ms: u64,
    review_next: &dyn Fn(&str) -> Option<u64>,
) -> ProjectInfo {
    let next = milestones(tasks, &p.id)
        .into_iter()
        .find(|m| !m.state.is_closed());
    ProjectInfo {
        id: p.id.clone(),
        title: p.title.clone(),
        objective: p.objective.clone(),
        acceptance: p.acceptance.clone(),
        state: p.state.as_str().to_string(),
        progress: progress(tasks, &p.id, now_ms),
        next_title: next.map(|m| m.title.clone()),
        review: p.review.clone(),
        review_next_ms: p.review.as_deref().and_then(review_next),
    }
}
