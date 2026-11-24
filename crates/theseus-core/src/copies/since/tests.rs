//! What a window lays beside a node: a look back's result and a task's report by their sources, a node by
//! its own label; nothing else changes, the stored node least of all.

use serde_json::json;

use super::*;
use crate::compiler::tests_view::result;

/// The message a result showed, and the retraction's words.
const GIST: &str = "the heron nests by the north weir";
const WHY: &str = "it nests by the south weir";
const AT_MS: u64 = 1_790_000_000_000;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("store")).unwrap();
    (dir, s)
}

/// The source message, stored: its id.
fn source(store: &Store) -> String {
    let n = Node::user("s", Some("t0"), "cli", GIST);
    store.append(&[n.record().unwrap()]).unwrap();
    n.id
}

/// `node`'s retraction row, as `fact.retract` writes it.
fn retracted(store: &Store, node: &str) {
    let f = crate::fact::retract::Retracted {
        node_id: node,
        session_id: "s",
        gist: GIST,
        why: WHY,
        who: "owner",
        via: "cli",
        at_ms: AT_MS,
        exposures: &[],
        partial: false,
        card: false,
        private: true,
        labeled: &[],
    };
    let row = crate::fact::row(&f, Some("s"), None).unwrap();
    store.append(&[row.scoped(crate::retract::SCOPE)]).unwrap();
}

/// A result of `tool` that showed `of`, by `via`, stored with its edge; marked outside, with meta. Its text is
/// a page's head and one line, which names the handle of [`source`]'s exchange (`t0`) at its end.
fn shown_by(store: &Store, tool: &str, via: &str, of: &str) -> Node {
    let line = format!(
        "context.history: 1 exchange of cli's history, newest first.\n\
         - 2026-03-03 \u{b7} cli \u{b7} the owner: \u{ab}{GIST}\u{bb} \u{2192} you did \u{b7} t0"
    );
    let mut n = result("t1", "h1", &line);
    if let Body::ToolResult {
        tool: t,
        external,
        meta,
        ..
    } = &mut n.body
    {
        *t = tool.into();
        *external = Some(theseus_tools::External {
            url: "history:s".into(),
        });
        *meta = json!({"look": "page", "lines": 1});
    }
    let e = Edge::new(EdgeKind::DerivedFrom, &n.id, of, via);
    store
        .append(&[n.record().unwrap(), e.record().unwrap()])
        .unwrap();
    n
}

fn lay_one(store: &Store, wrong: &BTreeSet<String>, n: &Node) -> Node {
    let laid = Since::new(store, wrong)
        .lay(vec![(0, Stub::from(n.clone()))])
        .unwrap();
    (*laid[0].1).clone()
}

fn content(n: &Node) -> &str {
    match &n.body {
        Body::ToolResult { content, .. } => content,
        Body::UserMessage { text, .. } => text,
        b => panic!("{b:?}"),
    }
}

/// The day the line names.
fn day() -> String {
    crate::judge::spend::local_day(AT_MS)
}

/// A history result whose source was retracted is laid with the line, after its words as they were; its
/// tool, its outside mark and its meta are as stored, and the stored node is not written.
#[test]
fn a_look_backs_result_is_laid_with_what_was_said_since_and_its_mark_unchanged() {
    let (_d, s) = store();
    let src = source(&s);
    retracted(&s, &src);
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &src,
    );
    let at = s.last_position();
    let wrong = BTreeSet::from([src]);
    let laid = lay_one(&s, &wrong, &n);
    assert_eq!(
        content(&laid),
        format!(
            "{}\n[Since this was read: \u{201c}{GIST}\u{201d} was marked wrong on {}: {WHY}. The words \
             above are as they were said.]",
            content(&n),
            day()
        )
    );
    let (
        Body::ToolResult {
            tool: t0,
            external: x0,
            meta: m0,
            tool_use_id: u0,
            ..
        },
        Body::ToolResult {
            tool: t1,
            external: x1,
            meta: m1,
            tool_use_id: u1,
            ..
        },
    ) = (&n.body, &laid.body)
    else {
        panic!("{laid:?}");
    };
    assert_eq!((t0, x0, m0, u0), (t1, x1, m1, u1));
    assert!(x1.is_some(), "still marked outside");
    assert_eq!(s.last_position(), at, "nothing is written");
    let (_, stored) = s.get_node(&n.id).unwrap().unwrap();
    assert_eq!(
        content(&stored),
        content(&n),
        "the stored node is as written"
    );
}

