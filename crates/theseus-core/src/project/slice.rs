//! The project's slice in an attached conversation (theseus-n5ui2): the goal,
//! the open milestones with their dates, and the next steps, in the request's
//! tail beside the task view's block and the commitments' (`task_graph::view`,
//! `commitments`): the same last message, after the cached prefix, with no
//! breakpoint of its own. It is built from the records every loop, so no
//! compaction can take it, and within `MAX_TOKENS` of the view's 1,500.
//!
//! Built from memory: `render` takes the records and the clock and reads
//! nothing else. A conversation with no project attached adds nothing, so a
//! plain turn's request, its token count and its frames are unchanged.

use theseus_protocol::projects::ProjectSliceSummary;
use theseus_protocol::tasks::{TaskKind, TaskRecord};

use super::progress::{milestones, progress};
use crate::task_graph::{subtree, view::BYTES_PER_TOKEN};
use crate::wake::Local;

/// How the block begins: a reader of a request (a test's stand-in model) can
/// tell it from the conversation's (`task_graph::view::is_head`).
pub const HEAD: &str = "[The project this conversation serves";
/// The slice's bound, in tokens: a fifth of the task view's.
pub const MAX_TOKENS: u64 = 300;
/// Most milestones and steps it lists.
const MAX_MILESTONES: usize = 5;
const MAX_STEPS: usize = 4;
/// A line's text, in characters, at the longest.
const TITLE_CHARS: usize = 48;
const GOAL_CHARS: usize = 160;

/// The block and what it showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    pub text: String,
    pub summary: ProjectSliceSummary,
}

fn tokens(s: &str) -> u64 {
    (s.len() as u64).div_ceil(BYTES_PER_TOKEN)
}

