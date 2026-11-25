//! Every reader lays a result kept by reference through the one renderer, and a turn lays each rendering as it
//! first made it (issue 49). The results are made by hand with the record's types. All names and words invented.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;

use super::*;
use crate::compiler::tests_view::{frozen, result, run, said};
use crate::provider::binding::unmarked;
use crate::quotes::{Form, Quote, ERASED_QUOTE};

const WORDS: &str = "the grey heron nests by the old weir";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("store")).unwrap();
    (dir, s)
}

/// `n` stored: its position.
fn put(s: &Store, n: &Node) -> u64 {
    s.append(&[n.record().unwrap()]).unwrap()[0]
}

/// The message the page quotes, stored: its id and its position.
fn source(s: &Store) -> (String, u64) {
    let n = Node::user("ses_weir", Some("t0"), "cli", WORDS);
    (n.id.clone(), put(s, &n))
}

/// A page line kept by reference: the code's words around the whole of `node`'s text.
fn page_parts(node: &str, position: u64) -> Vec<Part> {
    vec![
        Part::Code("- ".into()),
        Part::Quote(Quote {
            node: node.into(),
            position,
            field: Field::Text,
            from: 0,
            to: WORDS.len() as u32,
            form: Form::Flat1,
        }),
        Part::Code(" \u{b7} t0".into()),
    ]
}

/// The page as the line reads.
fn page_text() -> String {
    format!("- {WORDS} \u{b7} t0")
}

/// A result of `turn` answering `call` kept by reference: no words of its own.
fn page(turn: &str, call: &str, parts: Vec<Part>) -> Node {
    let mut n = result(turn, call, "");
    if let Body::ToolResult { tool, parts: p, .. } = &mut n.body {
        *tool = "context.history".into();
        *p = parts;
    }
    n
}

fn stubs(nodes: &[Node]) -> Vec<(u64, Stub)> {
    (nodes.iter().enumerate())
        .map(|(i, n)| (1000 + i as u64, n.clone().into()))
        .collect()
}

fn thinking() -> serde_json::Value {
    json!({"type": "thinking", "thinking": "", "signature": "sig"})
}

/// A turn whose first step read history, kept by reference: its second step repeats the first step's bytes,
/// the rendering included, though its quoted message was marked wrong in between (a since line would now
/// follow the words): the turn rendered it once.
#[test]
fn a_two_step_turn_that_read_history_repeats_its_bytes() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let mut f = frozen("t1", &[]);
    let read = json!({"type": "tool_use", "id": "c1", "name": "context_history", "input": {}});
    let mut own = vec![
        Node::user("s", Some("t1"), "cli", "where does the heron nest?"),
        said("t1", vec![thinking(), read]),
        page("t1", "c1", page_parts(&id, at)),
    ];
    freeze(&s, &mut f, &stubs(&own), || Ok(BTreeSet::new())).unwrap();
    let first = run(&f, &own, None);
    let sent = serde_json::to_string(&first.request.messages).unwrap();
    assert!(sent.contains(&page_text()), "{sent}");
    f.compilation_id = Some(first.compilation.id.clone());

    own.push(said(
        "t1",
        vec![thinking(), json!({"type": "text", "text": "By the weir."})],
    ));
    let marked = || Ok(BTreeSet::from([id.clone()]));
    freeze(&s, &mut f, &stubs(&own), marked).unwrap();
    let second = run(&f, &own, Some(&first.compilation));
    assert!(!second.new_compilation, "the same chain");
    for (i, m) in first.request.messages.iter().enumerate() {
        let (a, b) = (unmarked(m), unmarked(&second.request.messages[i]));
        assert_eq!(a, b, "messages.{i} changed between steps");
    }
}

/// A result of the turn's own kept by reference that the turn did not render is never laid empty, and never
/// rendered again at the request: it lays the fixed line of a quote that cannot be read.
#[test]
fn an_own_result_not_rendered_lays_the_fixed_line() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let f = frozen("t1", &[]);
    let n = page("t1", "c1", page_parts(&id, at));
    let laid = own_laid(Stub::from(n), &f);
    assert_eq!(
        crate::view::text::forms::result_text(&laid).unwrap(),
        UNREADABLE_QUOTE
    );
}

/// The window lays a result kept by reference as its rendering, keeps the node it read, and lays nothing more
/// beside it when nothing was said since; an old result with its words reads as it always did.
#[test]
fn the_window_lays_the_rendering_and_an_old_result_as_stored() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let none = BTreeSet::new();
    let since = Since::new(&s, &none);
    let nodes = stubs(&[
        page("t0", "c1", page_parts(&id, at)),
        result("t0", "c2", "as said"),
    ]);
    let mut read = Vec::new();
    let laid = lay_window(&s, nodes, &since, &mut read).unwrap();
    let laid = since.lay(laid).unwrap();
    let text = |i: usize| crate::view::text::forms::result_text(&laid[i].1).unwrap();
    assert_eq!((text(0), text(1)), (page_text(), "as said".to_string()));
    assert_eq!(read, vec![id]);
}

