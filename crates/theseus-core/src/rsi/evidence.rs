//! `self.evidence` (theseus-cudar): what a self session needs to look at
//! how things went, read-only, so it never greps the state dir or its store
//! (the store holds conversation content: the floor, which always asks).
//!
//! - **`cause`**: the ledger rows of a cause key (`turn.failed:kernel`) in a
//!   time range, as the night read them (`backlog::read::from_ledger`).
//! - **`reports`**: the reports earlier runs of an item left, newest first;
//!   with a `name`, one of them.
//! - **`gates`**: the gate scripts and logs of the runs, names; with a
//!   `name`, one of them.
//!
//! Nothing here writes, runs or reaches past `<state>/self/{reports,gates}`:
//! a name is a plain file name there. Only a self session may call it.
//! `briefing` is the same reports, as the paths a brief hands an item.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::backlog;
use super::tree;
use crate::store::Store;

pub const EVIDENCE: &str = "self.evidence";
pub const NAMES: [&str; 1] = [EVIDENCE];

/// A report or a script, cut at this many bytes.
const MAX_BYTES: usize = 20_000;
/// Rows or names listed, at most.
const MAX_LISTED: usize = 50;

/// Register `self.evidence`, only while `[self] mode = "act"`.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
    if cfg.self_improve.mode == theseus_protocol::rsi::SelfMode::Act {
        r.register(Arc::new(Evidence));
    }
}

pub struct Evidence;

impl Tool for Evidence {
    fn name(&self) -> &str {
        EVIDENCE
    }

