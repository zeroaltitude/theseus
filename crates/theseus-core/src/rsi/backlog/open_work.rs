//! `self.open_work` (issue 9): what is open on Theseus's own
//! development, read from the repository's issues and nowhere else.
//!
//! Asked what was open, the installed build went to an outside tracker by
//! shell, found nothing, and answered from a stale row. Its development is
//! tracked in this repository's issues, which the self backlog already reads
//! (`super::issues`); the chat had no tool that showed them. This is that
//! tool, and it holds three things:
//!
//! - **Read-only, from the file source only.** It reads what `[self_issues]
//!   file` names, through the same parse the night uses, and builds no new
//!   outside call and needs no token. The file's own age is said plainly, so
//!   a stale list is never passed off as today's.
//! - **Registered whenever `[self_issues] enabled` is true**, whatever
//!   `[self] mode` is: the question is about reading, not acting. It is no
//!   [`crate::places::public_tool`], so a shared place is never offered it.
//! - **Its own description is the instruction not to use another tracker.**
//!   Nothing in the soul or the standing guidance changes.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_protocol::Plan;
use theseus_tools::{
    AsyncMediaRun, Backend, Retry, Tool, ToolClass, ToolCtx, ToolFailure, ToolOutput,
};

use super::issues::{self, Issue};
use crate::config::self_issues::SelfIssuesConfig;

pub const OPEN_WORK: &str = "self.open_work";

/// A title, cut at this many characters on the line it is listed on.
const TITLE_CHARS: usize = 120;

/// Register `self.open_work`, only while `[self_issues] enabled` is true.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
    if cfg.self_issues.enabled {
        r.register(Arc::new(OpenWork {
            cfg: cfg.self_issues.clone(),
        }));
    }
}

pub struct OpenWork {
    cfg: SelfIssuesConfig,
}

impl Tool for OpenWork {
    fn name(&self) -> &str {
        OPEN_WORK
    }

    fn description(&self) -> &str {
        "List what is open on Theseus's own development: the open issues of Theseus's repository, \
         each with its number, title and link, and how old the list is. Theseus's own development \
         is tracked in the repository's issues and in its tasks and projects, and no other \
         tracker applies: answer a question about what is open from this list, cite the issue \
         numbers, and do not go to a tracker or a website by shell. Read-only."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}, "additionalProperties": false})
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Async
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, _input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        Ok(Plan {
            summary: "list the repository's open issues".into(),
            ..Default::default()
        })
    }

    fn run_async_with_media(&self, _input: &Value, _ctx: &ToolCtx) -> AsyncMediaRun {
        let cfg = self.cfg.clone();
        Box::pin(async move {
            let text =
                theseus_store::blocking(move || listing(&cfg, theseus_protocol::now_unix_ms()))
                    .map_err(ToolFailure::new)?;
            let out = ToolOutput {
                text,
                meta: Value::Null,
            };
            Ok((out, None, None))
        })
    }
}

/// What the tool answers: the open issues by number, oldest first, with the
/// file's age; or the plain reason there is nothing to list. The list is the
/// repository's whole, whatever its size: the night's `max` bounds only what
/// the self backlog takes as candidates, and this repository keeps one issue
/// to a bucket of work.
fn listing(cfg: &SelfIssuesConfig, now_ms: u64) -> Result<String, String> {
    if let Some(why) = cfg.refusal() {
        return Err(format!(
            "Theseus is not set up to read the repository's issues: {why}"
        ));
    }
    let path = crate::config::expand(cfg.file.trim());
    let read_at = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(0));
    let Some(read_at) = read_at else {
        return Err(format!(
            "Theseus has not read the repository's issues yet: {} is not there. Say so; do not \
             look for them anywhere else.",
            path.display()
        ));
    };
    let issues = issues::read(&path)?;
    Ok(render(&issues, cfg, read_at, now_ms))
}

fn render(issues: &[Issue], cfg: &SelfIssuesConfig, read_at: u64, now_ms: u64) -> String {
    let mut open: Vec<&Issue> = issues
        .iter()
        .filter(|i| i.is_open_issue())
        .filter(|i| cfg.version_label.is_empty() || i.has_label(&cfg.version_label))
        .collect();
    open.sort_by_key(|i| i.number);
    let repo = cfg.repo.trim();
    let age = match now_ms.saturating_sub(read_at) / 86_400_000 {
        0 => "today".to_string(),
        1 => "yesterday".to_string(),
        n => format!("{n} days ago"),
    };
    let mut out = format!(
        "Theseus has not read the repository's issues since {} ({age}); this is the list as it \
         stood then.\n",
        crate::owner_note::day(read_at)
    );
    // A version label narrows the list, as it narrows the night's candidates:
    // the header says so, or a count of some would read as a count of all.
    let of = match cfg.version_label.trim() {
        "" => repo.to_string(),
        label => format!("{repo} labelled {label}"),
    };
    if open.is_empty() {
        out.push_str(&format!("No issue of {of} was open in it.\n"));
        return out;
    }
    out.push_str(&format!("{} open in {of}:\n", open.len()));
    for i in open {
        let title = one_line(&i.title);
        let title = if title.is_empty() {
            "(no title)".to_string()
        } else {
            title
        };
        out.push_str(&format!(
            "- #{}: {title} (https://github.com/{repo}/issues/{})\n",
            i.number, i.number
        ));
    }
    out
}

/// A title on one line, cut at the last whole word with an ellipsis when long
/// (a word is never cut in two, unless the title's first is longer than the
/// limit).
fn one_line(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= TITLE_CHARS {
        return one;
    }
    let (cut, _) = crate::scrub::safe::head(&one, TITLE_CHARS);
    let next_is_space = one[cut.len()..].starts_with(' ');
    let whole = match cut.rsplit_once(' ') {
        _ if next_is_space => cut,
        Some((head, _)) if !head.is_empty() => head,
        _ => cut,
    };
    format!("{}…", whole.trim_end())
}

#[cfg(test)]
mod tests;
