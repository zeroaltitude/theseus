//! A look back's result kept by reference (issue 49): `context.history` and `look.work` give their answer's parts
//! in their meta (`history_tool::written`); the runtime takes them out, as it takes the messages they copy
//! (`copies::copied`), caps them where it would cap their text (`quotes::cap`), and writes the result with the
//! parts and no words of its own (`content` empty). The node it holds in memory has the parts' rendering as its
//! content, which is what the call's announcement and the content judge read, as they read every result's.

use serde_json::Value;
use theseus_store::NewRecord;
use theseus_tools::REST_NARROWER;

use super::{kept, ResultNode, ToolRuntime};
use crate::history_tool::written::PARTS;
use crate::node::{Body, Node};
use crate::quotes::{self, Part};
use crate::store::Store;

/// A result's parts, and the store its quotes are read from.
pub(super) struct ByRef<'a> {
    pub(super) parts: Vec<Part>,
    pub(super) store: &'a Store,
}

/// The parts a look back's call gave in its meta, taken out of it; none for any other tool, whose meta is never
/// read for them, and none when they do not read as parts: its words are then kept, as before.
pub(super) fn taken(tool: &str, meta: &mut Value) -> Vec<Part> {
    if !crate::copies::copier(tool) {
        return Vec::new();
    }
    let parts = meta.as_object_mut().and_then(|m| m.remove(PARTS));
    parts
        .and_then(|p| serde_json::from_value(p).ok())
        .unwrap_or_default()
}

/// `node`'s record as it is stored: a result kept by reference holds its parts, and none of the words they show.
pub(super) fn record(node: &Node) -> anyhow::Result<NewRecord> {
    match &node.body {
        Body::ToolResult { parts, content, .. } if !parts.is_empty() && !content.is_empty() => {
            let mut stored = node.clone();
            if let Body::ToolResult { content, .. } = &mut stored.body {
                content.clear();
            }
            stored.record()
        }
        _ => node.record(),
    }
}

impl ToolRuntime {
    /// `r`'s parts (its fit note before them, `fit_noted`) capped as its text would be, the runtime's words after
    /// them (its hint, and `group`, the line of a call that ran in a group); and what they show, and whether the
    /// cap cut. None when `r` is not kept by reference: it gave no parts, or its tool caps by a rule of its own or
    /// keeps its whole output, whose cut lines name its words and its file; or a quote, capped, would begin or end
    /// inside a known value (`quotes::edges_hold`).
    pub(super) fn capped_parts(
        &self,
        r: &ResultNode<'_>,
        group: Option<&str>,
    ) -> Option<(Vec<Part>, kept::Capped)> {
        let by_ref = r.by_ref.as_ref()?;
        let tool = self.tool(r.tool);
        if r.kept.is_some()
            || tool
                .as_ref()
                .is_some_and(|t| t.result_max_chars().is_some())
        {
            return None;
        }
        let rest = |left: &str| {
            tool.as_ref()
                .map_or_else(|| REST_NARROWER.into(), |t| t.rest(left))
        };
        let cuts = self.scrubber.cuts();
        let (parts, truncated) = quotes::cap::cap_parts(
            by_ref.store,
            by_ref.parts.clone(),
            self.result_max_chars,
            rest,
            &cuts,
        );
        // A range that begins or ends inside a known value is never stored: the result keeps its words, capped
        // as any result's are.
        if !quotes::edges_hold(by_ref.store, &parts, &cuts) {
            return None;
        }
        let parts = quotes::cap::ended(parts, r.hint.as_deref(), group);
        let content = quotes::render_with(&parts, |q| quotes::read(by_ref.store, q));
        let capped = kept::Capped {
            content,
            truncated,
            ..kept::Capped::default()
        };
        Some((parts, capped))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Only a look back's meta is read for parts: another tool's meta spelled `parts` keeps them and gives none.
    #[test]
    fn another_tools_meta_spelled_parts_is_not_read() {
        let mut meta = json!({"parts": [{"code": "x"}], "path": "a.rs"});
        assert!(taken("fs.read", &mut meta).is_empty());
        assert_eq!(meta["parts"], json!([{"code": "x"}]));
        let mut meta = json!({"parts": [{"code": "x"}], "look": "work"});
        assert_eq!(
            taken(crate::look::WORK, &mut meta),
            vec![Part::Code("x".into())]
        );
        assert_eq!(meta, json!({"look": "work"}), "taken out of what is stored");
    }
}