/// `look.work`'s result names no handle (`look/work/history.rs`), so its kept text never shows which message
/// it holds: it is laid as stored when its source is retracted (the look reads only its own conversation,
/// whose own message carries its mark where it is laid whole), and as the erased line when it is erased.
#[test]
fn a_looks_result_names_no_handle_and_is_laid_without_the_line() {
    let (_d, s) = store();
    let src = source(&s);
    retracted(&s, &src);
    let n = shown_by(&s, crate::look::WORK, crate::graph::VIA_LOOK, &src);
    let laid = lay_one(&s, &BTreeSet::from([src.clone()]), &n);
    assert_eq!(
        serde_json::to_vec(&laid).unwrap(),
        serde_json::to_vec(&n).unwrap()
    );
    let (_, was) = s.get_node(&src).unwrap().unwrap();
    s.append(&[was.erased(AT_MS, "a test").record().unwrap()])
        .unwrap();
    assert_eq!(content(&lay_one(&s, &BTreeSet::new(), &n)), ERASED_RESULT);
}

/// A history result whose kept text does not name its retracted source's handle (the runtime cut its line)
/// is laid as stored: the line would bring the gist and the reason of a message the reader never saw.
#[test]
fn a_retracted_source_the_kept_text_does_not_show_lays_no_line() {
    let (_d, s) = store();
    let src = Node::user("s", Some("t_cut"), "cli", GIST);
    s.append(&[src.record().unwrap()]).unwrap();
    retracted(&s, &src.id);
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &src.id,
    );
    let laid = lay_one(&s, &BTreeSet::from([src.id]), &n);
    assert_eq!(
        serde_json::to_vec(&laid).unwrap(),
        serde_json::to_vec(&n).unwrap()
    );
}

/// A result of another tool is laid byte for byte, even with an edge and a labelled node beside it; so is a
/// look back's result whose sources nothing was said about.
#[test]
fn a_result_of_another_tool_is_laid_byte_for_byte() {
    let (_d, s) = store();
    let src = source(&s);
    retracted(&s, &src);
    let other = shown_by(&s, "fs.read", crate::graph::VIA_HISTORY, &src);
    let wrong = BTreeSet::from([src.clone()]);
    let laid = lay_one(&s, &wrong, &other);
    assert_eq!(
        serde_json::to_vec(&laid).unwrap(),
        serde_json::to_vec(&other).unwrap()
    );
    let quiet = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &src,
    );
    let laid = lay_one(&s, &BTreeSet::new(), &quiet);
    assert_eq!(
        serde_json::to_vec(&laid).unwrap(),
        serde_json::to_vec(&quiet).unwrap()
    );
}

/// A source erased since: one fixed line, and none of the result's words.
#[test]
fn an_erased_source_lays_one_line_and_none_of_its_words() {
    let (_d, s) = store();
    let src = source(&s);
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &src,
    );
    let (_, was) = s.get_node(&src).unwrap().unwrap();
    s.append(&[was.erased(AT_MS, "a test").record().unwrap()])
        .unwrap();
    let laid = lay_one(&s, &BTreeSet::new(), &n);
    assert_eq!(content(&laid), ERASED_RESULT);
    assert!(!content(&laid).contains("heron"));
}

/// A task's report is laid with what was said of the message it relayed, by its report edge.
#[test]
fn a_report_is_laid_with_what_was_said_of_its_message() {
    let (_d, s) = store();
    let src = source(&s);
    retracted(&s, &src);
    let n = Node::relayed("s", Some("t2"), Origin::Harness, "task:abc123", GIST);
    let e = Edge::new(EdgeKind::DerivedFrom, &n.id, &src, VIA_REPORT);
    s.append(&[n.record().unwrap(), e.record().unwrap()])
        .unwrap();
    let laid = lay_one(&s, &BTreeSet::from([src]), &n);
    assert!(content(&laid).starts_with(GIST), "{laid:?}");
    assert!(
        content(&laid).contains("[Since this was read: "),
        "{laid:?}"
    );
}

/// The node itself, marked wrong, carries the mark wherever it is laid: a message in its text, an answer in
/// its last text block, its call untouched.
#[test]
fn a_node_marked_wrong_is_laid_with_its_mark() {
    let (_d, s) = store();
    let ask = Node::user("s", Some("t3"), "cli", GIST);
    let mut answer = Node::user("s", Some("t3"), "cli", "x");
    answer.body = Body::AssistantMessage {
        blocks: vec![
            json!({"type": "text", "text": "Noted."}),
            json!({"type": "tool_use", "id": "c1", "name": "fs_read", "input": {"path": "a"}}),
        ],
        model: "m".into(),
        provider: "anthropic".into(),
        stop_reason: None,
        usage: Default::default(),
        cost_usd: None,
        catalog_version: None,
        request_id: None,
        correlation_id: None,
        compilation_id: None,
        request_digest: None,
        effort: None,
    };
    s.append(&[ask.record().unwrap(), answer.record().unwrap()])
        .unwrap();
    retracted(&s, &ask.id);
    let wrong = BTreeSet::from([ask.id.clone(), answer.id.clone()]);
    let line = format!(
        "[Since this was said: it was marked wrong on {}: {WHY}. The words above are as they were said.]",
        day()
    );
    assert_eq!(
        content(&lay_one(&s, &wrong, &ask)),
        format!("{GIST}\n{line}")
    );
    let laid = lay_one(&s, &wrong, &answer);
    let Body::AssistantMessage { blocks, .. } = &laid.body else {
        panic!("{laid:?}");
    };
    assert_eq!(
        blocks[0]["text"],
        "Noted.\n\n[Since this was said: it was labelled wrong or stale. The words above are as they were \
         said.]"
    );
    let Body::AssistantMessage { blocks: was, .. } = &answer.body else {
        unreachable!()
    };
    assert_eq!(blocks[1], was[1], "the call is as stored");
}

