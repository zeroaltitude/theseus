//! A result that copies stored words into its session, and its edges (P0's rule 3). A tool that shows old
//! messages' words names their nodes in its result's meta under [`KEY`] (each with where its words are in the
//! tool's text, when the tool says: [`span`]); the completion frame takes them out
//! ([`copied`]) before the result node is built, so they are never stored, and writes one `derived_from` edge
//! from the result to each ([`edges`]), by the tool's own route: `node.reach` of a message then finds every
//! result that showed its words, and a retraction lists it ("a copy by history"), as a glide's borrowed node was
//! found from each message it took. Only the tools [`COPIERS`] names are read, so no other tool's meta can write
//! an edge.

use anyhow::Result;
use serde_json::Value;
use theseus_store::NewRecord;

use crate::graph::{Edge, EdgeKind, VIA_HISTORY, VIA_LOOK};

/// The meta key the sources ride under, from the tool to its completion's frame.
pub const KEY: &str = "copies";

/// The tools whose results copy stored words, each with its route.
const COPIERS: [(&str, &str); 2] = [
    (crate::view::history::HISTORY_TOOL, VIA_HISTORY),
    (crate::look::WORK, VIA_LOOK),
];

/// What one result copies: its route, and the nodes whose words it shows, each with where its words are in the
/// tool's text when the tool says (a span of characters, `from` to `to`).
#[derive(Debug, Default, PartialEq)]
pub struct Copied {
    via: &'static str,
    sources: Vec<(String, Option<(usize, usize)>)>,
}

/// One source as a tool names it: its node id, or its node id and where its words are in the tool's text
/// ([`span`]), which lets a later cut of that text drop a source none of whose words it keeps ([`Copied::kept`]).
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Entry {
    Id(String),
    Span { id: String, from: usize, to: usize },
}

/// A source with where its words are in the tool's text: characters `from` to `to`.
pub fn span(id: &str, from: usize, to: usize) -> Value {
    serde_json::json!({"id": id, "from": from, "to": to})
}

/// What `tool`'s result copies, taken out of its `meta`: none, and `meta` as it was, for any tool
/// [`COPIERS`] does not name.
pub fn copied(tool: &str, meta: &mut Value) -> Copied {
    let Some((_, via)) = COPIERS.iter().find(|(t, _)| *t == tool) else {
        return Copied::default();
    };
    let ids = meta.as_object_mut().and_then(|m| m.remove(KEY));
    let entries: Vec<Entry> = serde_json::from_value(ids.unwrap_or_default()).unwrap_or_default();
    Copied {
        via,
        sources: (entries.into_iter())
            .map(|e| match e {
                Entry::Id(id) => (id, None),
                Entry::Span { id, from, to } => (id, Some((from, to))),
            })
            .collect(),
    }
}

impl Copied {
    /// The sources a cut of the tool's text leaves: each whose span `keeps` (some of its words survive), and
    /// each the tool gave no span for.
    pub fn kept(mut self, keeps: impl Fn(usize, usize) -> bool) -> Copied {
        (self.sources).retain(|(_, span)| span.is_none_or(|(from, to)| keeps(from, to)));
        self
    }
}

/// The result `node`'s edges: one from it to each distinct source.
pub fn edges(node: &str, c: &Copied) -> Result<Vec<NewRecord>> {
    let mut seen = std::collections::HashSet::new();
    (c.sources.iter())
        .map(|(s, _)| s)
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

    /// A source with its span is kept when a cut leaves some of its words, and dropped when it leaves none; a
    /// source with no span is kept (D6 round 6).
    #[test]
    fn a_cut_keeps_a_source_whose_words_it_keeps_and_no_other() {
        let mut meta = json!({"copies": [
            span("msg_1", 0, 5), span("msg_2", 10, 20), "msg_3", span("msg_4", 6, 12),
        ]});
        let c = copied(crate::look::WORK, &mut meta).kept(|from, to| from < 8 || to > 18);
        let to: Vec<String> = (edges("trs_1", &c).unwrap().iter())
            .map(|r| serde_json::from_slice::<Edge>(&r.payload).unwrap().to)
            .collect();
        assert_eq!(to, ["msg_1", "msg_2", "msg_3", "msg_4"].map(String::from));
        let c = Copied {
            via: VIA_LOOK,
            sources: vec![("msg_5".into(), Some((9, 15)))],
        }
        .kept(|from, to| from < 8 || to > 18);
        assert!(
            edges("trs_1", &c).unwrap().is_empty(),
            "no word of it is kept"
        );
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
