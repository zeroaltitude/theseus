//! An old session's `Summary` nodes (M6 step 30c, retired): compaction, which wrote them where the ring
//! would drop a session's leading turns, is gone, and nothing writes a summary now. A store still holds
//! them, so they are still read:
//! - **The floor.** The session's latest summary stands for its range: a transcript compile selects it,
//!   then the renderable nodes after its range, and never the nodes it summarized.
//! - **First in the prefix.** A summary renders first in its compilation's prefix, whatever its position,
//!   as testimony under its frozen header. In a tail it renders nothing.

use super::renderable;
use crate::node::Node;
use crate::stub::{Kind, Shaped, Stub};

/// The strategy of an old compaction's compilation (one may still be a session's current one).
pub const STRATEGY: &str = "compaction";

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

/// The positions the session's latest summary stands for, first to last:
/// its body's range (it decodes that one node). A node before `first` that
/// it does not stand for, the instruction a summary inside a turn pins
/// (theseus-3ki0), stays visible.
pub(crate) fn hidden(nodes: &[(u64, Stub)]) -> Option<(u64, u64)> {
    let ((_, n), last) = floor(nodes)?;
    let first = match &n.body {
        crate::node::Body::Summary { first, .. } => *first,
        _ => 0,
    };
    Some((first, last))
}

/// Whether the latest summary (`hidden`) leaves the node at `p` visible:
/// after its range, or a message before it (a pinned instruction; never a
/// recall note, which a range before it never held).
pub(crate) fn outside(hidden: Option<(u64, u64)>, p: u64, n: &Stub) -> bool {
    match hidden {
        None => true,
        Some((first, last)) => p > last || (p < first && n.kind != Kind::Recall),
    }
}

/// The nodes a recompile selects from, in position order: the latest
/// summary (rendered first all the same), and every renderable node it
/// does not stand for but other summaries. With no summary, every
/// renderable node.
pub(super) fn visible(nodes: &[(u64, Stub)]) -> Vec<&(u64, Stub)> {
    let id = floor(nodes).map(|(e, _)| e.1.id.as_str());
    let hidden = hidden(nodes);
    nodes
        .iter()
        .filter(|(p, n)| {
            renderable(n)
                && if is_summary(n) {
                    Some(n.id.as_str()) == id
                } else {
                    outside(hidden, *p, n)
                }
        })
        .collect()
}

/// A prefix's nodes in the assembled order (30c): its recall section
/// (`recall`, the compilation's `recall_id`), what precedes its earliest
/// summary's range (an instruction a summary inside a turn pins,
/// theseus-3ki0), its summaries in the order they were written, and the
/// rest in position order.
pub(super) fn summaries_first<'n>(
    prefix: Vec<(u64, &'n Node)>,
    recall: Option<&str>,
) -> Vec<&'n Node> {
    let (first, rest): (Vec<_>, Vec<_>) = prefix
        .into_iter()
        .partition(|(_, n)| Some(n.id.as_str()) == recall);
    let (summaries, rest): (Vec<_>, Vec<_>) = rest.into_iter().partition(|(_, n)| is_summary(n));
    let from = summaries
        .iter()
        .filter_map(|(_, n)| match &n.body {
            crate::node::Body::Summary { first, .. } => Some(*first),
            _ => None,
        })
        .min();
    let (pinned, rest): (Vec<_>, Vec<_>) = rest
        .into_iter()
        .partition(|(p, _)| from.is_some_and(|f| *p < f));
    [first, pinned, summaries, rest]
        .into_iter()
        .flatten()
        .map(|(_, n)| n)
        .collect()
}
