//! Old results cleared (theseus-3ki0, by tokens at theseus-5lzox): the first
//! thing a long session gives up, before the ring drops a turn.
//!
//! A pass runs when the request's results not yet cleared pass
//! `CLEAR_AT_TOKENS`, or its estimate passes `CLEAR_AT` % of its budget,
//! whichever comes first (so a small window clears no later than it did).
//! It renders as a one-line stub every result longer than `MIN_CHARS`
//! characters that is not among the newest results whose tokens sum to
//! `keep` (at most `KEEP_TOKENS`, and at most what leaves the request under
//! `CLEAR_TO` %; the newest exchange's results always stay whole), oldest
//! first. A pass is one recompile: the compilation's report keeps each
//! cleared result as a drop of tier `cleared` (`node_id`, the tokens it
//! saved), which the render reads, so every later append renders the same
//! stubs byte for byte and the cache holds until the next pass. A pass that
//! cannot take the estimate halfway back from the line to the keep does not
//! run (`clear`'s doc says the formula): the newest exchange alone holds too
//! much, the next loop would pass the line again, and the ring makes room
//! instead. Past the percent line, where the 20-exchange rule cleared, a pass
//! the guard refuses is tried again keeping only the newest exchange whole and
//! taken when it ends under that rule's own line (`CLEAR_AT` + `CLEAR_TO`, halved):
//! results under `MIN_CHARS` are never cleared, and without the retry a session
//! of them would climb to the ring where the old rule made room. The stub names the call, what part of a file it read, what the
//! model wrote after it, and how to get the result back: the kept output's
//! file when there is one (`remember.rs`).

use std::collections::HashSet;

use serde_json::{json, Value};
use theseus_protocol::memory::{BudgetDrop, BudgetReport};

pub(super) mod remember;
pub(super) use remember::notes;

use super::{counted_part, estimate, render_request, situation};
use super::{Compilation, CompileInput, Estimate, Manifest, Rendered, RequestSpec};
use crate::catalog::TokenRates;
use crate::node::{Body, Node};
use crate::provider::ProviderRequest;
use crate::stub::{Kind, Stub};

/// The clearing line, in percent of the request's budget: a small window's
/// line, and the later of the two lines on a big one.
pub const CLEAR_AT: u64 = 50;
/// Where a pass stops on a small window, in percent of the budget.
pub const CLEAR_TO: u64 = 35;
/// The clearing line in tokens of results not yet cleared. The owner's bug
/// hunt of 2026-10-24 (a 1M window, 12-25K-token reads) cleared at 50 % (430K)
/// down to 270K and fired again 13 calls later: a sawtooth that rewrote the
/// cache seven times a trial, a third of its cost.
pub const CLEAR_AT_TOKENS: u64 = 150_000;
/// The newest results kept whole, in tokens. With the trigger above, a pass
/// leaves about 40K of results, so its rewrite is small and the model keeps
/// what it read last, not what it read first (the hunt re-read 66 % of its
/// lines after the old rule cleared its opening files). Chosen from the
/// replay's grid (`tests_replay.rs`): the cheapest cell of the owner's ranges
/// within 5 %, the lower mean context winning the tie.
pub const KEEP_TOKENS: u64 = 40_000;
/// A result this long or shorter stays whole.
pub const MIN_CHARS: usize = 2_000;
/// The drops' tier and reason.
pub const TIER: &str = "cleared";

/// The trigger and the keep in tokens: the constants, but a test may try
/// others on its own thread (the replay's grid).
fn rule() -> (u64, u64) {
    #[cfg(test)]
    if let Some(r) = TRIED.get() {
        return r;
    }
    (CLEAR_AT_TOKENS, KEEP_TOKENS)
}

#[cfg(test)]
thread_local! {
    pub(super) static TRIED: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
}

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
/// `est`, when its results not yet cleared (`live`) pass `CLEAR_AT_TOKENS` or
/// the estimate passes `CLEAR_AT` % of the budget, and a pass would take the
/// estimate at least halfway from the line to the keep. With `overhead` the
/// estimate less `live`: the line is the lower of `overhead + CLEAR_AT_TOKENS`
/// and `CLEAR_AT` % of the budget, the keep the lower of `KEEP_TOKENS` and
/// what leaves `CLEAR_TO` % less `overhead`, and the pass must leave the
/// estimate at most `(line + overhead + keep) / 2`. Where the newest
/// exchange alone holds more, there is no pass and the ring makes room; but
/// past the percent line a refused pass is tried again at keep 0 (only the
/// newest exchange whole) and taken at the old rule's midpoint, so a request
/// is never larger than the 20-exchange rule left it. The same nodes as a new compilation (trigger `why`), the results it clears and
/// those cleared before as its report's drops. Reads stub fields first, and
/// decodes only when the estimate passes the token line.
pub(super) fn clear(
    input: &CompileInput<'_>,
    at: &At<'_>,
    (c, request, est): (&Compilation, &ProviderRequest, &Estimate),
    make: &super::ring::Make<'_>,
) -> Option<Made> {
    let (at_tokens, keep_tokens) = rule();
    let by_percent = est.tokens > at.budget * CLEAR_AT / 100;
    if !(by_percent || est.tokens > at_tokens) || !any_old(input.nodes) {
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
    let live = live_tokens(&shown, &already, per_byte);
    if !by_percent && live <= at_tokens {
        return None;
    }
    let overhead = est.tokens.saturating_sub(live);
    let line = (overhead + at_tokens).min(at.budget * CLEAR_AT / 100);
    let keep = keep_tokens.min((at.budget * CLEAR_TO / 100).saturating_sub(overhead));
    let ends = |new: &[BudgetDrop]| {
        est.tokens
            .saturating_sub(new.iter().map(|d| d.tokens).sum::<u64>())
    };
    let (mut new, mut chars) = pass(&shown, &already, keep, per_byte);
    if new.is_empty() || ends(&new) > (line + overhead + keep) / 2 {
        // Past the percent line the 20-exchange rule cleared, so a refusal
        // here must not leave the request larger than it ended there: small
        // results, which no pass clears, count in `live` and hold the guard
        // below what any pass can reach. Keep only the newest exchange whole
        // and take the pass under the old rule's own line. Everything the
        // old rule cleared is cleared by this one, so it passes when the old
        // pass did. Under the percent line the old rule did nothing, and a
        // refused pass stays refused.
        if !by_percent {
            return None;
        }
        (new, chars) = pass(&shown, &already, 0, per_byte);
        if new.is_empty() || ends(&new) > at.budget * (CLEAR_AT + CLEAR_TO) / 200 {
            return None;
        }
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

/// The tokens of the results among `shown` that a pass could clear or keep:
/// not late, not cleared already, by `pass`'s own estimate.
fn live_tokens(shown: &[(u64, &Node)], already: &HashSet<&str>, per_byte: f64) -> u64 {
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
