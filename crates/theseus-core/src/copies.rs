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

/// Whether the stored, kept text of the look back's result `result` shows `source` to its reader. Its edges
/// are a conservative superset (a runtime cut keeps every link the tool gave, even to words it cut); this tells
/// apart the sources whose words reached the reader, and fails toward no (seen) in every doubt:
/// 1. **What the result is**, by its first line, which the runtime's cut always keeps ([`Head`]): a page of
///    exchanges, a page of imported messages, an opened exchange or an opened session. Anything else (another
///    tool, `look.work`, which names no handle, an error, a note before the head) shows nothing.
/// 2. **An open** names its handle once, in its heading: that counts only when the runtime did not cut the
///    result, since a cut keeps the heading and may drop any part under it. A body line never counts, even one
///    a cut left reading as a page's line.
/// 3. **A page** shows a source by a whole line the renderer wrote that starts `- ` and ends ` · <handle>`:
///    an exchange's turn on a page of exchanges, an imported message's session on a page of imports. In a
///    result the runtime cut, a line counts only when it is whole ([`whole`]).
/// 4. **A session's handle** is shared by every message of that import session: it shows `source` only in a
///    result not cut, or when `source` is the one node of its session the result links (`alone`, asked only
///    then).
///
/// A result kept by reference (`quotes`) says exactly what it showed: it shows `source` when one of its quotes
/// names `source` and keeps a byte of it, whatever tool of [`COPIERS`] wrote it; the rules above are for a
/// result stored with its words.
pub fn shows(result: &Node, source: &Node, alone: impl FnOnce() -> bool) -> bool {
    let Body::ToolResult {
        tool,
        content,
        truncated,
        parts,
        ..
    } = &result.body
    else {
        return false;
    };
    if !parts.is_empty() {
        return copier(tool) && quotes(parts, &source.id);
    }
    if route(tool) != Some(VIA_HISTORY) {
        return false;
    }
    let turn = source.turn_id.as_deref().filter(|t| !t.is_empty());
    let session = Some(source.session_id.as_str()).filter(|s| !s.is_empty());
    let first = content.lines().next().unwrap_or_default();
    let names = |h: &str| {
        let lines = whole(content, *truncated).unwrap_or_default();
        let ends = |l: &&str| {
            l.starts_with("- ") && l.strip_suffix(h).is_some_and(|l| l.ends_with(" \u{b7} "))
        };
        lines.iter().any(ends)
    };
    match Head::of(first) {
        Some(Head::Exchange) => {
            !*truncated && turn.is_some_and(|t| first.starts_with(&format!("[Exchange \"{t}\",")))
        }
        Some(Head::Session) => {
            !*truncated && session.is_some_and(|s| first.starts_with(&format!("[Session \"{s}\",")))
        }
        Some(Head::Page) => turn.is_some_and(names),
        Some(Head::Imports) => session.is_some_and(names) && (!*truncated || alone()),
        None => false,
    }
}

/// Whether `parts` quote at least one byte of the node `id`.
fn quotes(parts: &[crate::quotes::Part], id: &str) -> bool {
    parts.iter().any(|p| match p {
        crate::quotes::Part::Quote(q) => q.node == id && q.to > q.from,
        crate::quotes::Part::Code(_) => false,
    })
}

/// What a history result is, by its first line as each renderer writes it.
#[derive(Debug, PartialEq)]
enum Head {
    /// `context.history: <n> exchange…` (`history_tool/page.rs`, `render`).
    Page,
    /// `context.history: <n> imported item…` (`history_tool/imports.rs`, `render`).
    Imports,
    /// `[Exchange "<turn>", …` (`history_tool/open.rs`, `render`).
    Exchange,
    /// `[Session "<session>", …` (`history_tool/session.rs`, `open`).
    Session,
}

impl Head {
    fn of(first: &str) -> Option<Head> {
        if first.starts_with("[Exchange \"") {
            return Some(Head::Exchange);
        }
        if first.starts_with("[Session \"") {
            return Some(Head::Session);
        }
        let rest = first.strip_prefix("context.history: ")?;
        let count = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        if count.len() == rest.len() {
            return None;
        }
        if count.starts_with(" exchange") {
            Some(Head::Page)
        } else if count.starts_with(" imported item") {
            Some(Head::Imports)
        } else {
            None
        }
    }
}