fn clip(s: &str, n: usize) -> String {
    let first = s.lines().next().unwrap_or_default().trim();
    match first.char_indices().nth(n) {
        Some((at, _)) => format!("{}…", first[..at].trim_end()),
        None => first.to_string(),
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `Fri Oct 23`.
fn day(l: &Local) -> String {
    format!(
        "{} {} {}",
        l.weekday(),
        MONTHS[(l.month.clamp(1, 12) - 1) as usize],
        l.day
    )
}

/// One milestone's line: `⚑ Fri Oct 23 Harbour survey (a1b2c3)`, and
/// `⚑ OVERDUE …` when its date has passed.
fn milestone_line(t: &TaskRecord, now_ms: u64, at: &dyn Fn(u64) -> Local) -> String {
    let when = match t.due_at_ms {
        Some(d) if d < now_ms => format!("OVERDUE since {}", day(&at(d))),
        Some(d) => day(&at(d)),
        None => "undated".into(),
    };
    format!(
        "⚑ {when}: {} ({})",
        clip(&t.title, TITLE_CHARS),
        crate::task::short(&t.id)
    )
}

/// The slice of `project` at `now_ms`, in the zone `at` reads; None when no
/// such task is among `tasks`.
pub fn render(
    tasks: &[TaskRecord],
    project: &str,
    now_ms: u64,
    at: &dyn Fn(u64) -> Local,
) -> Option<Shown> {
    let p = tasks.iter().find(|t| t.id == project)?;
    let n = progress(tasks, &p.id, now_ms);
    let mut head = format!(
        "{HEAD}: {} \"{}\" [{}], {}/{} milestones, {}/{} steps",
        p.id,
        clip(&p.title, TITLE_CHARS),
        p.state.as_str(),
        n.milestones_done,
        n.milestones_total,
        n.steps_done,
        n.steps_total
    );
    if n.overdue > 0 {
        head.push_str(&format!(", {} overdue", n.overdue));
    }
    head.push_str(&format!(
        ". Its goal is the owner's; plan its steps as task_create items under {}.]",
        crate::task::short(&p.id)
    ));
    // Candidate lines, by priority: the goal, then the milestones, the
    // steps, and the acceptance.
    let mut cand: Vec<(u8, String)> = Vec::new();
    if !p.objective.trim().is_empty() && p.objective.trim() != p.title.trim() {
        cand.push((0, format!("Goal: {}", clip(&p.objective, GOAL_CHARS))));
    }
    let open_ms: Vec<&TaskRecord> = milestones(tasks, &p.id)
        .into_iter()
        .filter(|t| !t.state.is_closed())
        .collect();
    for t in open_ms.iter().take(MAX_MILESTONES) {
        cand.push((1, milestone_line(t, now_ms, at)));
    }
    let steps: Vec<&TaskRecord> = subtree(tasks, &p.id)
        .into_iter()
        .filter(|t| t.kind == TaskKind::Item && !t.state.is_closed() && t.id != p.id)
        .filter(|t| {
            t.deps.iter().all(|d| {
                tasks
                    .iter()
                    .find(|x| &x.id == d)
                    .is_none_or(|x| x.state.is_closed())
            })
        })
        .collect();
    for t in steps.iter().take(MAX_STEPS) {
        cand.push((
            2,
            format!(
                "→ {} ({}) [{}]",
                clip(&t.title, TITLE_CHARS),
                crate::task::short(&t.id),
                t.state.as_str()
            ),
        ));
    }
    if let Some(a) = p.acceptance.first() {
        let more = match p.acceptance.len() {
            1 => String::new(),
            k => format!(" (+{} more)", k - 1),
        };
        cand.push((3, format!("Done when: {}{more}", clip(a, GOAL_CHARS))));
    }
    let listed = open_ms.len().min(MAX_MILESTONES) + steps.len().min(MAX_STEPS);
    let total = open_ms.len() + steps.len();
    // Fill the budget in priority order; a line that does not fit is left
    // out, and the closing line says how many were.
    let more = |k: usize| format!("(and {k} more: task.list shows the project's tree.)");
    let mut used = tokens(&head);
    let mut keep: Vec<bool> = vec![false; cand.len()];
    let mut order: Vec<usize> = (0..cand.len()).collect();
    order.sort_by_key(|i| (cand[*i].0, *i));
    let mut left = total - listed;
    for i in order {
        let reserve = tokens(&more(left + 1)) + 1;
        if used + tokens(&cand[i].1) + 1 + reserve <= MAX_TOKENS {
            used += tokens(&cand[i].1) + 1;
            keep[i] = true;
        } else if cand[i].0 != 0 && cand[i].0 != 3 {
            left += 1;
        }
    }
    let mut text = head;
    let mut lines = 0u32;
    for (i, (_, l)) in cand.iter().enumerate() {
        if keep[i] {
            text.push('\n');
            text.push_str(l);
            lines += 1;
        }
    }
    if left > 0 {
        text.push('\n');
        text.push_str(&more(left));
    }
    use sha2::{Digest, Sha256};
    Some(Shown {
        summary: ProjectSliceSummary {
            project: p.id.clone(),
            digest: hex::encode(Sha256::digest(text.as_bytes())),
            lines,
            left_out: left as u32,
            tokens: tokens(&text),
        },
        text,
    })
}

/// The turn's side (`compile_step`): the session's project slice, attached
/// to the compiled request after the commitments' block (the last block),
/// its tokens counted in the estimate. None, and the request untouched, for
/// a conversation with no project, or one whose project is gone.
pub fn attach(
    store: &crate::store::Store,
    kernel: &theseus_kernel::Kernel,
    session: &crate::session::SessionRecord,
    compiled: &mut crate::compiler::Compiled,
) -> Option<ProjectSliceSummary> {
    let project = session.project.as_deref()?;
    let tasks = match crate::task_graph::all_shown(store, kernel) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "the project's slice was not read");
            return None;
        }
    };
    let shown = render(&tasks, project, kernel.now_ms(), &crate::wake::local)?;
    crate::task_graph::view::attach_text(&mut compiled.request, &shown.text);
    let t = shown.summary.tokens;
    compiled.est_tokens += t;
    compiled.estimate.tokens += t;
    compiled.estimate.estimated += t;
    compiled.estimate.upper += t + t * crate::compiler::MARGIN_PERCENT / 100;
    Some(shown.summary)
}
