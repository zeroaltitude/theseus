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

use theseus_protocol::memory::{BudgetDrop, BudgetRange};

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
}

/// Ring: the first cut (earliest) whose request is under 60 % of `budget`,
/// or the last; none when there is nothing to cut.
pub(super) fn ring(input: &CompileInput<'_>, r: &Ring<'_>, make: &Make<'_>) -> Option<Rung> {
    let target = r.budget * 6 / 10;
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
    for &cut in starts.iter().skip(usize::from(!floored)) {
        let includes: Vec<String> = seq[cut..].iter().map(|n| n.id.clone()).collect();
        let candidate = make("overflow".into(), "ring", true, includes, r.base);
        let rendered = render_request(r.recompiled, input.catalog, &candidate, input.nodes, media);
        let e = estimate(&rendered.request, r.rates, None);
        let last = cut == *starts.last().unwrap();
        let tokens = input.overflowed.map_or(e.tokens, |o| o.scale(e.tokens));
        if tokens <= target || last {
            return Some(Rung {
                cut: ring_cut(&seq[..cut], r.est.tokens.saturating_sub(e.tokens)),
                compilation: candidate,
                rendered,
                est: e,
            });
        }
    }
    None
}

/// The ring's cut (M6 30b, §2.11): the leading nodes it left out, as a range,
/// and the tokens the estimate says that saved.
fn ring_cut(dropped: &[&Stub], tokens: u64) -> BudgetDrop {
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
