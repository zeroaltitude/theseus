//! Old results cleared (theseus-3ki0): the first thing a long session gives
//! up, before the ring drops a turn.
//!
//! When a request's estimate passes `CLEAR_AT` % of its budget, the results
//! of calls older than the newest `KEEP_EXCHANGES` exchanges, each longer
//! than `MIN_CHARS` characters, render as a one-line stub, oldest first,
//! until the estimate is under `CLEAR_TO` %. A pass is one recompile: the
//! compilation's report keeps each cleared result as a drop of tier
//! `cleared` (`node_id`, the tokens it saved), which the render reads, so
//! every later append renders the same stubs and the next pass, rare from
//! 35 % back to 50 %, starts from them. A pass that cannot take the
//! estimate halfway back from the line to 35 % does not run: the newest
//! exchanges alone hold too much, the next loop would pass the line again,
//! and the ring makes room instead. The stub names the call and how to
//! get the result back: the kept output's file when there is one.

use std::collections::HashSet;

use serde_json::{json, Value};
use theseus_protocol::memory::{BudgetDrop, BudgetReport};

use super::{counted_part, estimate, render_request, situation};
use super::{Compilation, CompileInput, Estimate, Manifest, Rendered, RequestSpec};
use crate::catalog::TokenRates;
use crate::node::{Body, Node};
use crate::provider::ProviderRequest;
use crate::stub::{Kind, Stub};

/// The clearing line, in percent of the request's budget.
pub const CLEAR_AT: u64 = 50;
/// Where a pass stops, in percent of the budget.
pub const CLEAR_TO: u64 = 35;
/// The newest exchanges (the model's answers) whose results stay whole.
pub const KEEP_EXCHANGES: usize = 20;
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

/// What a pass reads besides the compile's input.
pub(super) struct At<'a> {
    /// The spec a recompile renders: a pass is a new compilation.
    pub spec: &'a RequestSpec,
    pub base: &'a Manifest,
    pub budget: u64,
    pub rates: TokenRates,
    /// The compile made a new compilation already: the pass's trigger is
    /// its trigger and `+cleared`, else `cleared`.
    pub new: bool,
}

/// A pass's outcome: the compilation it made, its render and estimate.
pub(super) struct Made {
    pub compilation: Compilation,
    pub rendered: Rendered,
    pub est: Estimate,
    pub pass: Pass,
}

/// A clearing pass over `c`, rendered as `request` and estimated at
/// `est`, when the estimate passes the clearing line and a pass would take
/// it at least halfway back to `CLEAR_TO` (so the next pass is that far
/// away; where the newest exchanges alone hold more, there is no pass, and
/// the ring makes room): the same nodes as a new compilation (trigger
/// `why`), the results it clears and those cleared before as its report's
/// drops. Reads stub fields first, and decodes only when a result is old
/// enough to clear.
pub(super) fn clear(
    input: &CompileInput<'_>,
    at: &At<'_>,
    (c, request, est): (&Compilation, &ProviderRequest, &Estimate),
    make: &super::ring::Make<'_>,
) -> Option<Made> {
    if est.tokens <= at.budget * CLEAR_AT / 100 || !any_old(input.nodes) {
        return None;
    }
    let (prefix, tail) = situation::selected(c, input.nodes);
    let mut shown: Vec<(u64, &Node)> = prefix
        .into_iter()
        .chain(
            tail.into_iter()
                .filter(|(_, n)| !matches!(n.body, Body::Summary { .. })),
        )
        .collect();
    shown.sort_by_key(|(p, _)| *p);
    let already = of(c);
    let per_byte = est.tokens as f64 / request.json_bytes().max(1) as f64;
    let to = at.budget * CLEAR_TO / 100;
    let (new, chars) = pass(&shown, &already, est.tokens, to, per_byte);
    let saved: u64 = new.iter().map(|d| d.tokens).sum();
    if new.is_empty() || est.tokens.saturating_sub(saved) > at.budget * (CLEAR_AT + CLEAR_TO) / 200
    {
        return None;
    }
    let includes: Vec<String> = shown.iter().map(|(_, n)| n.id.clone()).collect();
    let why = match at.new {
        true => format!("{}+cleared", c.trigger),
        false => "cleared".into(),
    };
    let mut made = make(why, &c.strategy, true, includes, at.base);
    let ids: HashSet<&str> = made.includes.iter().map(String::as_str).collect();
    let kept: Vec<BudgetDrop> = drops(c.budget.as_ref())
        .filter(|d| d.node_id.as_deref().is_some_and(|id| ids.contains(id)))
        .cloned()
        .collect();
    let pass = Pass {
        results: new.len() as u64,
        chars,
        kept: kept.len() as u64,
        tokens_before: est.tokens,
        tokens_after: 0,
    };
    made.budget = Some(BudgetReport {
        dropped: kept.into_iter().chain(new).collect(),
        ..Default::default()
    });
    let media = (
        input.blobs,
        input.hidden,
        input.overflowed.and_then(|o| o.retrying.as_deref()),
        input.sources,
    );
    let rendered = render_request(at.spec, input.catalog, &made, input.nodes, media);
    let counted = counted_part(input.nodes, &made, &rendered.request);
    let est = estimate(&rendered.request, at.rates, counted);
    Some(Made {
        pass: Pass {
            tokens_after: est.tokens,
            ..pass
        },
        compilation: made,
        rendered,
        est,
    })
}

