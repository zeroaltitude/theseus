//! Old results cleared (theseus-3ki0, by tokens at theseus-5lzox): the pieces the view's clearing pass is made
//! of (`compiler::view`, on its own numbers). A pass renders as a one-line stub every result longer than
//! `MIN_CHARS` characters that is not among the newest results whose tokens sum to its keep, oldest first; the
//! compilation's report keeps each cleared result as a drop of tier `cleared` (`node_id`, the tokens it saved),
//! which the render reads, so every later call of the chain renders the same stubs byte for byte. The stub
//! names the call, what part of a file it read, what the model wrote after it, and how to get the result back:
//! the kept output's file when there is one (`remember.rs`).

use std::collections::HashSet;

use serde_json::{json, Value};
use theseus_protocol::memory::{BudgetDrop, BudgetReport};

pub(super) mod remember;
pub(super) use remember::notes;

use super::Compilation;
use crate::node::{Body, Node};
use crate::scrub::safe::head_marked;
use crate::stub::{Kind, Stub};

/// Half the request's budget: the line a task's fan-in keeps its reports under (`task_wait::fan_in_bound`).
pub const CLEAR_AT: u64 = 50;
/// A result this long or shorter stays whole.
pub const MIN_CHARS: usize = 2_000;
/// The drops' tier and reason.
pub const TIER: &str = "cleared";

/// What a pass did, for its `context.cleared` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pass {
    /// The results it cleared, and their characters.
    pub results: u64,
    pub chars: u64,
    /// The results cleared before it, which it kept so.
    pub kept: u64,
    /// The request's estimate before the pass and after it.
    pub tokens_before: u64,
    pub tokens_after: u64,
}

/// The results `c`'s report says are cleared.
pub(super) fn of(c: &Compilation) -> HashSet<&str> {
    drops(c.budget.as_ref())
        .filter_map(|d| d.node_id.as_deref())
        .collect()
}

/// The cleared drops of a report.
pub(crate) fn drops(b: Option<&BudgetReport>) -> impl Iterator<Item = &BudgetDrop> {
    b.into_iter()
        .flat_map(|b| b.dropped.iter())
        .filter(|d| d.tier == TIER)
}

/// The tokens of the results among `shown` that a pass could clear or keep:
/// not late, not cleared already, by `pass`'s own estimate.
pub(super) fn live_tokens(shown: &[(u64, &Node)], already: &HashSet<&str>, per_byte: f64) -> u64 {
    shown
        .iter()
        .filter_map(|(_, n)| result_tokens(n, already, per_byte))
        .sum()
}

/// A result's tokens, none for a node a pass leaves alone.
fn result_tokens(n: &Node, already: &HashSet<&str>, per_byte: f64) -> Option<u64> {
    match &n.body {
        Body::ToolResult {
            content,
            late: false,
            ..
        } if !already.contains(n.id.as_str()) => {
            Some((content.len().saturating_sub(200) as f64 * per_byte).ceil() as u64)
        }
        _ => None,
    }
}

/// A pass over the nodes a compilation renders (`shown`, in position
/// order): walking back from the newest, the results whose tokens sum to at
/// most `keep` stay whole (the newest exchange's, those after the last
/// answer, always do), and every older one longer than `MIN_CHARS` is
/// cleared, in position order. `per_byte` is the request's tokens per byte.
/// Results cleared already (`already`) are not counted again. With the
/// drops, the characters of the results they clear.
pub(super) fn pass(
    shown: &[(u64, &Node)],
    already: &HashSet<&str>,
    keep: u64,
    per_byte: f64,
) -> (Vec<BudgetDrop>, u64) {
    let last_answer = shown
        .iter()
        .rev()
        .find(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        .map_or(0, |(p, _)| *p);
    let (mut held, mut chars, mut cutting) = (0u64, 0u64, false);
    let mut out = Vec::new();
    for (p, n) in shown.iter().rev() {
        let Some(tokens) = result_tokens(n, already, per_byte) else {
            continue;
        };
        cutting = cutting || (*p < last_answer && held + tokens > keep);
        if !cutting {
            held += tokens;
            continue;
        }
        let Body::ToolResult { content, .. } = &n.body else {
            continue;
        };
        let count = content.chars().count();
        if count <= MIN_CHARS {
            continue;
        }
        chars += count as u64;
        out.push(BudgetDrop {
            node_id: Some(n.id.clone()),
            range: None,
            reason: TIER.into(),
            tokens,
            tier: TIER.into(),
        });
    }
    out.reverse();
    (out, chars)
}

/// Whether a pass may clear anything among a session's nodes at all: a
/// result with an answer after it (the newest exchange's never clears).
/// Reads stub fields only.
pub(super) fn any_old(nodes: &[(u64, Stub)]) -> bool {
    let mut answered = false;
    nodes.iter().rev().any(|(_, n)| {
        answered = answered || n.kind == Kind::AssistantMessage;
        answered && n.kind == Kind::ToolResult
    })
}

/// A cleared result's block, in place of its content: what the call was,
/// how long its result ran, what the model wrote next (`note`), and how to
/// get it back.
pub(super) fn block(name: &str, input: &Value, r: &Node, note: Option<&str>) -> Value {
    let Body::ToolResult {
        tool_use_id,
        is_error,
        ..
    } = &r.body
    else {
        return Value::Null;
    };
    let content = stub_noted(name, input, r, note);
    match is_error {
        true => {
            json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": true})
        }
        false => json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content}),
    }
}

