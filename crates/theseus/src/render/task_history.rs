//! Finding a task later (theseus-ypnxt.1): `theseus tasks --ended`, a search,
//! and `theseus task show`. Pure: the caller passes the `task.list` answer and
//! the time, so a test draws what the binary does.
//!
//! The list is one line a task, newest first: a mark, the short id, the title,
//! how it ended and when, what it spent, and the first line of its report, cut
//! to the width. `show` is one task whole, with the commands that act on it
//! now. The money is `usd`, the times are `task_tree`'s `coarse`, and the id is
//! `task_tree::short_id`; nothing here words a progress line.

use anyhow::{anyhow, bail, Result};
use theseus_protocol::tasks::TaskRecord;
use theseus_protocol::term::words::clip;
use theseus_protocol::LedgerEntry;
use theseus_protocol::{usd, TaskDetail, TaskInfo, TaskListResult};

use super::task_tree::{coarse, short_id};
use super::time::{fmt_date, fmt_day, local_day_at};

/// The widest a title is drawn in the list, before the report gets what is left.
const TITLE_MAX: usize = 28;
/// The narrowest report column worth drawing.
const REPORT_MIN: usize = 16;

/// How a task stands, in the words of this list: `done`, `failed`, `stopped`
/// for an end, else what it is doing.
fn how(t: &TaskInfo) -> &str {
    match t.state.as_str() {
        "complete" => "done",
        "failed" | "budget_exhausted" => "failed",
        "cancelled" => "stopped",
        "queued" | "running" => "running",
        other => other,
    }
}

fn ended(t: &TaskInfo) -> bool {
    matches!(how(t), "done" | "failed" | "stopped")
}

fn mark(t: &TaskInfo) -> char {
    match how(t) {
        "done" => '✓',
        "failed" => '✗',
        "running" => '◐',
        _ => '○',
    }
}

/// When a task ended (or was last updated), as `5m ago` today and a date
/// after a day.
fn when(t: &TaskInfo, now_ms: u64) -> String {
    let age = now_ms.saturating_sub(t.updated_at_ms);
    if age < 86_400_000 {
        format!("{} ago", coarse(age))
    } else {
        fmt_day(t.updated_at_ms)
    }
}

fn detail<'a>(r: &'a TaskListResult, t: &TaskInfo) -> Option<&'a TaskDetail> {
    r.details.iter().find(|d| d.task_id == t.task_id)
}

fn title(t: &TaskInfo) -> &str {
    t.title
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("(no title)")
}

/// The list: one line a task, in the order the daemon gave (newest first), or
/// a sentence that says why there is none.
pub fn lines(r: &TaskListResult, now_ms: u64, width: usize, what: &str) -> Vec<String> {
    if r.tasks.is_empty() {
        return vec![format!("no {what}")];
    }
    let tw = r
        .tasks
        .iter()
        .map(|t| title(t).chars().count())
        .max()
        .unwrap_or(0)
        .min(TITLE_MAX);
    let ends: Vec<String> = r
        .tasks
        .iter()
        .map(|t| format!("{} {}", how(t), when(t, now_ms)))
        .collect();
    let money: Vec<String> = r.tasks.iter().map(|t| usd(t.spent_usd)).collect();
    let ew = ends.iter().map(|e| e.chars().count()).max().unwrap_or(0);
    let mw = money.iter().map(|m| m.chars().count()).max().unwrap_or(0);
    r.tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let head = format!(
                "{} {}  {:tw$}  {:ew$}  {:>mw$}",
                mark(t),
                short_id(&t.task_id),
                clip(title(t), tw),
                ends[i],
                money[i],
            );
            let room = width.saturating_sub(head.chars().count() + 2);
            match detail(r, t).and_then(|d| d.report_line.as_deref()) {
                Some(l) if room >= REPORT_MIN => format!("{head}  {}", clip(l, room)),
                _ => head,
            }
        })
        .collect()
}

/// The bare `tasks` line when nothing is open: what ended today, and how to
/// see it.
pub fn nothing_open(ended_today: usize) -> String {
    match ended_today {
        0 => "no tasks".to_string(),
        n => format!("no open tasks · {n} ended today: theseus tasks --ended"),
    }
}

/// `--since`'s value: an age (`30m`, `12h`, `7d`, `2w`) back from `now_ms`, or
/// a calendar day (`2026-10-15`) at this machine's midnight.
pub fn since_ms(s: &str, now_ms: u64) -> Result<u64> {
    let t = s.trim();
    let bad =
        || anyhow!("`{s}` is no time: write an age (30m, 12h, 7d, 2w) or a date (2026-10-15)");
    if let Ok(day) = t.parse::<jiff::civil::Date>() {
        let today: jiff::civil::Date = fmt_day(now_ms).parse().map_err(|_| bad())?;
        let back = i64::from((today - day).get_days());
        if back < 0 {
            bail!("`{s}` is in the future");
        }
        return local_day_at(now_ms, back, 0, 0).ok_or_else(bad);
    }
    let (n, unit) = t.split_at(t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len()));
    let n: u64 = n.parse().map_err(|_| bad())?;
    let unit = match unit {
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 7 * 86_400_000,
        _ => return Err(bad()),
    };
    Ok(now_ms.saturating_sub(n * unit))
}