    fn description(&self) -> &str {
        "Read how Theseus's own runs went, read-only. kind \"cause\": the ledger rows of a cause \
         key such as `turn.failed:kernel`, within `since_hours` back to `until_hours` back \
         (default 24 to 0). kind \"reports\": the reports earlier runs of `item` left, newest \
         first; with `name`, one of them. kind \"gates\": the gate scripts and logs of the runs; \
         with `name`, one of them. Use this instead of searching the state dir or its store, \
         which always asks. Only a self session may call it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "kind": {"type": "string", "enum": ["cause", "reports", "gates"]},
                "cause": {"type": "string", "description": "A cause key, e.g. turn.failed:kernel."},
                "since_hours": {"type": "number", "description": "How far back to start, in hours (default 24)."},
                "until_hours": {"type": "number", "description": "How far back to stop, in hours (default 0, now)."},
                "item": {"type": "string", "description": "The backlog item's id, for its reports."},
                "name": {"type": "string", "description": "One report or gate file, by the name a listing gave."},
                "limit": {"type": "integer", "description": "Rows listed, at most 50 (default 20)."}
            },
            "required": ["kind"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = input_of(input)?;
        Ok(Plan {
            summary: format!("self evidence ({})", i.kind),
            ..Default::default()
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    kind: String,
    cause: Option<String>,
    since_hours: Option<f64>,
    until_hours: Option<f64>,
    item: Option<String>,
    name: Option<String>,
    limit: Option<usize>,
}

fn input_of(v: &Value) -> Result<Input, String> {
    let i: Input = serde_json::from_value(v.clone()).map_err(|e| format!("invalid input: {e}"))?;
    match i.kind.as_str() {
        "cause" if i.cause.as_deref().is_none_or(|c| c.trim().is_empty()) => {
            Err("kind cause needs a `cause` key".into())
        }
        "reports" if i.item.as_deref().is_none_or(|c| c.trim().is_empty()) => {
            Err("kind reports needs an `item`".into())
        }
        "cause" | "reports" | "gates" => Ok(i),
        other => Err(format!("kind {other} is not cause, reports or gates")),
    }
}

/// A file name that stays in its directory.
fn plain_name(n: &str) -> Result<&str, String> {
    let ok =
        !n.is_empty() && !n.contains(['/', '\\']) && n != ".." && n != "." && !n.starts_with('.');
    ok.then_some(n)
        .ok_or_else(|| format!("`{n}` is not a file name from a listing"))
}

fn clock(ms: u64) -> String {
    let l = crate::wake::local(ms);
    format!("{:02}-{:02} {:02}:{:02}", l.month, l.day, l.hour, l.minute)
}

fn modified_ms(p: &Path) -> u64 {
    std::fs::metadata(p)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
}

/// The files of `dir` whose names contain `needle` (all when empty), newest
/// first, with their modified times.
fn files(dir: &Path, needle: &str) -> Vec<(PathBuf, u64)> {
    let mut v: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file() && e.file_name().to_string_lossy().contains(needle))
        .map(|e| (e.path(), modified_ms(&e.path())))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

fn listing(what: &str, found: &[(PathBuf, u64)], limit: usize) -> String {
    if found.is_empty() {
        return format!("no {what} yet");
    }
    let mut out = format!("{} {what}, newest first:", found.len());
    for (p, ms) in found.iter().take(limit) {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        out.push_str(&format!("\n- {name} ({})", clock(*ms)));
    }
    out
}

/// One file of `dir`, by name, cut at `MAX_BYTES` on a character boundary.
fn one(dir: &Path, name: &str) -> Result<String, String> {
    let p = dir.join(plain_name(name)?);
    let bytes = std::fs::read(&p).map_err(|_| format!("`{name}` is not in the listing"))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(match text.char_indices().nth(MAX_BYTES) {
        Some((at, _)) => format!("{}\n[cut at {MAX_BYTES} characters]", &text[..at]),
        None => text.into_owned(),
    })
}

/// The call's result.
pub fn run(
    state: &Path,
    store: &Store,
    now_ms: u64,
    input: &Value,
) -> Result<(String, Value), String> {
    let i = input_of(input)?;
    let limit = i.limit.unwrap_or(20).clamp(1, MAX_LISTED);
    let (reports, gates) = (state.join("self/reports"), state.join("self/gates"));
    let text = match i.kind.as_str() {
        "cause" => {
            let hours = |h: Option<f64>, d: f64| (h.unwrap_or(d).max(0.0) * 3_600_000.0) as u64;
            let since = now_ms.saturating_sub(hours(i.since_hours, 24.0));
            let until = now_ms.saturating_sub(hours(i.until_hours, 0.0));
            let key = i.cause.unwrap_or_default();
            let key = key.trim();
            let mut rows: Vec<backlog::Obs> = backlog::from_ledger(store, since, until)
                .map_err(|e| format!("the ledger could not be read just now: {e:#}"))?
                .into_iter()
                .filter(|o| o.key == key)
                .collect();
            rows.sort_by_key(|o| std::cmp::Reverse(o.at_ms));
            let mut out = format!("{} rows of `{key}` in the range", rows.len());
            for o in rows.iter().take(limit) {
                out.push_str(&format!("\n- {} {} · {}", clock(o.at_ms), o.id, o.note));
            }
            out
        }
        "reports" => {
            let slug = tree::slug(i.item.as_deref().unwrap_or_default());
            match i.name.as_deref() {
                Some(n) => one(&reports, n)?,
                None => listing("reports", &files(&reports, &slug), limit),
            }
        }
        _ => match i.name.as_deref() {
            Some(n) => one(&gates, n)?,
            None => listing("gate files", &files(&gates, ""), limit),
        },
    };
    Ok((text, Value::Null))
}

/// What a brief hands an item that has run before: the paths of its
/// earlier reports and gate logs, and where to look for more.
pub fn briefing(state: &Path, item: &str) -> String {
    let slug = tree::slug(item);
    let mut out = String::from(
        "\n\nTo see how earlier runs went, call `self.evidence` (ledger rows of a cause, an \
         item's reports, gate files); do not search the state dir or its store.",
    );
    for (what, dir) in [("reports", "self/reports"), ("gate logs", "self/gates")] {
        let found = files(&state.join(dir), &slug);
        if found.is_empty() {
            continue;
        }
        out.push_str(&format!("\nEarlier {what} of this item:"));
        for (p, ms) in found.iter().take(5) {
            out.push_str(&format!("\n- {} ({})", p.display(), clock(*ms)));
        }
    }
    out
}

#[cfg(test)]
mod tests;