/// A rendering the turn made is carried by a reassembly of the same turn, but not one whose quoted message
/// was erased since; rendered again, it shows the erased line, and reads nothing that could be erased.
#[test]
fn an_erase_drops_the_turns_rendering_and_the_next_shows_the_line() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let n = page("t1", "c1", page_parts(&id, at));
    let none = BTreeSet::new();
    let r = rendering(&s, &n, &Since::new(&s, &none)).unwrap().unwrap();
    let quoted = HashMap::from([(n.id.clone(), r.clone())]);
    assert!(!erased_since(&s, &r.read));
    assert_eq!(carried(&s, quoted.clone()).len(), 1);

    let (_, m) = s.get_node(&id).unwrap().unwrap();
    put(&s, &m.erased(1, "a test"));
    assert!(erased_since(&s, &r.read));
    assert!(carried(&s, quoted).is_empty());
    let again = rendering(&s, &n, &Since::new(&s, &none)).unwrap().unwrap();
    assert_eq!(again.text, format!("- {ERASED_QUOTE} \u{b7} t0"));
    assert!(again.read.is_empty());
}

/// One of the turn's own results kept by reference whose ONE quoted message of two was erased before the turn
/// rendered it lays that quote as the erased line and the other with its words (issue 49): the turn's own
/// results are rendered as the window's are, quote by quote.
#[test]
fn an_own_result_blanks_only_its_erased_quote() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let tern = Node::user("ses_weir", Some("t1"), "cli", "the tern nests by the mill");
    let tern_at = put(&s, &tern);
    let mut parts = page_parts(&id, at);
    parts.push(Part::Code("\n- ".into()));
    parts.push(Part::Quote(Quote {
        node: tern.id,
        position: tern_at,
        field: Field::Text,
        from: 0,
        to: 26,
        form: Form::Flat1,
    }));
    let n = page("t1", "c1", parts);
    let (_, m) = s.get_node(&id).unwrap().unwrap();
    put(&s, &m.erased(1, "a test"));
    let mut f = frozen("t1", &[]);
    freeze(&s, &mut f, &stubs(std::slice::from_ref(&n)), || {
        Ok(BTreeSet::new())
    })
    .unwrap();
    let laid = own_laid(Stub::from(n), &f);
    assert_eq!(
        crate::view::text::forms::result_text(&laid).unwrap(),
        format!("- {ERASED_QUOTE} \u{b7} t0\n- the tern nests by the mill")
    );
}

/// A reader outside a conversation (inspect, a node's page, publish) shows the rendering; a node with no parts
/// as it is.
#[test]
fn a_node_shown_outside_a_conversation_is_its_rendering() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let n = page("t0", "c1", page_parts(&id, at));
    let info = crate::Core::node_info(0, &shown(&s, &n));
    assert_eq!(info.text, page_text());
    let old = result("t0", "c2", "as said");
    assert_eq!(crate::Core::node_info(0, &shown(&s, &old)).text, "as said");
}

/// A reader with no store at hand (a node's short preview, the command-result miner) reads a result kept by
/// reference as the fixed line of a quote that cannot be read now, never an empty text; an old result as its
/// words.
#[test]
fn a_reader_with_no_store_reads_the_fixed_line() {
    let (_d, s) = store();
    let (id, at) = source(&s);
    let n = page("t0", "c1", page_parts(&id, at));
    assert_eq!(
        n.preview(200),
        format!("context.history [ok] {UNREADABLE_QUOTE}")
    );
    let Body::ToolResult { content, parts, .. } = &n.body else {
        unreachable!()
    };
    assert_eq!(words_or_line(content, parts), UNREADABLE_QUOTE);
    let old = result("t0", "c2", "as said");
    let Body::ToolResult { content, parts, .. } = &old.body else {
        unreachable!()
    };
    assert_eq!(words_or_line(content, parts), "as said");
}