/// The commands that act on a task now.
fn commands(t: &TaskInfo, held: bool) -> Vec<String> {
    let id = short_id(&t.task_id);
    let mut out = Vec::new();
    if t.pending_confirms > 0 {
        out.push(
            "theseus answer ID …  (it asks: `theseus tasks` shows the question's ID)".to_string(),
        );
    }
    out.push(format!("theseus chat --task {id}"));
    if !ended(t) {
        out.push(format!(
            "theseus task {} {id}",
            if held { "resume" } else { "pause" }
        ));
        out.push(format!("theseus cancel {id}"));
    }
    out
}

fn indented(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines().map(|l| format!("  {l}"))
}

/// A task's model, from its session's `route.decided` rows (theseus-ypnxt.9): where
/// its brief put it (`haikuhi (task brief)`), then each move a checkpoint made, newest
/// last (`sonnet (from haikuhi, loop 7: failing)`). Only a move that switched counts;
/// a checkpoint that left it where it was is no line. A step back down says `step down`.
pub fn model_lines(rows: &[LedgerEntry]) -> Vec<String> {
    let mut rows: Vec<&LedgerEntry> = rows.iter().collect();
    rows.sort_by_key(|r| r.position);
    rows.into_iter()
        .filter_map(|r| {
            let d = &r.data;
            let str = |k: &str| d[k].as_str();
            let profile = str("profile")?;
            match (str("source"), d["switch"].as_bool()) {
                (Some("task brief"), _) => Some(format!("{profile} (task brief)")),
                (Some("task checkpoint"), Some(true)) => {
                    let why = match str("reason") {
                        Some("step_down") => "step down",
                        _ => str("trigger").unwrap_or("checkpoint"),
                    };
                    let at = d["loop"]
                        .as_u64()
                        .map_or(String::new(), |n| format!(", loop {}", n + 1));
                    Some(format!("{profile} (from {}{at}: {why})", str("from")?))
                }
                _ => None,
            }
        })
        .collect()
}

/// One task whole: its state, where it reports, its brief, plan and
/// children, its money and times, its report, and what to type next.
pub fn show(r: &TaskListResult) -> Result<Vec<String>> {
    show_with(r, &[])
}

/// `show`, with the task's model lines (`model_lines`) after where it reports.
pub fn show_with(r: &TaskListResult, models: &[String]) -> Result<Vec<String>> {
    let (Some(t), Some(rec)) = (r.tasks.first(), r.records.first()) else {
        bail!("that is a step with no task: it has no run to show");
    };
    let d = detail(r, t);
    let held = d.is_some_and(|d| d.paused);
    let state = if held {
        "paused".to_string()
    } else {
        how(t).to_string()
    };
    let mut out = vec![format!("{} {}  ·  {state}", mark(t), title(t))];
    out.push(format!("  id        {} ({})", short_id(&t.task_id), rec.id));
    if let Some(to) = t.target.as_deref() {
        out.push(format!("  reports   {to}"));
    }
    for (n, m) in models.iter().enumerate() {
        out.push(format!(
            "  {}{m}",
            if n == 0 { "model     " } else { "          " }
        ));
    }
    out.push(format!(
        "  spent     {} of {}",
        usd(t.spent_usd),
        usd(t.limit_usd)
    ));
    out.push(format!("  started   {}", fmt_date(t.created_at_ms)));
    if ended(t) {
        let took = t.updated_at_ms.saturating_sub(t.created_at_ms);
        out.push(format!(
            "  ended     {} ({} later)",
            fmt_date(t.updated_at_ms),
            coarse(took)
        ));
    } else if let Some(w) = t.waiting_on.as_deref() {
        out.push(format!("  waiting   {w}"));
    }
    if let Some(why) = t.ended_reason.as_deref().filter(|_| ended(t)) {
        out.push(format!("  because   {why}"));
    }
    section(&mut out, "Brief", &rec.objective);
    let (steps, kids): (Vec<&TaskRecord>, Vec<&TaskRecord>) =
        r.records.iter().skip(1).partition(|c| c.session.is_none());
    if !steps.is_empty() {
        out.push("Plan".to_string());
        out.extend(
            steps
                .iter()
                .map(|s| format!("  {} {}", step_mark(s), s.title)),
        );
    }
    if !kids.is_empty() {
        out.push("Children".to_string());
        out.extend(
            kids.iter()
                .map(|k| format!("  {} {}  {}", step_mark(k), k.short(), k.title)),
        );
    }
    if let Some(report) = d.and_then(|d| d.report.as_deref()) {
        section(&mut out, "Report", report);
    }
    out.push("Commands".to_string());
    out.extend(commands(t, held).into_iter().map(|c| format!("  {c}")));
    Ok(out)
}

fn step_mark(t: &TaskRecord) -> char {
    use theseus_protocol::tasks::TaskState::*;
    match t.state {
        Done => '✓',
        Failed => '✗',
        InProgress => '◐',
        Abandoned => '○',
        _ => '·',
    }
}

fn section(out: &mut Vec<String>, name: &str, text: &str) {
    if !text.trim().is_empty() {
        out.push(name.to_string());
        out.extend(indented(text.trim_end()));
    }
}

#[cfg(test)]
#[path = "task_history_tests.rs"]
mod tests;