/// The stub's text, with no note:
/// `[Result cleared to save context: fs_read src/lib.rs, lines 1-400 of 1,210
/// (18,233 characters). You read it, and your notes after it still stand; read
/// again only the line range you need.]`, or for a program,
/// `[Result cleared to save context: proc_run cargo test (exit 101, 41,002
/// characters); the whole output is at <path>.]` when its output was kept.
#[cfg(test)]
pub fn stub(name: &str, input: &Value, r: &Node) -> String {
    stub_noted(name, input, r, None)
}

/// `stub`, ending with what the model wrote after the result when it wrote
/// text: ` After it you wrote: "…".`
pub fn stub_noted(name: &str, input: &Value, r: &Node, note: Option<&str>) -> String {
    let Body::ToolResult {
        content,
        full_ref,
        meta,
        ..
    } = &r.body
    else {
        return String::new();
    };
    let chars = crate::narrative::thousands(content.chars().count() as u64);
    let head = "[Result cleared to save context:";
    let str_of = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or_default();
    let n = |k: &str| input.get(k).and_then(Value::as_u64);
    let note = note.map_or(String::new(), |t| format!(" After it you wrote: \"{t}\"."));
    match name {
        "fs_read" => {
            let lines = match remember::read_lines(content, meta) {
                Some((a, b, Some(total))) => {
                    format!(", lines {a}-{b} of {}", crate::narrative::thousands(total))
                }
                Some((a, b, None)) => format!(", lines {a}-{b}"),
                None => match (n("offset"), n("limit")) {
                    (None, None) => String::new(),
                    (o, l) => {
                        let from = o.unwrap_or(1);
                        match l {
                            Some(l) => format!(", lines {from}-{}", from + l.saturating_sub(1)),
                            None => format!(", from line {from}"),
                        }
                    }
                },
            };
            format!(
                "{head} {name} {}{lines} ({chars} characters). You read it, and your notes after it \
                 still stand; read again only the line range you need.{note}]",
                str_of("path")
            )
        }
        "proc_run" => {
            let argv = argv_of(input);
            let exit = match meta.get("exit_code").and_then(Value::as_i64) {
                Some(code) => format!("exit {code}, "),
                None => String::new(),
            };
            match full_ref {
                Some(path) => format!(
                    "{head} {name} {argv} ({exit}{chars} characters); the whole output is at {path}.{note}]"
                ),
                None => format!(
                    "{head} {name} {argv} ({exit}{chars} characters). Run it again if you need its output.{note}]"
                ),
            }
        }
        _ => format!(
            "{head} {name} {} ({chars} characters). Call it again if you need it.{note}]",
            head_marked(&input.to_string(), 120)
        ),
    }
}

/// A program call's argv as one line, or its steps' joined by `&&`.
fn argv_of(input: &Value) -> String {
    let join = |v: &Value| {
        v.as_array()
            .map(|a| {
                a.iter()
                    .map(|s| s.as_str().unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default()
    };
    let line = match (input.get("argv"), input.get("steps")) {
        (Some(argv), _) => join(argv),
        (None, Some(steps)) => steps
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| join(&s["argv"]))
            .collect::<Vec<_>>()
            .join(" && "),
        _ => String::new(),
    };
    head_marked(&line, 200)
}