/// The lines of `content` that are whole lines its renderer wrote: every line of a result not cut. A result
/// the runtime cut (`toolrun::cap`) is its head, one line of its own between two line breaks it inserts
/// (`…[<what> not shown…]…`, the seam), and its tail; either part may begin or end inside a line, wherever the
/// cut fell. So the lines kept are those of the head but its last, and those of the tail after its first line
/// break: never a line beside the seam. A cut result whose seam is not found exactly once shows none.
fn whole(content: &str, truncated: bool) -> Option<Vec<&str>> {
    let lines: Vec<&str> = content.lines().collect();
    if !truncated {
        return Some(lines);
    }
    let seam = |l: &str| {
        l.starts_with("\u{2026}[") && l.ends_with("]\u{2026}") && l.contains(" not shown")
    };
    let mut seams = (lines.iter().enumerate()).filter(|(_, l)| seam(l));
    let (Some((at, _)), None) = (seams.next(), seams.next()) else {
        return None;
    };
    let head = &lines[..at.saturating_sub(1)];
    let tail = lines.get(at + 2..).unwrap_or_default();
    Some(head.iter().chain(tail).copied().collect())
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

    /// A result of `tool` holding `text`, cut by the runtime when `truncated`.
    fn result(tool: &str, text: &str, truncated: bool) -> Node {
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
    }

    /// Whether a history result holding `text` shows `source`, where no other linked node is asked of.
    fn shown(text: &str, truncated: bool, source: &Node) -> bool {
        let history = crate::view::history::HISTORY_TOOL;
        shows(&result(history, text, truncated), source, || unreachable!())
    }

    /// A page's head, as `history_tool/page.rs` writes it.
    const HEAD: &str = "context.history: 4 exchanges of cli's history, newest first. Each: when";

    /// The seam the runtime's cap leaves in a cut result.
    const SEAM: &str =
        "\u{2026}[2 lines (90 characters) not shown: a narrower call returns them]\u{2026}";

    /// A page's line naming the handle `h`.
    fn line(h: &str) -> String {
        format!(
            "- 2026-03-03 \u{b7} cli \u{b7} the owner: \u{ab}w\u{bb} \u{2192} you did \u{b7} {h}"
        )
    }

    /// What the kept text of a page names: a whole line by its exchange's handle, at the line's end, and only in
    /// a result whose first line is a page's. In a cut result, a line beside the seam is never whole, and a
    /// result whose seam is not found once shows nothing. A handle anywhere else, a look's result and another
    /// tool's result show nothing.
    #[test]
    fn a_result_shows_a_source_only_where_its_kept_text_names_the_handle() {
        let mut source = Node::user("ses_a", Some("turn_a"), "cli", "words");
        let page = format!("{HEAD}\n{}\nHistory is exhausted.", line("turn_a"));
        assert!(shown(&page, false, &source));
        assert!(!shown(&line("turn_a"), false, &source), "no page's head");
        // A cut page: the head's lines but its last, the tail's after its first line break.
        let cut = |at: usize| {
            let mut l: Vec<String> = ["turn_x"; 4].map(line).into();
            l[at] = line("turn_a");
            let [a, b, c, d] = [&l[0], &l[1], &l[2], &l[3]];
            format!("{HEAD}\n{a}\n{b}\n{SEAM}\n{c}\n{d}\nHistory is exhausted.")
        };
        assert!(shown(&cut(0), true, &source), "whole in the head");
        assert!(!shown(&cut(1), true, &source), "beside the seam, head");
        assert!(!shown(&cut(2), true, &source), "beside the seam, tail");
        assert!(shown(&cut(3), true, &source), "whole in the tail");
        let unseamed = cut(0).replace(SEAM, "[cut]");
        assert!(!shown(&unseamed, true, &source), "no seam");
        let two = cut(3).replace("History is exhausted.", SEAM);
        assert!(!shown(&two, true, &source), "two seams");
        // Elsewhere, and other tools.
        let elsewhere = format!("{HEAD}\nMore before these: before \u{b7} turn_a.");
        assert!(!shown(&elsewhere, false, &source));
        let look = result(crate::look::WORK, &page, false);
        assert!(!shows(&look, &source, || true));
        assert!(!shows(&result("fs.read", &page, false), &source, || true));
        source.turn_id = Some("turn_b".into());
        assert!(!shown(&page, false, &source));
        let longer = page.replace("turn_a", "turn_ab");
        source.turn_id = Some("turn_a".into());
        assert!(!shown(&longer, false, &source));
    }

    /// An open shows its source by its heading, only in a result the runtime did not cut, and never by a body
    /// line. An imported message's line names its session: in a cut page it shows the source only when the
    /// result links no other node of that session; a page of imports is not read by a turn's handle, nor a page
    /// of exchanges by a session's.
    #[test]
    fn an_open_shows_by_its_heading_and_an_import_line_by_its_session() {
        let source = Node::user("ses_a", Some("turn_a"), "cli", "words");
        let heading = "[Exchange \"turn_a\", in \"cli\", 2026-03-03: what was said]\n\u{2502} w";
        assert!(shown(heading, false, &source));
        assert!(!shown(heading, true, &source), "a cut open");
        let session = "[Session \"ses_a\", imported, its messages]\n\u{2502} words";
        assert!(shown(session, false, &source));
        assert!(!shown(session, true, &source));
        let (l, end) = (line("turn_a"), "That is its beginning.");
        let forged =
            format!("[Session \"ses_b\", its messages]\n\u{2502} a\n{SEAM}\n{l}\n{l}\n{end}");
        assert!(!shown(&forged, true, &source), "a body line of a cut open");
        let whole = forged.replace(&format!("{SEAM}\n"), "");
        assert!(!shown(&whole, false, &source), "a body line of an open");
        let head = "context.history: 2 imported items for \u{ab}w\u{bb} in import, best first.";
        let imports = |h: &str| {
            format!("{head}\n- 2026-03-03 \u{b7} casebook: \u{ab}w\u{bb} \u{b7} {h}\nThat is all it found.")
        };
        assert!(shown(&imports("ses_a"), false, &source));
        assert!(!shown(&imports("turn_a"), false, &source), "by a turn");
        let page = format!("{HEAD}\n{}\nHistory is exhausted.", line("ses_a"));
        assert!(!shown(&page, false, &source), "a page by a session");
        let cut_imports = |alone: bool| {
            let i = imports("ses_a");
            let l: Vec<&str> = i.lines().collect();
            let text = format!("{}\n{}\nfirst\n{SEAM}\nlast\n{}", l[0], l[1], l[2]);
            let history = crate::view::history::HISTORY_TOOL;
            shows(&result(history, &text, true), &source, || alone)
        };
        assert!(cut_imports(true), "the one node of its session linked");
        assert!(!cut_imports(false), "another linked node shares it");
    }

    /// A result kept by reference shows a source when a quote of it kept a byte, whatever the runtime cut: an
    /// open cut by the runtime, a look's result, a quote of another node, an empty quote, a handle in the code's
    /// own words, and another tool's result are each told apart by the parts alone.
    #[test]
    fn a_parts_result_shows_a_source_by_a_kept_quote_of_it() {
        use crate::quotes::{Field, Form, Part, Quote};
        let source = Node::user("ses_a", Some("turn_a"), "cli", "words");
        let quote = |node: &str, (from, to): (u32, u32)| {
            Part::Quote(Quote {
                node: node.into(),
                position: 7,
                field: Field::Text,
                from,
                to,
                form: Form::Margin1,
            })
        };
        let of = |tool: &str, truncated: bool, parts: Vec<Part>| {
            let mut n = result(tool, "", truncated);
            if let Body::ToolResult { parts: p, .. } = &mut n.body {
                *p = parts;
            }
            n
        };
        let head =
            || Part::Code("[Exchange \"turn_a\", in \"cli\", 2026-03-03: what was said]\n".into());
        let history = crate::view::history::HISTORY_TOOL;
        let cut = of(history, true, vec![head(), quote(&source.id, (0, 5))]);
        assert!(
            shows(&cut, &source, || unreachable!()),
            "a cut open's kept quote"
        );
        let look = of(crate::look::WORK, false, vec![quote(&source.id, (0, 2))]);
        assert!(shows(&look, &source, || unreachable!()), "a look's quote");
        let other = of(history, false, vec![head(), quote("msg_other", (0, 5))]);
        assert!(
            !shows(&other, &source, || true),
            "the handle in code, another node quoted"
        );
        let empty = of(history, false, vec![head(), quote(&source.id, (3, 3))]);
        assert!(!shows(&empty, &source, || true), "no byte kept");
        let tool = of("fs.read", false, vec![quote(&source.id, (0, 5))]);
        assert!(!shows(&tool, &source, || true), "another tool");
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
