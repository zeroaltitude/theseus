//! Claude Code's session files (JSON Lines, one per session, under a
//! directory per project in `~/.claude/projects`).
//!
//! A line is an object with a `type`. `user` and `assistant` lines carry a
//! `message` (`role`, and `content`: a string, or blocks of `text`,
//! `thinking`, `tool_use` and `tool_result`), a `timestamp`, and often the
//! session's `cwd` and `gitBranch`. Every other type (`summary`,
//! `file-history-snapshot`, `system`, `queue-operation`, …) is the harness's
//! bookkeeping: skipped, counted by its type. A line of a sidechain
//! (`isSidechain`, a subagent's working) or a meta line (`isMeta`, the
//! harness's own notes to the model) is skipped too, and so is a compact
//! summary, which repeats what the file's own lines hold.

use serde_json::Value;

use super::{bounded, call_line, result_line, time_ms, Counts, Draft, Msg, TEXT_MAX};

pub(super) fn line(v: &Value, number: u64, d: &mut Draft, counts: &mut Counts) {
    if d.cwd.is_none() {
        d.cwd = v
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    if d.branch.is_none() {
        d.branch = v
            .get("gitBranch")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && *s != "HEAD")
            .map(str::to_string);
    }
    let kind = v.get("type").and_then(Value::as_str).unwrap_or("");
    let role = match kind {
        "user" | "assistant" => kind,
        "" => return counts.skip("(no type)"),
        other => return counts.skip(other),
    };
    let flag = |k: &str| v.get(k).and_then(Value::as_bool) == Some(true);
    if flag("isSidechain") {
        return counts.skip("sidechain");
    }
    if flag("isMeta") {
        return counts.skip("meta");
    }
    if flag("isCompactSummary") {
        return counts.skip("compact_summary");
    }
    let Some(at_ms) = v.get("timestamp").and_then(time_ms) else {
        return reject(counts, number, "a message line with no readable timestamp");
    };
    let Some(content) = v.get("message").and_then(|m| m.get("content")) else {
        return reject(counts, number, "a message line with no message content");
    };
    let mut out = |author: &str, integrity: &'static str, text: String| {
        d.msgs.push(Msg {
            author: author.to_string(),
            integrity,
            text,
            at_ms,
            line: number,
        });
    };
    let (author, integrity) = match role {
        "user" => ("user", "operator"),
        _ => ("agent:claude-code", "agent"),
    };
    match content {
        Value::String(s) => {
            if !s.trim().is_empty() {
                out(author, integrity, bounded(s, TEXT_MAX, counts));
            }
        }
        Value::Array(blocks) => {
            for b in blocks {
                match b.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text" => {
                        let t = b.get("text").and_then(Value::as_str).unwrap_or("");
                        if !t.trim().is_empty() {
                            out(author, integrity, bounded(t, TEXT_MAX, counts));
                        }
                    }
                    "thinking" | "redacted_thinking" => counts.skip("thinking"),
                    "tool_use" => {
                        let name = b.get("name").and_then(Value::as_str).unwrap_or("?");
                        let input = b.get("input").unwrap_or(&Value::Null);
                        out("agent:claude-code", "agent", call_line(name, input, counts));
                    }
                    "tool_result" => {
                        let error = b.get("is_error").and_then(Value::as_bool) == Some(true);
                        let text = result_text(b.get("content"));
                        out("tool", "outside", result_line(&text, error, counts));
                    }
                    "image" => counts.skip("image"),
                    other => counts.skip(&format!("block:{other}")),
                }
            }
        }
        _ => reject(counts, number, "message content is neither text nor blocks"),
    }
}

fn reject(counts: &mut Counts, number: u64, why: &str) {
    counts.rejected.push((number, why.to_string()));
}

/// A result's text: a string, or the text of its blocks (an image as a note).
fn result_text(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a
            .iter()
            .map(|b| match b.get("type").and_then(Value::as_str) {
                Some("text") => b
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                Some(t) => format!("[{t}]"),
                None => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
