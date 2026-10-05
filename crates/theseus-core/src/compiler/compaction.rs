//! Compaction roots (M6 step 30c, §2.5): where the ring would drop a
//! session's leading turns, a cheap profile summarizes them into a `Summary`
//! node, and the new compilation (`strategy: compaction`) is that summary,
//! then the turns the ring kept, with thinking stripped as the ring strips it.
//!
//! The compiler's half, pure as the rest of it:
//! - **The floor.** The session's latest summary stands for its range at
//!   every compile: a recompile selects it, then the renderable nodes after
//!   its range, and never the nodes it summarized. The ring rings over those
//!   later nodes, and leaves the summary out: the ring is the fallback, with
//!   nothing of compaction in it.
//! - **First in the prefix.** A summary renders first in its compilation's
//!   prefix, whatever its position (it is written after the range it
//!   summarizes, and after the turn's new message), as testimony under its
//!   frozen header. In a tail it renders nothing: a compilation older than
//!   it still carries its range.
//! - **The compaction's compilation** (`compact`): the summary, then the
//!   ring's kept turns, as of the summary's position, so the next request
//!   appends to its bytes.

use std::borrow::Cow;

use theseus_protocol::memory::BudgetDrop;

use super::{
    budget_report, cache_layout, counted_part, estimate, render_request, renderable, CompileInput,
    Compiled, RequestSpec,
};
use crate::catalog::TokenRates;
use crate::node::Node;
use crate::stub::{Kind, Shaped, Stub};

/// The strategy of a compaction's compilation.
pub const STRATEGY: &str = "compaction";

/// A summary's testimony header (§2.11): `[Summary of 212 earlier messages,
/// 2026-09-20 to 2026-09-27, written by glm]`, the dates UTC.
pub fn header(messages: u32, first_ms: u64, last_ms: u64, profile: &str) -> String {
    let (a, b) = (day(first_ms), day(last_ms));
    let when = if a == b { a } else { format!("{a} to {b}") };
    let what = if messages == 1 { "message" } else { "messages" };
    format!("[Summary of {messages} earlier {what}, {when}, written by {profile}]")
}

fn day(ms: u64) -> String {
    let (y, m, d) = crate::wake::civil_from_days((ms / 1000 / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// What a summary renders: its header, then its text.
pub fn rendered(header: &str, text: &str) -> String {
    format!("{header}\n{text}")
}

pub(crate) fn is_summary(n: &(impl Shaped + ?Sized)) -> bool {
    n.kind() == Kind::Summary
}

/// The session's latest summary, with its position, and the position of
/// the last node of its range: read from the stubs, so it decodes nothing.
pub fn floor(nodes: &[(u64, Stub)]) -> Option<(&(u64, Stub), u64)> {
    nodes
        .iter()
        .rev()
        .find_map(|e| e.1.summary_last.map(|last| (e, last)))
}

/// The nodes a recompile selects from, in position order: the latest
/// summary (rendered first all the same), and every renderable node after
/// its range but other summaries. With no summary, every renderable node.
pub(super) fn visible(nodes: &[(u64, Stub)]) -> Vec<&(u64, Stub)> {
    let floor = floor(nodes);
    let after = floor.map_or(0, |(_, last)| last);
    let id = floor.map(|(e, _)| e.1.id.as_str());
    nodes
        .iter()
        .filter(|(p, n)| {
            renderable(n)
                && if is_summary(n) {
                    Some(n.id.as_str()) == id
                } else {
                    floor.is_none() || *p > after
                }
        })
        .collect()
}

/// A prefix's nodes in the assembled order (30c): its recall section
/// (`recall`, the compilation's `recall_id`), its summaries in the order
/// they were written, and the rest in position order.
pub(super) fn summaries_first<'n>(prefix: Vec<&'n Node>, recall: Option<&str>) -> Vec<&'n Node> {
    let (mut first, rest): (Vec<&Node>, Vec<&Node>) = prefix
        .into_iter()
        .partition(|n| Some(n.id.as_str()) == recall);
    let (summaries, rest): (Vec<&Node>, Vec<&Node>) = rest.into_iter().partition(is_summary);
    first.extend(summaries);
    first.extend(rest);
    first
}

/// The compaction's compilation: `summary` (written, so among `input.nodes`
/// at or before `input.last_position`), then the turns `ring` kept, as of
/// the summary's frame. `drops` is what it left out: its range, summarized,
/// and the recall notes in it, dropped (tier `compaction`). The caller
/// checks it fits (`fits`).
pub fn compact(
    input: CompileInput<'_>,
    ring: &Compiled,
    summary: &str,
    drops: Vec<BudgetDrop>,
) -> Compiled {
    let spec: Cow<'_, RequestSpec> = match input.spec.walk.as_deref() {
        Some(walk) => crate::ontology::guided(input.spec, walk.compose(&walk.current)),
        None => Cow::Borrowed(input.spec),
    };
    let mut c = ring.compilation.clone();
    c.id = crate::new_id("cmp");
    c.strategy = STRATEGY.into();
    c.as_of = input.last_position;
    c.includes = std::iter::once(summary.to_string())
        .chain(c.includes.iter().filter(|id| *id != summary).cloned())
        .collect();
    c.manifest.strip_thinking = true;
    c.recall_id = input.assembled.map(str::to_string);
    let media = (
        input.blobs,
        input.hidden,
        input.overflowed.and_then(|o| o.retrying.as_deref()),
        input.sources,
    );
    let rendered = render_request(&spec, input.catalog, &c, input.nodes, media);
    let entry = input.catalog.get(&spec.model);
    let rates = entry.map_or_else(|| TokenRates::of(&spec.model), |e| e.bytes_per_token);
    let counted = counted_part(input.nodes, &c, &rendered.request);
    let mut est = estimate(&rendered.request, rates, counted);
    let request = rendered.request;
    est.bytes = request.json_bytes();
    let mut budget = budget_report(
        (ring.budget.limit_tokens > 0).then_some(ring.budget.limit_tokens),
        &est,
        None,
    );
    budget.dropped = drops;
    c.budget = Some(budget.clone());
    Compiled {
        budget,
        messages: request.messages.len(),
        digest: request.digest(),
        compilation: c,
        new_compilation: true,
        trigger: ring.trigger.clone(),
        prefix_nodes: rendered.prefix_nodes,
        tail_nodes: rendered.tail_nodes,
        est_tokens: est.tokens,
        estimate: est,
        repairs: rendered.repairs,
        cache: cache_layout(&spec, input.catalog),
        withheld: ring.withheld,
        signals: ring.signals.clone(),
        request,
    }
}

