//! CONTINUE's candidate signals (M5 25b; design `m5-judgment.md` §2.4; spec
//! §4.4a, step 2): what a compile sees that might make the current
//! compilation stale, read deterministically and cheaply from what
//! `compiler::compile` is given, the clock included (passed in, never read
//! here). They decide nothing: every one that fires rides on
//! `context.compiled`, and a compile that fired one and no deterministic
//! trigger asks `continue.v1` in shadow (`judge::compile`).
//!
//! - **`dormancy`**: the first input since the model's last answer (an
//!   operator's message, a task's report, a wake) came more than
//!   `dormancy_minutes` after the node before it.
//! - **`tail_band`**: the tail (what the request carries past the
//!   compilation's prefix) passed `tail_band` of the window, or a further
//!   quarter past it, with what was written since the last answer.
//! - **`report`** and **`wake`**: a task's report, or a wake, arrived since
//!   the last answer.
//! - **`cache_miss`**: the provider read nothing from its cache on the last
//!   call, where the call before it, of the same compilation, read some: the
//!   prefix did not change, and the cache went cold.
//!
//! "Since the last answer" is what a loop adds: a turn's first loop sees its
//! input, and a later loop sees its tool results, so an input's signals fire
//! once, at its turn's first compile.

use theseus_protocol::signals::CompileSignal;

use crate::catalog::TokenRates;
use crate::compiler::Compilation;
use crate::config::SignalsConfig;
use crate::node::{Body, Node, Origin};
use crate::provider::{Census, ProviderRequest};

/// What a compile needs to read the signals: their thresholds, and the
/// clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignalsAt {
    pub config: SignalsConfig,
    /// Now, in Unix milliseconds: the caller's clock.
    pub now_ms: u64,
}

/// The signals a compile fired, and the sizes `continue.v1` reads beside
/// them. Tokens are estimates from bytes at the catalog's figures (images
/// left out), as the compiler sizes a new part.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Signals {
    pub fired: Vec<CompileSignal>,
    /// The model's window, in tokens (0 when no catalog entry names it).
    pub window: u64,
    /// The request's estimate less the tail's.
    pub prefix_tokens: u64,
    pub tail_tokens: u64,
    /// The provider's cache read on the session's last call.
    pub last_cache_read: u64,
    /// Since the compilation was made, by the caller's clock.
    pub compilation_age_minutes: u64,
}

/// What `read` reads: the compile's nodes and outcome.
pub struct Seen<'a> {
    pub nodes: &'a [(u64, crate::stub::Stub)],
    /// The compilation the request was rendered from.
    pub compilation: &'a Compilation,
    pub request: &'a ProviderRequest,
    /// The index of the request's first message that carries the tail.
    pub tail_from: usize,
    /// The request's estimate, in tokens.
    pub tokens: u64,
    pub window: Option<u64>,
    pub rates: TokenRates,
}

/// The signals, read.
pub fn read(at: &SignalsAt, seen: &Seen<'_>) -> Signals {
    let nodes = seen.nodes;
    let answers: Vec<&Node> = nodes
        .iter()
        .rev()
        .map(|(_, n)| &**n)
        .filter(|n| matches!(n.body, Body::AssistantMessage { .. }))
        .take(2)
        .collect();
    // What was written since the model's last answer.
    let since = nodes
        .iter()
        .rposition(|(_, n)| matches!(n.body, Body::AssistantMessage { .. }))
        .map_or(0, |i| i + 1);
    let mut fired = Vec::new();
    if let Some(s) = dormancy(at, nodes, since) {
        fired.push(s);
    }
    let window = seen.window.unwrap_or(0);
    let messages = &seen.request.messages;
    let tail_from = seen.tail_from.min(messages.len());
    let tail_tokens = Census::of_messages(&messages[tail_from..]).tokens(seen.rates);
    // The tail as the last call sent it: through the last answer.
    let before = messages
        .iter()
        .rposition(|m| m["role"] == "assistant")
        .filter(|&i| i >= tail_from)
        .map_or(0, |i| {
            Census::of_messages(&messages[tail_from..=i]).tokens(seen.rates)
        });
    if let Some(s) = tail_band(at.config.tail_band, window, before, tail_tokens) {
        fired.push(s);
    }
    fired.extend(arrivals(&nodes[since..]));
    let usage = |n: &Node| match &n.body {
        Body::AssistantMessage {
            usage,
            compilation_id,
            ..
        } => Some((usage.cache_read_input_tokens, compilation_id.clone())),
        _ => None,
    };
    let last = answers.first().and_then(|n| usage(n));
    let previous = answers.get(1).and_then(|n| usage(n));
    let mine = Some(seen.compilation.id.clone());
    if let (Some((0, a)), Some((read, b))) = (&last, &previous) {
        if *read > 0 && *a == mine && *b == mine {
            fired.push(CompileSignal {
                name: "cache_miss".into(),
                value: *read,
                detail: format!(
                    "the last call read nothing from the provider's cache; the call before it, with the same prefix, read {read} tokens"
                ),
            });
        }
    }
    Signals {
        fired,
        window,
        prefix_tokens: seen.tokens.saturating_sub(tail_tokens),
        tail_tokens,
        last_cache_read: last.map_or(0, |(r, _)| r),
        compilation_age_minutes: at.now_ms.saturating_sub(seen.compilation.created_at_ms) / 60_000,
    }
}