/// The results `c`'s report says are cleared.
pub(super) fn of(c: &Compilation) -> HashSet<&str> {
    drops(c.budget.as_ref())
        .filter_map(|d| d.node_id.as_deref())
        .collect()
}

/// The cleared drops of a report.
pub(super) fn drops(b: Option<&BudgetReport>) -> impl Iterator<Item = &BudgetDrop> {
    b.into_iter()
        .flat_map(|b| b.dropped.iter())
        .filter(|d| d.tier == TIER)
}

/// A pass over the nodes a compilation renders (`shown`, in position
/// order): the results to clear, oldest first, until `tokens` less what
/// they save is at most `to`. `per_byte` is the request's tokens per byte.
/// Results cleared already (`already`) are not counted again. With the
/// drops, the characters of the results they clear.
pub(super) fn pass(
    shown: &[(u64, &Node)],
    already: &HashSet<&str>,
    tokens: u64,
    to: u64,
    per_byte: f64,
) -> (Vec<BudgetDrop>, u64) {
    let answers: Vec<u64> = shown
        .iter()
        .filter(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        .map(|(p, _)| *p)
        .collect();
    let Some(&line) = answers.iter().rev().nth(KEEP_EXCHANGES - 1) else {
        return (Vec::new(), 0);
    };
    let (mut saved, mut chars) = (0u64, 0u64);
    let mut out = Vec::new();
    for (p, n) in shown {
        if *p >= line || tokens.saturating_sub(saved) <= to {
            break;
        }
        let Body::ToolResult {
            content,
            late: false,
            ..
        } = &n.body
        else {
            continue;
        };
        let count = content.chars().count();
        if already.contains(n.id.as_str()) || count <= MIN_CHARS {
            continue;
        }
        chars += count as u64;
        let tokens = (content.len().saturating_sub(200) as f64 * per_byte).ceil() as u64;
        saved += tokens;
        out.push(BudgetDrop {
            node_id: Some(n.id.clone()),
            range: None,
            reason: TIER.into(),
            tokens,
            tier: TIER.into(),
        });
    }
    (out, chars)
}

/// Whether a pass may clear anything among a session's nodes at all: a
/// result older than the newest `KEEP_EXCHANGES` answers. Reads stub
/// fields only.
pub(super) fn any_old(nodes: &[(u64, Stub)]) -> bool {
    let mut answers = 0;
    nodes.iter().rev().any(|(_, n)| {
        if n.kind == Kind::AssistantMessage {
            answers += 1;
        }
        answers >= KEEP_EXCHANGES && n.kind == Kind::ToolResult
    })
}

/// A cleared result's block, in place of its content: what the call was,
/// how long its result ran, and how to get it back.
pub(super) fn block(name: &str, input: &Value, r: &Node) -> Value {
    let Body::ToolResult {
        tool_use_id,
        is_error,
        ..
    } = &r.body
    else {
        return Value::Null;
    };
    let content = stub(name, input, r);
    match is_error {
        true => {
            json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": true})
        }
        false => json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content}),
    }
}

/// The stub's text:
/// `[Result cleared to save context: fs_read src/lib.rs, lines 1-400
/// (18,233 characters). Read it again if you need it.]`, or for a program,
/// `[Result cleared to save context: proc_run cargo test (exit 101, 41,002
/// characters); the whole output is at <path>.]` when its output was kept.
pub fn stub(name: &str, input: &Value, r: &Node) -> String {
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
    match name {
        "fs_read" => {
            let lines = match (n("offset"), n("limit")) {
                (None, None) => String::new(),
                (o, l) => {
                    let from = o.unwrap_or(1);
                    match l {
                        Some(l) => format!(", lines {from}-{}", from + l.saturating_sub(1)),
                        None => format!(", from line {from}"),
                    }
                }
            };
            format!(
                "{head} {name} {}{lines} ({chars} characters). Read it again if you need it.]",
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
                    "{head} {name} {argv} ({exit}{chars} characters); the whole output is at {path}.]"
                ),
                None => format!(
                    "{head} {name} {argv} ({exit}{chars} characters). Run it again if you need its output.]"
                ),
            }
        }
        _ => format!(
            "{head} {name} {} ({chars} characters). Call it again if you need it.]",
            short(&input.to_string(), 120)
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
    short(&line, 200)
}

/// `s` cut at `max` characters, with an ellipsis when cut.
fn short(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}