/// The store's node reads so far: each `get_node` that finds a node is one cache hit or one miss.
fn node_reads(s: &Store) -> u64 {
    let h = s.node_cache().health();
    h.hits + h.misses
}

/// An exchange with no look back's result and no report costs no store read here: no node is read, no
/// scope is read (every scope read fails while the fault is on, and the lay still answers), and an edge
/// planted from each node to an erased source is never followed (followed, it would read that source and
/// lay the erased line). Each node is the same bytes.
#[test]
fn an_exchange_with_no_look_back_and_no_report_reads_nothing() {
    let (_d, s) = store();
    let src = source(&s);
    retracted(&s, &src);
    let (_, was) = s.get_node(&src).unwrap().unwrap();
    s.append(&[was.erased(AT_MS, "a test").record().unwrap()])
        .unwrap();
    let ask = Node::user("s", Some("t4"), "cli", "where does it nest?");
    let mut answer = Node::user("s", Some("t4"), "cli", "x");
    answer.body = Body::AssistantMessage {
        blocks: vec![
            json!({"type": "text", "text": "Reading."}),
            json!({"type": "tool_use", "id": "c1", "name": "fs_read", "input": {"path": "a"}}),
        ],
        model: "m".into(),
        provider: "anthropic".into(),
        stop_reason: None,
        usage: Default::default(),
        cost_usd: None,
        catalog_version: None,
        request_id: None,
        correlation_id: None,
        compilation_id: None,
        request_digest: None,
        effort: None,
    };
    let note = Node::relayed("s", Some("t4"), Origin::Harness, "retract", "a note");
    s.append(&[
        ask.record().unwrap(),
        answer.record().unwrap(),
        note.record().unwrap(),
    ])
    .unwrap();
    let other = shown_by(&s, "fs.read", crate::graph::VIA_HISTORY, &src);
    let mut planted = Vec::new();
    for n in [&ask, &answer, &note] {
        for via in [
            crate::graph::VIA_HISTORY,
            crate::graph::VIA_LOOK,
            VIA_REPORT,
        ] {
            planted.push(
                Edge::new(EdgeKind::DerivedFrom, &n.id, &src, via)
                    .record()
                    .unwrap(),
            );
        }
    }
    s.append(&planted).unwrap();
    // The label set holds the retracted source, so it is not empty; no laid node is in it.
    let wrong = BTreeSet::from([src]);
    let exchange = [ask, answer, note, other];
    let reads = node_reads(&s);
    s.fail_scope_reads(true);
    let laid = Since::new(&s, &wrong)
        .lay(
            (exchange.iter())
                .map(|n| (0, Stub::from(n.clone())))
                .collect(),
        )
        .unwrap();
    s.fail_scope_reads(false);
    assert_eq!(node_reads(&s), reads, "no node was read");
    for (n, (_, l)) in exchange.iter().zip(&laid) {
        assert_eq!(
            serde_json::to_vec(&**l).unwrap(),
            serde_json::to_vec(n).unwrap()
        );
    }
}

/// A look back's result costs one node read per distinct source it names (one edge each, a second edge to
/// the same source read once), and the retraction rows are read once a window, however many nodes need them.
#[test]
fn a_look_backs_result_reads_each_source_once() {
    let (_d, s) = store();
    let sources: Vec<String> = (0..3).map(|_| source(&s)).collect();
    let a = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &sources[0],
    );
    let mut more = Vec::new();
    for src in [&sources[1], &sources[2], &sources[2]] {
        more.push(
            Edge::new(EdgeKind::DerivedFrom, &a.id, src, crate::graph::VIA_HISTORY)
                .record()
                .unwrap(),
        );
    }
    s.append(&more).unwrap();
    let none = BTreeSet::new();
    let since = Since::new(&s, &none);
    let reads = node_reads(&s);
    since.lay(vec![(0, Stub::from(a.clone()))]).unwrap();
    assert_eq!(node_reads(&s) - reads, 3, "one read per distinct source");
    // Two labelled nodes in one window: the retraction scope is read once (the second read would fail).
    retracted(&s, &sources[0]);
    let wrong = BTreeSet::from([sources[0].clone()]);
    let since = Since::new(&s, &wrong);
    since.lay(vec![(0, Stub::from(a.clone()))]).unwrap();
    s.fail_scope_reads(true);
    let laid = since.lay(vec![(0, Stub::from(a))]);
    s.fail_scope_reads(false);
    let laid = laid.unwrap();
    assert!(content(&laid[0].1).contains("[Since this was read: "));
}

