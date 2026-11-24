//! A result that copies stored words into its session, and its edges (P0's rule 3). A tool that shows old
//! messages' words names their nodes in its result's meta under [`KEY`]; the completion frame takes them out
//! ([`copied`]) before the result node is built, so they are never stored, and writes one `derived_from` edge
//! from the result to each ([`edges`]), by the tool's own route: `node.reach` of a message then finds every
//! result that showed its words, and a retraction tells the conversation that read it, as a glide's borrowed
//! node was found from each message it took. Only the tools [`COPIERS`] names are read, so no other tool's
//! meta can write an edge. A window lays such a result with what has been said about its sources since
//! ([`since`]).

use anyhow::Result;
use serde_json::Value;
use theseus_store::NewRecord;

use crate::graph::{Edge, EdgeKind, VIA_HISTORY, VIA_LOOK};
use crate::node::{Body, Node};

pub mod since;

/// The meta key the sources ride under, from the tool to its completion's frame.
pub const KEY: &str = "copies";

/// The tools whose results copy stored words, each with its route.
const COPIERS: [(&str, &str); 2] = [
    (crate::view::history::HISTORY_TOOL, VIA_HISTORY),
    (crate::look::WORK, VIA_LOOK),
];

/// Whether `tool`'s results copy stored words: recall never serves one as its own words.
pub fn copier(tool: &str) -> bool {
    route(tool).is_some()
}

/// The route of `tool`'s copy edges; none for a tool whose results copy nothing.
pub fn route(tool: &str) -> Option<&'static str> {
    COPIERS
        .iter()
        .find(|(t, _)| *t == tool)
        .map(|(_, via)| *via)
}

/// Whether the stored, kept text of the look back's result `result` shows `source` to its reader: whether a
/// line the runtime kept of it names the handle `source` is read under. Its edges are a conservative superset
/// (a runtime cut keeps every link the tool gave, even to words it cut); this tells apart the sources whose
/// words reached the reader, and fails toward no:
/// - a page's line ends ` · <handle>`: its exchange's turn (`history_tool/page.rs`, `render`), or an imported
///   message's session (`history_tool/imports.rs`, `render`); a line the cut took, or split before its
///   handle, names none;
/// - an opened exchange or session names its handle once, in its heading (`history_tool/open.rs`, `render`;
///   `history_tool/session.rs`, `open`): that counts only in a result the runtime did not cut, since a cut
///   keeps the heading and may drop any part under it;
/// - `look.work` names no handle (`look/work/history.rs`, `history`): never.
pub fn shows(result: &Node, source: &Node) -> bool {
    let Body::ToolResult {
        tool,
        content,
        truncated,
        ..
    } = &result.body
    else {
        return false;
    };
    if route(tool) != Some(VIA_HISTORY) {
        return false;
    }
    let turn = source.turn_id.as_deref().filter(|t| !t.is_empty());
    let session = Some(source.session_id.as_str()).filter(|s| !s.is_empty());
    let ends = |l: &str, h: &str| {
        l.starts_with("- ") && l.strip_suffix(h).is_some_and(|l| l.ends_with(" \u{b7} "))
    };
    let heads =
        |l: &str, form: &str, h: &str| !*truncated && l.starts_with(&format!("[{form} \"{h}\","));
    content.lines().any(|l| {
        turn.is_some_and(|t| ends(l, t) || heads(l, "Exchange", t))
            || session.is_some_and(|s| ends(l, s) || heads(l, "Session", s))
    })
}

/// What one result copies: its route, and the nodes whose words it shows.
#[derive(Debug, Default, PartialEq)]
pub struct Copied {
    via: &'static str,
    sources: Vec<String>,
}

/// What `tool`'s result copies, taken out of its `meta`: none, and `meta` as it was, for any tool
/// [`COPIERS`] does not name.
pub fn copied(tool: &str, meta: &mut Value) -> Copied {
    let Some((_, via)) = COPIERS.iter().find(|(t, _)| *t == tool) else {
        return Copied::default();
    };
    let ids = meta.as_object_mut().and_then(|m| m.remove(KEY));
    Copied {
        via,
        sources: serde_json::from_value(ids.unwrap_or_default()).unwrap_or_default(),
    }
}

