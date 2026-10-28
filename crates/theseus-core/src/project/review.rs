//! The weekly review (theseus-n5ui2; the owner's decision D-H12: Monday 08:00
//! unless the project says otherwise): a routine made with the project, whose
//! runs are fresh tasks. Its run log (`routine.runs`) is the project's
//! history, and the project's `review` field names its routine.

use theseus_protocol::routines::RoutineSpec;

use crate::config::ProjectsConfig;
use crate::routine::{self, Book, NewRoutine};

/// The routine's title.
pub fn title(project_title: &str) -> String {
    let t: String = project_title.chars().take(60).collect();
    format!("Review: {t}")
}

/// What each weekly run does. It names the project by id, because a run is a
/// fresh task that sees nothing of the conversation that made the project.
pub fn brief(project: &str, project_title: &str) -> String {
    format!(
        "You are the weekly review of the project {project} (\"{project_title}\"). Do these in \
         order, then report.\n\
         1. Read the project: run `theseus project show {project}`. Its goal and acceptance are \
         the owner's: never change them. Its milestones and their dates are the plan.\n\
         2. Read the week: what closed (`theseus tasks`, the project's steps and milestones that \
         changed state since the last run), the reports that came in, and your last run's report.\n\
         3. Say what slipped: each overdue or moved milestone, what it cost (the dollars a step \
         spent), and why, as far as the record shows.\n\
         4. Propose next week's steps: record each as a plan item with `task_create` (a `title`, \
         and `parent` {project}), so they appear in the project's slice. Propose only what the \
         record supports; do not start tasks.\n\
         5. Where a decision is the owner's (a date to move, a milestone to drop, a trade-off), \
         do not decide it: put it under a `Decisions for you` heading in your report with the \
         options and your recommendation. (This build has no owner_ask tool yet; the report is \
         where the question goes.)\n\
         Keep the report short: progress as `done/total`, what slipped, next week's steps, then \
         decisions. If nothing changed since the last run, say so in one line."
    )
}

/// The request for `project`'s review: its routine's words.
pub fn request(
    cfg: &ProjectsConfig,
    (project, project_title): (&str, &str),
    schedule: Option<&str>,
    usd: Option<f64>,
) -> NewRoutine {
    NewRoutine {
        title: title(project_title),
        schedule: schedule.unwrap_or(&cfg.review_schedule).to_string(),
        brief: brief(project, project_title),
        budget_usd: Some(usd.unwrap_or(cfg.review_run_usd)),
        month_usd: Some(cfg.review_month_usd.max(usd.unwrap_or(0.0))),
        deliver_to: None,
        catch_up: None,
        speak: None,
        start: None,
        stop: None,
        carry: None,
        grant: vec![],
    }
}

/// Make the review routine (the same functions the routine verbs call).
pub fn make(
    kernel: &theseus_kernel::Kernel,
    store: &crate::store::Store,
    book: &Book,
    new: &NewRoutine,
    (by, created_in): (&str, Option<&str>),
) -> Result<RoutineSpec, String> {
    let prepared = routine::prepare(kernel, &book.cfg, new, by, created_in)?;
    routine::create(kernel, store, book, prepared, by).map_err(|e| format!("{e:#}"))
}
