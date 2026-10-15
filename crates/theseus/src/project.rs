//! `theseus projects` and `theseus project …` (theseus-n5ui2): long-term work
//! on the task graph. `projects` lists them (a bar of milestones done, the next
//! milestone and its date, the weekly review's next run); `project show ID`
//! prints the tree with dates; `project new`, `attach` and `detach` are the
//! owner's acts (refused inside a job, `client::OPERATORS`). The review's run
//! log is `theseus routine runs <review>`.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::method;
use theseus_protocol::projects::{
    MilestoneSpec, ProjectAttachParams, ProjectAttachResult, ProjectCreateParams,
    ProjectCreateResult, ProjectInfo, ProjectListParams, ProjectListResult, ProjectShowParams,
    ProjectShowResult,
};
use theseus_protocol::tasks::{TaskKind, TaskRecord};

use crate::cmd::output;
use theseus_client::render::time::{fmt_date, fmt_starts};

const BAR: usize = 7;

#[derive(Subcommand, Debug, Default)]
pub enum ProjectCmd {
    /// The open projects (the same as `theseus projects`).
    #[default]
    List,
    /// A project and everything under it, with dates.
    Show { id: String },
    /// Make a project: its goal (yours: an agent's edit of it waits for your yes), dated milestones,
    /// and a weekly review routine (Monday 08:00 unless `--review` says otherwise).
    New {
        title: String,
        /// What the project is for.
        #[arg(long)]
        objective: Option<String>,
        /// How to know it is done; repeat for more.
        #[arg(long = "accept")]
        accept: Vec<String>,
        /// A milestone, `TITLE@WHEN` (`Survey the moorings@2026-11-03`, `Chart@fri 09:00`); repeat.
        #[arg(long = "milestone")]
        milestone: Vec<String>,
        /// The weekly review's schedule (`mon 08:00`, the default), or `none`.
        #[arg(long)]
        review: Option<String>,
        /// The most one review run may spend, in dollars.
        #[arg(long)]
        review_budget: Option<f64>,
    },
    /// Attach a session to a project: every request it makes ends with the project's slice.
    Attach { session: String, id: String },
    /// Detach a session from its project.
    Detach { session: String },
}

/// `Title@when` into a milestone; the last `@` splits, so a title may hold one.
pub fn milestone_of(s: &str) -> MilestoneSpec {
    match s.rsplit_once('@') {
        Some((t, w)) if !w.trim().is_empty() => MilestoneSpec {
            title: t.trim().to_string(),
            due: Some(w.trim().to_string()),
        },
        _ => MilestoneSpec {
            title: s.trim().to_string(),
            due: None,
        },
    }
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ProjectCmd) -> Result<()> {
    match cmd {
        ProjectCmd::List => list(conn, json, false).await,
        ProjectCmd::Show { id } => {
            let v = conn
                .request(method::PROJECT_SHOW, ProjectShowParams { id })
                .await?;
            output(json, v, |r: ProjectShowResult| {
                for l in show_lines(&r, theseus_protocol::now_unix_ms()) {
                    println!("{l}");
                }
                Ok(())
            })
        }
        ProjectCmd::New {
            title,
            objective,
            accept,
            milestone,
            review,
            review_budget,
        } => {
            let p = ProjectCreateParams {
                title,
                objective,
                acceptance: accept,
                milestones: milestone.iter().map(|m| milestone_of(m)).collect(),
                review,
                review_usd: review_budget,
                ..Default::default()
            };
            let v = conn.request(method::PROJECT_CREATE, p).await?;
            output(json, v, |r: ProjectCreateResult| {
                let now = theseus_protocol::now_unix_ms();
                println!(
                    "project {} made: {}",
                    short(&r.project.id),
                    list_lines(std::slice::from_ref(&r.project), now)[0]
                );
                if let Some(rv) = &r.project.review {
                    println!(
                        "  weekly review: routine {} (`theseus routine runs {}` is its history)",
                        short(rv),
                        short(rv)
                    );
                }
                Ok(())
            })
        }
        ProjectCmd::Attach { session, id } => attach(conn, json, session, Some(id)).await,
        ProjectCmd::Detach { session } => attach(conn, json, session, None).await,
    }
}

