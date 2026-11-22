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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::tests_parts::{answer, spec};
    use super::super::*;
    use crate::stub::Stub;

    /// A summary as compaction wrote it, in the layout of a store from before the cut.
    const OLD_SUMMARY: &str = r#"{"id":"sum_00000000000000000000000000000003","schema":1,"session_id":"s","turn_id":null,"loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000003,"body":{"kind":"summary","first":1,"last":2,"nodes":2,"text":"The keeper asked when high water was; it was at noon.","profile":"glm","model":"glm-5.3-flash","cost_usd":0.001,"header":"[Summary of 2 earlier messages, 2026-09-20 (@1 to @2), written by glm on glm-5.3-flash]"}}"#;

    #[test]
    fn an_old_sessions_summary_still_stands_for_its_range_and_renders_first() {
        let summary: Node = serde_json::from_str(OLD_SUMMARY).expect("an old summary decodes");
        let nodes: Vec<(u64, Stub)> = vec![
            (
                1,
                Node::user("s", None, "web", "when is high water?").into(),
            ),
            (
                2,
                answer(vec![json!({"type": "text", "text": "At noon."})]).into(),
            ),
            (3, summary.into()),
            (4, Node::user("s", None, "web", "and low water?").into()),
        ];
        let spec = spec();
        let c = compile(CompileInput {
            session_id: "s",
            current: None,
            nodes: &nodes,
            last_position: 4,
            spec: &spec,
            catalog: &crate::catalog::Catalog::builtin(),
            force: None,
            window_override: None,
            blobs: None,
            hidden: &[],
            strip: None,
            overflowed: None,
            sources: &Default::default(),
            assembled: None,
            situation: &situation::Situation::ConversationStart,
        });
        let first = c.request.messages[0]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert!(
            first.starts_with("[Summary of 2 earlier messages"),
            "{first}"
        );
        assert!(first.contains("it was at noon."), "{first}");
        let all = serde_json::to_string(&c.request.messages).unwrap();
        assert!(
            !all.contains("when is high water?"),
            "its range stays out: {all}"
        );
        assert!(all.contains("and low water?"), "{all}");
    }

    fn user(text: &str) -> Stub {
        Node::user("ses_1", Some("turn_1"), "cli", text).into()
    }

    fn summary(first: u64, last: u64) -> Stub {
        Stub::from(Node::summary(
            "ses_1",
            "turn_9",
            crate::node::Body::Summary {
                first,
                last,
                nodes: 2,
                text: "the gist".into(),
                profile: "glm".into(),
                model: "glm-5.3-flash".into(),
                cost_usd: None,
                header: format!("[Summary of 2 earlier messages, 2025-09-20 (@{first} to @{last}), written by glm]"),
            },
        ))
    }

    /// The latest summary stands for its range: a compile selects it,
    /// then what came after its range, never an older summary.
    #[test]
    fn the_latest_summary_is_the_floor() {
        use super::{is_summary, summaries_first, visible};
        let nodes: Vec<(u64, Stub)> = vec![
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
        let none: Vec<(u64, Stub)> = vec![(1, user("a")), (2, user("b"))];
        assert_eq!(visible(&none).len(), 2);
        // In the prefix, a summary renders first whatever its position.
        let three = || nodes[4..7].iter().map(|(p, n)| (*p, &**n)).collect();
        let ordered = summaries_first(three(), None);
        assert!(is_summary(ordered[0]));
        // An assembled prefix's recall section comes before it.
        let section = nodes[6].1.id.as_str();
        let ordered = summaries_first(three(), Some(section));
        assert_eq!(ordered[0].id, section);
        assert!(is_summary(ordered[1]));
    }
}