/// Every reader of a stored result's words (`Body::ToolResult { .. content .. }`, in a product source outside
/// a test module), each `<file under theseus-core/src> <count> <why it may read them>`. A new one fails here:
/// lay the result through this module first ([`laid`], [`shown`]), or say here why it need not.
const READERS: &[&str] = &[
    "arrangement.rs 1 a task's quote matched to a node: its callers lay the nodes first (`resolve_shown`)",
    "check.rs 1 the check's working text: its nodes laid first (`checked_nodes`)",
    "compiler/clearing.rs 3 the laid nodes, rendered before the clearing",
    "compiler/pairing.rs 1 the harness's refusal, never kept by reference",
    "compiler/render.rs 2 the laid nodes, rendered before the wire",
    "compiler/view.rs 2 the laid nodes: a turn's own head, a receipt",
    "content.rs 1 the content judge at entry, on what the writer wrote",
    "copies.rs 1 the old results' rule; parts ask their parts (R5)",
    "copies/since.rs 3 old results only: a parts result is read by its parts (R5)",
    "fact/tool.rs 2 the end event and its narration, at write",
    "flow/run.rs 1 a flow step's result, rendered when it holds parts",
    "judge/gate.rs 1 reads of files and pages, never kept by reference",
    "node.rs 2 the field itself, and a preview: the fixed line for parts (`words_or_line`)",
    "own_bench/whole/runloop.rs 1 a replay's results, rendered when they hold parts",
    "quoted.rs 1 the rendering set as a laid node's words",
    "recall.rs 1 `text_of`: the index's text (a result kept by reference indexes nothing); retract's gist renders first",
    "rpc/info.rs 1 nodes shown through `quoted::shown`",
    "rpc/inspect.rs 1 nodes shown through `quoted::shown`",
    "rpc/publish.rs 1 the node shown through `quoted::shown`",
    "rsi/mine/read.rs 1 command results mined, no store: the fixed line for parts (`words_or_line`)",
    "rsi/mint/source.rs 1 command results read, rendered when they hold parts",
    "toolrun.rs 1 the writer",
    "toolrun/parts.rs 2 the writer: the stored record's words cleared when it holds parts",
    "turn/checkpoint_step.rs 1 a failed command's error result",
    "turn/progress_step.rs 1 failed commands' results",
    "turn/view_step.rs 1 the window's nodes, rendered before the labels",
    "turn/view_step/room.rs 1 the turn's own, measured by its rendering",
    "view/sources_store.rs 1 rendered when it holds parts",
    "view/text/forms.rs 3 the laid nodes",
];

/// `s` up to its first inline test module.
fn product(s: &str) -> &str {
    let mut at = 0;
    while let Some(i) = s[at..].find("#[cfg(test)]") {
        let start = at + i;
        let rest = s[start + "#[cfg(test)]".len()..].trim_start();
        let rest = rest.strip_prefix("pub(crate) ").unwrap_or(rest);
        let rest = rest.strip_prefix("pub ").unwrap_or(rest);
        if let Some(m) = rest.strip_prefix("mod ") {
            let head: String = m.chars().take_while(|c| *c != ';' && *c != '{').collect();
            if m[head.len()..].starts_with('{') {
                return &s[..start];
            }
        }
        at = start + 1;
    }
    s
}

/// How many `ToolResult { .. }` of `s` name `content`.
fn readers_in(s: &str) -> usize {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut n = 0;
    for (i, _) in s.match_indices("ToolResult") {
        if s[..i].chars().next_back().is_some_and(ident) {
            continue;
        }
        let rest = s[i + "ToolResult".len()..].trim_start();
        let Some(body) = rest.strip_prefix('{') else {
            continue;
        };
        let body = &body[..body.find('}').unwrap_or(body.len())];
        let named = body.match_indices("content").any(|(j, w)| {
            !body[..j].chars().next_back().is_some_and(ident)
                && !body[j + w.len()..].chars().next().is_some_and(ident)
        });
        n += usize::from(named);
    }
    n
}

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if p.is_dir() {
            if !name.starts_with("tests") && name != "target" {
                walk(&p, out);
            }
        } else if name.ends_with(".rs") && !name.starts_with("tests") {
            out.push(p);
        }
    }
}

/// A result kept by reference stores no words, so a reader of `content` that does not render first lays
/// nothing. Every reader in the product's sources is one of [`READERS`], at its count.
#[test]
fn no_reader_reads_content_of_a_parts_result() {
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for c in std::fs::read_dir(&crates).unwrap().flatten() {
        let src = c.path().join("src");
        if src.is_dir() {
            walk(&src, &mut files);
        }
    }
    let mut found = BTreeMap::new();
    for p in files {
        let s = std::fs::read_to_string(&p).unwrap();
        let n = readers_in(product(&s));
        if n > 0 {
            let rel = p
                .strip_prefix(&crates)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            found.insert(rel, n);
        }
    }
    let listed: BTreeMap<String, usize> = (READERS.iter())
        .map(|r| {
            let mut w = r.splitn(3, ' ');
            let file = format!("theseus-core/src/{}", w.next().unwrap_or_default());
            (file, w.next().and_then(|n| n.parse().ok()).unwrap_or(0))
        })
        .collect();
    let files: BTreeSet<&String> = found.keys().chain(listed.keys()).collect();
    let differ: Vec<(&String, Option<&usize>, Option<&usize>)> = (files.into_iter())
        .map(|f| (f, found.get(f), listed.get(f)))
        .filter(|(_, a, b)| a != b)
        .collect();
    assert!(
        differ.is_empty(),
        "readers of a result's words (file, found, listed) not as listed: {differ:?}"
    );
}