/// Two windows with nothing said between them lay the same bytes.
#[test]
fn two_lays_with_nothing_said_between_are_the_same_bytes() {
    let (_d, s) = store();
    let src = source(&s);
    retracted(&s, &src);
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &src,
    );
    let wrong = BTreeSet::from([src]);
    let a = serde_json::to_vec(&lay_one(&s, &wrong, &n)).unwrap();
    let b = serde_json::to_vec(&lay_one(&s, &wrong, &n)).unwrap();
    assert_eq!(a, b);
}

/// A named source that cannot be found is not shown: the result is laid as the erased line, and a replay of
/// it is refused, as recall does not show a source it cannot read.
#[test]
fn a_source_that_cannot_be_found_is_not_shown() {
    let (_d, s) = store();
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        "msg_never_written",
    );
    let laid = lay_one(&s, &BTreeSet::new(), &n);
    assert_eq!(content(&laid), ERASED_RESULT);
    assert!(shows_erased(&s, &n).unwrap());
}

/// A result with as many edges as are read is laid as it is when nothing was said of its sources; one edge
/// more and its sources cannot all be checked: none of its words are laid, one fixed line is, and a replay
/// of it is refused.
#[test]
fn a_result_past_the_edges_read_lays_none_of_its_words() {
    let (_d, s) = store();
    let first = source(&s);
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &first,
    );
    let more = |k: usize| -> Vec<theseus_store::NewRecord> {
        let sources: Vec<Node> = (0..k)
            .map(|_| Node::user("s", Some("t0"), "cli", "x"))
            .collect();
        let mut out: Vec<_> = sources.iter().map(|m| m.record().unwrap()).collect();
        out.extend(sources.iter().map(|m| {
            Edge::new(
                EdgeKind::DerivedFrom,
                &n.id,
                &m.id,
                crate::graph::VIA_HISTORY,
            )
            .record()
            .unwrap()
        }));
        out
    };
    s.append(&more(SOURCES_READ - 1)).unwrap();
    let laid = lay_one(&s, &BTreeSet::new(), &n);
    assert_eq!(content(&laid), content(&n), "at the bound, as stored");
    assert!(!shows_erased(&s, &n).unwrap());
    s.append(&more(1)).unwrap();
    let laid = lay_one(&s, &BTreeSet::new(), &n);
    assert_eq!(
        content(&laid),
        "[This result showed more messages than can be checked now; its words are not laid.]"
    );
    assert!(shows_erased(&s, &n).unwrap());
}

/// Each line is laid once, in the order first said: two sources labelled with no retraction row say the
/// same line, and a retracted source read between them does not make it said twice.
#[test]
fn each_line_is_laid_once_in_the_order_first_said() {
    let (_d, s) = store();
    let mut ids: Vec<String> = (0..3).map(|_| source(&s)).collect();
    ids.sort();
    // Read in key order (the edge's key ends with the source's id): bare, retracted, bare.
    retracted(&s, &ids[1]);
    let n = shown_by(
        &s,
        crate::view::history::HISTORY_TOOL,
        crate::graph::VIA_HISTORY,
        &ids[0],
    );
    let edges: Vec<_> = (ids[1..].iter())
        .map(|src| {
            Edge::new(EdgeKind::DerivedFrom, &n.id, src, crate::graph::VIA_HISTORY)
                .record()
                .unwrap()
        })
        .collect();
    s.append(&edges).unwrap();
    let wrong: BTreeSet<String> = ids.iter().cloned().collect();
    let bare = "[Since this was read: a message it showed was labelled wrong or stale. The words above are as \
                they were said.]";
    let row = format!(
        "[Since this was read: \u{201c}{GIST}\u{201d} was marked wrong on {}: {WHY}. The words above are as \
         they were said.]",
        day()
    );
    let laid = lay_one(&s, &wrong, &n);
    assert_eq!(
        content(&laid),
        format!("{}\n{bare}\n{row}", content(&n)),
        "{laid:?}"
    );
}