/// `theseus projects`.
pub async fn list(conn: &mut Conn, json: bool, all: bool) -> Result<()> {
    let v = conn
        .request(method::PROJECT_LIST, ProjectListParams { all })
        .await?;
    output(json, v, |r: ProjectListResult| {
        for l in list_lines(&r.projects, theseus_protocol::now_unix_ms()) {
            println!("{l}");
        }
        Ok(())
    })
}

async fn attach(conn: &mut Conn, json: bool, session: String, id: Option<String>) -> Result<()> {
    let session = theseus_client::resolve::session(conn, &session).await?;
    let v = conn
        .request(
            method::PROJECT_ATTACH,
            ProjectAttachParams {
                session,
                project: id,
            },
        )
        .await?;
    output(json, v, |r: ProjectAttachResult| {
        match &r.project {
            Some(p) => println!(
                "session {} is attached to {}: its requests carry the project's slice",
                short(&r.session),
                short(p)
            ),
            None => println!("session {} has no project", short(&r.session)),
        }
        Ok(())
    })
}

fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(6)..]
}

/// `▰▰▰▱▱▱▱ 3/7  a1b2c3 Harbour survey · next Chart the moorings Fri 09:00 · 1 overdue · review Mon 08:00`
pub fn list_lines(projects: &[ProjectInfo], now_ms: u64) -> Vec<String> {
    if projects.is_empty() {
        return vec!["no projects: `theseus project new TITLE --milestone TITLE@WHEN`".into()];
    }
    projects
        .iter()
        .map(|p| {
            let mut s = format!("{}  {} {}", p.progress.bar(BAR), short(&p.id), p.title);
            if p.state != "accepted" && p.state != "in_progress" {
                s.push_str(&format!(" [{}]", p.state));
            }
            if let (Some(t), Some(d)) = (&p.next_title, p.progress.next_due_ms) {
                s.push_str(&format!(" · next {t} {}", fmt_starts(d, now_ms)));
            } else if let Some(t) = &p.next_title {
                s.push_str(&format!(" · next {t}"));
            }
            if p.progress.overdue > 0 {
                s.push_str(&format!(" · {} overdue", p.progress.overdue));
            }
            if p.progress.steps_total > 0 {
                s.push_str(&format!(
                    " · steps {}/{}",
                    p.progress.steps_done, p.progress.steps_total
                ));
            }
            match p.review_next_ms {
                Some(at) => s.push_str(&format!(" · review {}", fmt_starts(at, now_ms))),
                None if p.review.is_some() => s.push_str(" · review paused"),
                None => s.push_str(" · no review"),
            }
            s
        })
        .collect()
}

