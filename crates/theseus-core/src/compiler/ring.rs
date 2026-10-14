//! The ring (M3; M6 30b, §2.11): where a request passes the window, drop
//! leading turns. Moved out of `compiler.rs` (theseus-3ki0) with no change
//! of behaviour; the compiler's estimate, render and report stay there.
//!
//! It cuts only before a user message. It rings on the estimate's upper
//! bound, so a request whose bytes run denser than the catalog's figures
//! still rings in time; it keeps turns while the estimate itself is under
//! 60 %, and that candidate's bound, at most 84 %, does not ring again
//! (theseus-f5hf). A request the provider said passed the window rings
//! whatever the estimate says, and its candidates are read at the
//! provider's count: each estimate is raised by how far the overflowing
//! request's count ran over its estimate (theseus-9p88).
//!
//! **Inside a turn** (theseus-3ki0): when the range holds only one user
//! message (a long single instruction, its turn's loops after it), there is
//! no user boundary to cut at, so the ring may also cut at an exchange
//! boundary after it: the start of an answer of the model's whose earlier
//! calls all have their results before it, so no call is left without its
//! result and no result without its call. The instruction (the message and
//! what came with it before the first answer) is pinned: always kept,
//! verbatim, at the top. Those candidates are searched by halves (the
//! estimate falls as the cut moves on), so a long turn costs a few renders.

use std::collections::HashMap;

use theseus_protocol::memory::{BudgetDrop, BudgetRange, BudgetReport};

use super::{compaction, estimate, render_request, Compilation, CompileInput, Estimate, Rendered};
use super::{Manifest, RequestSpec};
use crate::catalog::TokenRates;
use crate::stub::{Kind, Stub};

/// How a compile makes a new compilation: its trigger, strategy, whether it
/// strips the prefix's thinking, its includes, and the manifest it takes.
pub(super) type Make<'m> = dyn Fn(String, &str, bool, Vec<String>, &Manifest) -> Compilation + 'm;

/// A ring's outcome: the compilation it made, its render and estimate,
/// and its cut.
pub(super) struct Rung {
    pub compilation: Compilation,
    pub rendered: Rendered,
    pub est: Estimate,
    pub cut: BudgetDrop,
}

/// What the ring reads besides the compile's input.
pub(super) struct Ring<'a> {
    /// The spec a recompile renders: a ring is a new compilation.
    pub recompiled: &'a RequestSpec,
    /// The manifest a new compilation takes (the walk's current
    /// memberships, as any recompile does).
    pub base: &'a Manifest,
    /// Every node a recompile may select, in position order.
    pub renderable: &'a [&'a (u64, Stub)],
    /// The request as it stands: the cut's tokens are what it saved.
    pub est: &'a Estimate,
    pub budget: u64,
    pub rates: TokenRates,
    /// The report of the compilation it cuts: the results cleared to save
    /// context there (theseus-3ki0), which every candidate keeps cleared.
    pub carried: Option<&'a BudgetReport>,
}

/// Ring: the first cut (earliest) whose request is under 60 % of `budget`,
/// or the last; none when there is nothing to cut.
pub(super) fn ring(input: &CompileInput<'_>, r: &Ring<'_>, make: &Make<'_>) -> Option<Rung> {
    let target = r.budget * 6 / 10;
    let cleared: Vec<&BudgetDrop> = super::clearing::drops(r.carried).collect();
    let media = (
        input.blobs,
        input.hidden,
        input.overflowed.and_then(|o| o.retrying.as_deref()),
        input.sources,
    );
    // The ring leaves a summary out with its range (30c): from its first
    // kept message on, it may keep every later one.
    let seq: Vec<&Stub> = r
        .renderable
        .iter()
        .map(|(_, n)| n)
        .filter(|n| !compaction::is_summary(n))
        .collect();
    let floored = seq.len() < r.renderable.len();
    let starts: Vec<usize> = seq
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind == Kind::UserMessage)
        .map(|(i, _)| i)
        .collect();
    let try_cut = |kept: Vec<&Stub>| {
        let includes: Vec<String> = kept.iter().map(|n| n.id.clone()).collect();
        let mut candidate = make("overflow".into(), "ring", true, includes, r.base);
        candidate.budget = cleared_in(&cleared, &candidate);
        let rendered = render_request(r.recompiled, input.catalog, &candidate, input.nodes, media);
        let e = estimate(&rendered.request, r.rates, None);
        let tokens = input.overflowed.map_or(e.tokens, |o| o.scale(e.tokens));
        (candidate, rendered, e, tokens)
    };
    let rung = |(compilation, rendered, est, _): (Compilation, Rendered, Estimate, u64),
                dropped: &[&Stub]| Rung {
        cut: ring_cut(dropped, r.est.tokens.saturating_sub(est.tokens)),
        compilation,
        rendered,
        est,
    };
    let pinned = starts.len() == 1;
    for &cut in starts.iter().skip(usize::from(!floored)) {
        let c = try_cut(seq[cut..].to_vec());
        let last = cut == *starts.last().unwrap();
        if c.3 <= target || (last && !pinned) {
            return Some(rung(c, &seq[..cut]));
        }
    }
    // A single instruction: cut inside its turn, the instruction pinned.
    let &pin = starts.first()?;
    let (head, cuts) = in_turn(&seq, pin);
    let kept =
        |j: usize| -> Vec<&Stub> { seq[pin..head].iter().chain(&seq[j..]).copied().collect() };
    let dropped =
        |j: usize| -> Vec<&Stub> { seq[..pin].iter().chain(&seq[head..j]).copied().collect() };
    let (mut lo, mut hi) = (0, cuts.len());
    let mut found = None;
    while lo < hi {
        let mid = (lo + hi) / 2;
        let c = try_cut(kept(cuts[mid]));
        if c.3 <= target {
            hi = mid;
            found = Some((mid, c));
        } else {
            lo = mid + 1;
        }
    }
    match found {
        Some((k, c)) => Some(rung(c, &dropped(cuts[k]))),
        None if floored && starts.len() == 1 && cuts.is_empty() => {
            Some(rung(try_cut(seq[pin..].to_vec()), &seq[..pin]))
        }
        None => {
            let &j = cuts.last()?;
            Some(rung(try_cut(kept(j)), &dropped(j)))
        }
    }
}