/// The result `node`'s edges: one from it to each distinct source.
pub fn edges(node: &str, c: &Copied) -> Result<Vec<NewRecord>> {
    let mut seen = std::collections::HashSet::new();
    (c.sources.iter())
        .filter(|s| seen.insert(s.as_str()))
        .map(|s| Edge::new(EdgeKind::DerivedFrom, node, s, c.via).record())
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Another tool's meta spelled `copies` is not read: nothing is taken out of it and no edge is written.
    #[test]
    fn another_tools_meta_spelled_copies_writes_no_edge() {
        let mut meta = json!({"copies": ["msg_1"], "path": "a.rs"});
        let c = copied("fs.read", &mut meta);
        assert_eq!(c, Copied::default());
        assert_eq!(meta, json!({"copies": ["msg_1"], "path": "a.rs"}));
        assert!(edges("trs_1", &c).unwrap().is_empty());
    }

    /// What the kept text of a history result names: a page's line by its exchange's handle, at the line's
    /// end; an imported message's line by its session; an open's heading only in a result the runtime did
    /// not cut. A handle anywhere else, a look's result and another tool's result show nothing.
    #[test]
    fn a_result_shows_a_source_only_where_its_kept_text_names_the_handle() {
        let mut source = crate::node::Node::user("ses_a", Some("turn_a"), "cli", "words");
        let result = |tool: &str, text: &str, truncated: bool| {
            let mut n = crate::compiler::tests_view::result("turn_r", "h1", text);
            if let Body::ToolResult {
                tool: t,
                truncated: c,
                ..
            } = &mut n.body
            {
                *t = tool.into();
                *c = truncated;
            }
            n
        };
        let history = crate::view::history::HISTORY_TOOL;
        let line = "context.history: 1 exchange\n- 2026-03-03 \u{b7} cli \u{b7} the owner: \u{ab}words\u{bb} \u{2192} you did \u{b7} turn_a";
        let heading =
            "[Exchange \"turn_a\", in \"cli\", 2026-03-03: what was said then]\n\u{2502} words";
        let session = "[Session \"ses_a\", imported, its messages]\n\u{2502} words";
        assert!(shows(&result(history, line, false), &source));
        assert!(shows(&result(history, line, true), &source), "a kept line");
        assert!(shows(&result(history, heading, false), &source));
        assert!(
            !shows(&result(history, heading, true), &source),
            "a cut open"
        );
        assert!(shows(&result(history, session, false), &source));
        assert!(!shows(&result(history, session, true), &source));
        let elsewhere =
            "More before these: call again with before {\"turn_id\":\"turn_a\"} \u{b7} turn_a.";
        assert!(!shows(&result(history, elsewhere, false), &source));
        assert!(!shows(&result(crate::look::WORK, line, false), &source));
        assert!(!shows(&result("fs.read", line, false), &source));
        source.turn_id = Some("turn_b".into());
        assert!(!shows(&result(history, line, false), &source));
        let longer = line.replace("turn_a", "turn_ab");
        source.turn_id = Some("turn_a".into());
        assert!(!shows(&result(history, &longer, false), &source));
    }

    /// A copier's sources are taken out of its meta, and each distinct one is one edge by its route.
    #[test]
    fn a_copiers_sources_are_taken_out_and_each_is_one_edge() {
        let mut meta = json!({"copies": ["msg_1", "msg_2", "msg_1"], "look": "look.work"});
        let c = copied(crate::look::WORK, &mut meta);
        assert_eq!(meta, json!({ "look": "look.work" }));
        let e = edges("trs_1", &c).unwrap();
        assert_eq!(e.len(), 2);
        for r in &e {
            let edge: Edge = serde_json::from_slice(&r.payload).unwrap();
            assert_eq!((edge.from.as_str(), edge.via.as_str()), ("trs_1", VIA_LOOK));
        }
    }
}