/// The project's line, its goal, then the tree with dates.
pub fn show_lines(r: &ProjectShowResult, now_ms: u64) -> Vec<String> {
    let mut out = list_lines(std::slice::from_ref(&r.project), now_ms);
    if !r.project.objective.is_empty() {
        out.push(format!("  goal: {}", r.project.objective));
    }
    for a in &r.project.acceptance {
        out.push(format!("  done when: {a}"));
    }
    let by_parent = |id: &str| -> Vec<&TaskRecord> {
        let mut v: Vec<&TaskRecord> = r
            .records
            .iter()
            .filter(|t| t.parent.as_deref() == Some(id))
            .collect();
        v.sort_by_key(|t| (t.due_at_ms.unwrap_or(u64::MAX), t.created_at_ms));
        v
    };
    let mut stack: Vec<(&TaskRecord, usize)> = by_parent(&r.project.id)
        .into_iter()
        .rev()
        .map(|t| (t, 1))
        .collect();
    while let Some((t, depth)) = stack.pop() {
        let mark = match t.kind {
            TaskKind::Milestone => "⚑",
            _ => "·",
        };
        let state = match (t.state.is_closed(), t.due_at_ms) {
            (false, Some(d)) if d < now_ms => format!("{} OVERDUE", t.state.as_str()),
            _ => t.state.as_str().to_string(),
        };
        let due = t
            .due_at_ms
            .map_or(String::new(), |d| format!(" · due {}", fmt_date(d)));
        out.push(format!(
            "{}{mark} {} \"{}\" [{state}]{due}",
            "  ".repeat(depth),
            short(&t.id),
            t.title
        ));
        for c in by_parent(&t.id).into_iter().rev() {
            stack.push((c, depth + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::projects::ProjectProgress;
    use theseus_protocol::tasks::TaskState;

    fn info() -> ProjectInfo {
        ProjectInfo {
            id: "tsk_00harbour".into(),
            title: "Harbour survey".into(),
            objective: "Chart every mooring".into(),
            acceptance: vec!["every mooring has a depth".into()],
            state: "accepted".into(),
            progress: ProjectProgress {
                milestones_done: 3,
                milestones_total: 7,
                steps_done: 1,
                steps_total: 4,
                next_due_ms: Some(1_790_000_000_000),
                overdue: 1,
            },
            next_title: Some("Chart the moorings".into()),
            review: Some("ses_review".into()),
            review_next_ms: Some(1_790_100_000_000),
        }
    }

    /// The list line: the bar, the short id, the next milestone, what is late,
    /// and the review's time.
    #[test]
    fn a_list_line_carries_the_bar_the_next_milestone_and_the_review() {
        theseus_client::render::time::pin_for_tests();
        let l = list_lines(&[info()], 1_789_999_000_000);
        assert!(
            l[0].starts_with("▰▰▰▱▱▱▱ 3/7  harbour Harbour survey"),
            "{}",
            l[0]
        );
        assert!(l[0].contains("next Chart the moorings"), "{}", l[0]);
        assert!(l[0].contains("1 overdue"), "{}", l[0]);
        assert!(l[0].contains("steps 1/4"), "{}", l[0]);
        assert!(l[0].contains("review "), "{}", l[0]);
    }

    /// A milestone is `Title@when`; the last `@` splits.
    #[test]
    fn a_milestone_reads_its_title_and_date() {
        let m = milestone_of("Survey@Harbour@fri 09:00");
        assert_eq!(
            (m.title.as_str(), m.due.as_deref()),
            ("Survey@Harbour", Some("fri 09:00"))
        );
        assert_eq!(milestone_of("No date").due, None);
    }

    /// The tree: milestones by date, overdue marked.
    #[test]
    fn show_prints_the_tree_with_dates() {
        theseus_client::render::time::pin_for_tests();
        let mut m = TaskRecord {
            id: "tsk_00m1".into(),
            title: "Chart the moorings".into(),
            parent: Some("tsk_00harbour".into()),
            kind: TaskKind::Milestone,
            due_at_ms: Some(1_789_000_000_000),
            state: TaskState::Accepted,
            ..Default::default()
        };
        let r = ProjectShowResult {
            project: info(),
            records: vec![TaskRecord::default(), m.clone()],
        };
        let lines = show_lines(&r, 1_790_000_000_000);
        assert!(lines
            .iter()
            .any(|l| l.contains("goal: Chart every mooring")));
        let ms = lines.iter().find(|l| l.contains("⚑")).unwrap();
        assert!(ms.contains("OVERDUE") && ms.contains("due 2026"), "{ms}");
        m.state = TaskState::Done;
        let r = ProjectShowResult {
            project: info(),
            records: vec![m],
        };
        assert!(!show_lines(&r, 1_790_000_000_000)
            .iter()
            .any(|l| l.contains("OVERDUE")));
    }
}