/// The cut points inside a single instruction's turn (theseus-3ki0): where
/// its pinned head ends (the first answer after the message at `pin`), and
/// each later answer that leaves no call without its result: no result at
/// or after it answers a call before it. Decodes the answers and results.
fn in_turn(seq: &[&Stub], pin: usize) -> (usize, Vec<usize>) {
    let head = (pin..seq.len())
        .find(|&i| seq[i].kind == Kind::AssistantMessage)
        .unwrap_or(seq.len());
    let mut called: HashMap<&str, usize> = HashMap::new();
    // `open[j]`: how many calls before `j` have their result at or after it.
    let mut open = vec![0i64; seq.len() + 1];
    for (i, n) in seq.iter().enumerate().skip(head) {
        match (n.kind, &n.body) {
            (Kind::AssistantMessage, crate::node::Body::AssistantMessage { blocks, .. }) => {
                for b in blocks {
                    if b.get("type").and_then(serde_json::Value::as_str) == Some("tool_use") {
                        if let Some(id) = b.get("id").and_then(serde_json::Value::as_str) {
                            called.insert(id, i);
                        }
                    }
                }
            }
            (
                Kind::ToolResult,
                crate::node::Body::ToolResult {
                    tool_use_id,
                    late: false,
                    ..
                },
            ) => {
                if let Some(&c) = called.get(tool_use_id.as_str()) {
                    // Every cut in (c, i] would split the call from it.
                    open[c + 1] += 1;
                    open[i + 1] -= 1;
                }
            }
            _ => {}
        }
    }
    let mut cuts = Vec::new();
    let mut running = 0;
    for (j, n) in seq.iter().enumerate() {
        running += open[j];
        if j > head && n.kind == Kind::AssistantMessage && running == 0 {
            cuts.push(j);
        }
    }
    (head, cuts)
}

/// The cleared results a candidate keeps: those it includes; none, no
/// report.
pub(super) fn cleared_in(cleared: &[&BudgetDrop], c: &Compilation) -> Option<BudgetReport> {
    let ids: std::collections::HashSet<&str> = c.includes.iter().map(String::as_str).collect();
    let dropped: Vec<BudgetDrop> = cleared
        .iter()
        .filter(|d| d.node_id.as_deref().is_some_and(|id| ids.contains(id)))
        .map(|d| (*d).clone())
        .collect();
    (!dropped.is_empty()).then(|| BudgetReport {
        dropped,
        ..Default::default()
    })
}

/// The ring's cut (M6 30b, §2.11): the leading nodes it left out, as a range,
/// and the tokens the estimate says that saved.
pub(super) fn ring_cut(dropped: &[&Stub], tokens: u64) -> BudgetDrop {
    BudgetDrop {
        node_id: None,
        range: dropped
            .first()
            .zip(dropped.last())
            .map(|(a, b)| BudgetRange {
                first: a.id.clone(),
                last: b.id.clone(),
                nodes: dropped.len() as u64,
            }),
        reason: "overflow".into(),
        tokens,
        tier: "ring".into(),
    }
}
