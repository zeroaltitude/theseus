//! A thread's cold resume (theseus-h4a7, thread-context): when a thread's next
//! message arrives after its cache window has passed (`stubs::cold`), the
//! provider has dropped its prefix and the request writes the whole prompt
//! again at the write price, so the rewrite is the moment to make it small.
//! The compile is a recompile (trigger `cold_resume`): the thread's fresh card
//! (`card`), the compaction summary, and the last `[threads] resume_keep_turns`
//! turns; a result over `clearing::MIN_CHARS` before the newest turn is a
//! stub that says how to read it again, and a turn is dropped from the front
//! only when that is not enough. Never above `[threads] working_tokens`, but
//! for the newest turn alone. A ring made of it is summarized as any ring is
//! (`turn::compaction`).
//!
//! A warm thread never comes here: the cold check is the compile step's, a
//! comparison, and a warm thread is recompiled for size only by the ring and
//! compaction, which guard the window as they always have.

use theseus_protocol::memory::{BudgetDrop, BudgetReport};

use super::ring::{cleared_in, ring_cut, Make, Ring, Rung};
use super::{clearing, counted_part, estimate, render_request, Compilation, CompileInput};
use crate::node::Body;
use crate::stub::{Kind, Stub};

/// A cold resume's bounds: from `[threads]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cold {
    /// The newest turns a resume keeps whole.
    pub keep_turns: u32,
    /// The tokens a resume stays within.
    pub working_tokens: u64,
}

/// The smallest rewrite of the request in `r` that fits `cold`: turns are
/// kept from the earliest cut `keep_turns` allows, its old big results
/// stubbed, then later cuts and the same stubs; the last cut is taken when
/// none fits. None when there is no turn to cut and nothing to stub.
pub(super) fn resume(
    input: &CompileInput<'_>,
    r: &Ring<'_>,
    cold: Cold,
    make: &Make<'_>,
) -> Option<Rung> {
    let seq: Vec<&Stub> = r
        .renderable
        .iter()
        .map(|(_, n)| n)
        .filter(|n| !super::compaction::is_summary(n))
        .collect();
    let starts: Vec<usize> = seq
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind == Kind::UserMessage)
        .map(|(i, _)| i)
        .collect();
    let newest = *starts.last()?;
    let carried: Vec<&BudgetDrop> = clearing::drops(r.carried).collect();
    let media = (
        input.blobs,
        input.hidden,
        input.overflowed.and_then(|o| o.retrying.as_deref()),
        input.sources,
    );
    let try_cut = |from: usize, stub: bool| {
        let kept = &seq[from..];
        let includes: Vec<String> = kept.iter().map(|n| n.id.clone()).collect();
        let strategy = if from == 0 { "transcript" } else { "ring" };
        let mut c = make("cold_resume".into(), strategy, true, includes, r.base);
        c.budget = cleared_in(&carried, &c);
        if stub {
            stubs_in(&mut c, &kept[..newest.saturating_sub(from)]);
        }
        let rendered = render_request(r.recompiled, input.catalog, &c, input.nodes, media);
        let counted = counted_part(input.nodes, &c, &rendered.request);
        let est = estimate(&rendered.request, r.rates, counted);
        (c, rendered, est)
    };
    let first = starts.len().saturating_sub(cold.keep_turns.max(1) as usize);
    let mut last = None;
    for &from in &starts[first..] {
        for stub in [false, true] {
            if stub && !any_big(&seq[from..newest]) {
                continue;
            }
            let (c, rendered, est) = try_cut(from, stub);
            let fits = est.tokens <= cold.working_tokens;
            last = Some((from, c, rendered, est));
            if fits {
                let (from, c, rendered, est) = last.take()?;
                return Some(rung(r, &seq, from, c, rendered, est));
            }
        }
    }
    let (from, c, rendered, est) = last?;
    Some(rung(r, &seq, from, c, rendered, est))
}

fn rung(
    r: &Ring<'_>,
    seq: &[&Stub],
    from: usize,
    compilation: Compilation,
    rendered: super::Rendered,
    est: super::Estimate,
) -> Rung {
    let dropped = &seq[..if compilation.strategy == "ring" {
        from
    } else {
        0
    }];
    let mut cut = ring_cut(dropped, r.est.tokens.saturating_sub(est.tokens));
    cut.reason = "cold_resume".into();
    Rung {
        compilation,
        rendered,
        est,
        cut,
    }
}

/// Whether any of `nodes` is a result worth stubbing. Reads stub fields.
fn any_big(nodes: &[&Stub]) -> bool {
    nodes.iter().any(|n| n.kind == Kind::ToolResult)
}

/// Stub the results among `nodes` longer than `MIN_CHARS`, as a clearing
/// pass would: drops of tier `cleared` in the candidate's report.
fn stubs_in(c: &mut Compilation, nodes: &[&Stub]) {
    let mut report = c.budget.take().unwrap_or_default();
    let already: std::collections::HashSet<String> = clearing::drops(Some(&report))
        .filter_map(|d| d.node_id.clone())
        .collect();
    for n in nodes.iter().filter(|n| n.kind == Kind::ToolResult) {
        let Body::ToolResult {
            content,
            late: false,
            ..
        } = &n.body
        else {
            continue;
        };
        let chars = content.chars().count();
        if chars <= clearing::MIN_CHARS || already.contains(n.id.as_str()) {
            continue;
        }
        report.dropped.push(BudgetDrop {
            node_id: Some(n.id.clone()),
            range: None,
            reason: clearing::TIER.into(),
            tokens: (content.len() / 4) as u64,
            tier: clearing::TIER.into(),
        });
    }
    c.budget = Some(BudgetReport {
        dropped: report.dropped,
        ..Default::default()
    });
}
