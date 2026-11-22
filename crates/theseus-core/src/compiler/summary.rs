//! An old session's `Summary` nodes (M6 step 30c, retired): compaction, which wrote them where the ring
//! would drop a session's leading turns, is gone, and nothing writes a summary now. A store still holds
//! them, so they are still read: a summary an old exchange holds renders first in its request's prefix, as
//! testimony under its frozen header (in a tail it renders nothing).

use crate::node::Node;
use crate::stub::{Kind, Shaped};

/// What a summary renders: its header, then its text.
pub fn rendered(header: &str, text: &str) -> String {
    format!("{header}\n{text}")
}

pub(crate) fn is_summary(n: &(impl Shaped + ?Sized)) -> bool {
    n.kind() == Kind::Summary
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
    use super::super::view::{self, Frozen};
    use super::super::*;
    use crate::stub::Stub;

    /// A summary as compaction wrote it, in the layout of a store from before the cut.
    const OLD_SUMMARY: &str = r#"{"id":"sum_00000000000000000000000000000003","schema":1,"session_id":"s","turn_id":null,"loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000003,"body":{"kind":"summary","first":1,"last":2,"nodes":2,"text":"The keeper asked when high water was; it was at noon.","profile":"glm","model":"glm-5.3-flash","cost_usd":0.001,"header":"[Summary of 2 earlier messages, 2026-09-20 (@1 to @2), written by glm on glm-5.3-flash]"}}"#;

    /// An old summary an exchange in the view's window holds still decodes and renders as its testimony,
    /// under its frozen header, first in the request, before the turn's new message.
    #[test]
    fn an_old_exchanges_summary_renders_under_its_header_in_the_window() {
        let summary: Node = serde_json::from_str(OLD_SUMMARY).expect("an old summary decodes");
        let window: Vec<(u64, Stub)> = vec![
            (1, summary.into()),
            (
                2,
                answer(vec![json!({"type": "text", "text": "At noon."})]).into(),
            ),
        ];
        let turn = "turn_now";
        let own: Vec<(u64, Stub)> = vec![(
            3,
            Node::user("s", Some(turn), "web", "and low water?").into(),
        )];
        let frozen = Frozen {
            turn_id: turn.into(),
            window,
            ..Default::default()
        };
        let spec = spec();
        let c = view::compile(
            CompileInput {
                session_id: "s",
                current: None,
                nodes: &own,
                last_position: 3,
                spec: &spec,
                catalog: &crate::catalog::Catalog::builtin(),
                force: false,
                window_override: None,
                blobs: None,
                hidden: &[],
                strip: None,
                overflowed: None,
                sources: &Default::default(),
                assembled: None,
                situation: &situation::Situation::Continuation,
            },
            &frozen,
        );
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
            all.contains("At noon."),
            "the exchange's answer follows: {all}"
        );
        assert!(all.contains("and low water?"), "{all}");
    }
}