/// Whether a compiled request fits its limit: no overage, or no window to
/// measure it by.
pub fn fits(c: &Compiled) -> bool {
    c.budget.overage.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::Body;

    fn user(text: &str) -> Stub {
        Node::user("ses_1", Some("turn_1"), "cli", text).into()
    }

    fn summary(first: u64, last: u64) -> Stub {
        Stub::from(Node::summary(
            "ses_1",
            "turn_9",
            Body::Summary {
                first,
                last,
                nodes: 2,
                text: "the gist".into(),
                profile: "glm".into(),
                model: "glm-5.3-flash".into(),
                cost_usd: None,
                header: header(2, 0, 0, "glm"),
            },
        ))
    }

    #[test]
    fn the_header_says_how_many_when_and_by_whom() {
        let day = 86_400_000;
        assert_eq!(
            header(212, 20_351 * day, 20_358 * day + 5, "glm"),
            "[Summary of 212 earlier messages, 2025-09-20 to 2025-09-27, written by glm]"
        );
        assert_eq!(
            header(1, 20_351 * day, 20_351 * day + 9, "glm"),
            "[Summary of 1 earlier message, 2025-09-20, written by glm]"
        );
    }

    /// The latest summary stands for its range: a recompile selects it,
    /// then what came after its range, never an older summary.
    #[test]
    fn the_latest_summary_is_the_floor() {
        let nodes: Vec<(u64, crate::stub::Stub)> = vec![
            (1, user("a")),
            (2, user("b")),
            (3, user("c")),
            (4, summary(1, 2)),
            (5, user("d")),
            (6, summary(1, 3)),
            (7, user("e")),
        ];
        let seen: Vec<u64> = visible(&nodes).iter().map(|(p, _)| *p).collect();
        assert_eq!(seen, vec![5, 6, 7]);
        let none: Vec<(u64, crate::stub::Stub)> = vec![(1, user("a")), (2, user("b"))];
        assert_eq!(visible(&none).len(), 2);
        // In the prefix, a summary renders first whatever its position.
        let ordered = summaries_first(vec![&*nodes[4].1, &*nodes[5].1, &*nodes[6].1], None);
        assert!(is_summary(ordered[0]));
        // An assembled prefix's recall section comes before it.
        let section = nodes[6].1.id.as_str();
        let ordered = summaries_first(
            vec![&*nodes[4].1, &*nodes[5].1, &*nodes[6].1],
            Some(section),
        );
        assert_eq!(ordered[0].id, section);
        assert!(is_summary(ordered[1]));
    }
}