/// The first input since the last answer, against the node before it.
fn dormancy(
    at: &SignalsAt,
    nodes: &[(u64, crate::stub::Stub)],
    since: usize,
) -> Option<CompileSignal> {
    let k = since
        + nodes[since..]
            .iter()
            .position(|(_, n)| matches!(n.body, Body::UserMessage { .. }))?;
    let before = nodes[..k].last()?;
    let gap = nodes[k]
        .1
        .created_at_ms
        .saturating_sub(before.1.created_at_ms);
    let minutes = gap / 60_000;
    (gap > at.config.dormancy_minutes.saturating_mul(60_000)).then(|| CompileSignal {
        name: "dormancy".into(),
        value: minutes,
        detail: format!(
            "the new input came {} after the node before it",
            words_of_minutes(minutes)
        ),
    })
}

/// The tail's band: 0 below `band` of the window, 1 from it, and one more
/// for each further quarter. The signal fires when the tail's band rose with
/// what was written since the last answer.
fn tail_band(band: f64, window: u64, before: u64, now: u64) -> Option<CompileSignal> {
    if window == 0 {
        return None;
    }
    let first = (band * 100.0).round() as u64;
    let of = |tokens: u64| {
        let percent = tokens.saturating_mul(100) / window;
        match percent.checked_sub(first) {
            Some(past) => 1 + past / 25,
            None => 0,
        }
    };
    let (was, is) = (of(before), of(now));
    (is > was).then(|| {
        let edge = first + (is - 1) * 25;
        CompileSignal {
            name: "tail_band".into(),
            value: edge,
            detail: format!(
                "the tail passed {edge}% of the window: about {now} of {window} tokens, from about {before}"
            ),
        }
    })
}

/// A task's reports and wakes among the nodes written since the last answer.
fn arrivals(new: &[(u64, crate::stub::Stub)]) -> Vec<CompileSignal> {
    let mut reports = Vec::new();
    let mut wakes = Vec::new();
    for (_, n) in new {
        if !matches!(n.body, Body::UserMessage { .. }) || n.origin != Origin::Harness {
            continue;
        }
        let author = n.author.as_deref().unwrap_or_default();
        if let Some(task) = author.strip_prefix("task:") {
            reports.push(task);
        } else if let Some(wake) = author.strip_prefix("wake:") {
            wakes.push(wake);
        }
    }
    let mut out = Vec::new();
    for (name, what, ids) in [("report", "task report", reports), ("wake", "wake", wakes)] {
        if ids.is_empty() {
            continue;
        }
        let s = if ids.len() == 1 { "" } else { "s" };
        out.push(CompileSignal {
            name: name.into(),
            value: ids.len() as u64,
            detail: format!("{} {what}{s} arrived: {}", ids.len(), ids.join(", ")),
        });
    }
    out
}

fn words_of_minutes(m: u64) -> String {
    match m {
        0 => "under a minute".into(),
        1 => "1 minute".into(),
        m if m < 120 => format!("{m} minutes"),
        m => format!("{} hours {} minutes", m / 60, m % 60),
    }
}
