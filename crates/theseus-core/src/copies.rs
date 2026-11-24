//! A result that copies stored words into its session, and its edges (P0's rule 3). A tool that shows old
//! messages' words names their nodes in its result's meta under [`KEY`]; the completion frame takes them out
//! ([`copied`]) before the result node is built, so they are never stored, and writes one `derived_from` edge
//! from the result to each ([`edges`]), by the tool's own route: `node.reach` of a message then finds every
//! result that showed its words, and a retraction tells the conversation that read it, as a glide's borrowed
//! node was found from each message it took. Only the tools [`COPIERS`] names are read, so no other tool's meta can write
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
