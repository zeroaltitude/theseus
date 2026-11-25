//! A look back's result kept by reference, laid: an erase blanks only the words of the quote it erased, and a
//! quoted source marked wrong is said once, by the result's rendering; its own mark is said after it. Its copy
//! edges, as the writer leaves them, are not read for it. All names and words invented.

use super::*;
use crate::compiler::tests_view::result;
use crate::quotes::{Field, Form, Part, Quote, ERASED_QUOTE};

const AT_MS: u64 = 1_790_000_000_000;
const NORTH: &str = "the heron nests by the north weir";
const SOUTH: &str = "the tern nests by the south weir";
const WHY: &str = "it nests by the mill race";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("store")).unwrap();
    (dir, s)
}

/// A message saying `words` in the exchange `turn`, stored: it and its position.
fn said(s: &Store, turn: &str, words: &str) -> (Node, u64) {
    let n = Node::user("s", Some(turn), "cli", words);
    let at = s.append(&[n.record().unwrap()]).unwrap()[0];
    (n, at)
}

/// The page head, and one page line's code before and after its quote, for the exchange `turn`.
const HEAD: &str = "context.history: 2 exchanges of cli's history, newest first.\n";
const BEFORE: &str = "- 2026-03-03 \u{b7} cli \u{b7} the owner: \u{ab}";
fn after(turn: &str) -> String {
    format!("\u{bb} \u{2192} you did \u{b7} {turn}\n")
}
const END: &str = "History is exhausted.";

/// A page of two lines kept by reference, each the code's words around a quote of one message's words in a
/// page's form, stored with a copy edge to each message, as the history tool's completion leaves it.
fn page(s: &Store, lines: [&(Node, u64); 2]) -> Node {
    let mut parts = vec![Part::Code(HEAD.into())];
    for (n, at) in lines {
        let Body::UserMessage { text, .. } = &n.body else {
            unreachable!()
        };
        parts.push(Part::Code(BEFORE.into()));
        parts.push(Part::Quote(Quote {
            node: n.id.clone(),
            position: *at,
            field: Field::Text,
            from: 0,
            to: text.len() as u32,
            form: Form::Flat1,
        }));
        parts.push(Part::Code(after(n.turn_id.as_deref().unwrap())));
    }
    parts.push(Part::Code(END.into()));
    let mut r = result("t9", "h1", "");
    if let Body::ToolResult { tool, parts: p, .. } = &mut r.body {
        *tool = crate::view::history::HISTORY_TOOL.into();
        *p = parts;
    }
    let mut records = vec![r.record().unwrap()];
    for (n, _) in lines {
        let e = Edge::new(
            EdgeKind::DerivedFrom,
            &r.id,
            &n.id,
            crate::graph::VIA_HISTORY,
        );
        records.push(e.record().unwrap());
    }
    s.append(&records).unwrap();
    r
}

/// `n` as a window lays it: its words the rendering of its parts (`quotes::render`), as a reader fills them,
/// then what has been said since.
fn lay(s: &Store, wrong: &BTreeSet<String>, n: &Node) -> String {
    let since = Since::new(s, wrong);
    let mut read = n.clone();
    if let Body::ToolResult { content, parts, .. } = &mut read.body {
        *content = crate::quotes::render(s, parts, &since).unwrap();
    }
    let laid = since.lay(vec![(0, Stub::from(read))]).unwrap();
    match &laid[0].1.body {
        Body::ToolResult { content, .. } => content.clone(),
        b => panic!("{b:?}"),
    }
}

/// `node`'s retraction row, as `fact.retract` writes it.
fn retracted(s: &Store, node: &str, gist: &str) {
    let f = crate::fact::retract::Retracted {
        node_id: node,
        session_id: "s",
        gist,
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
    s.append(&[row.scoped(crate::retract::SCOPE)]).unwrap();
}

/// A source erased since blanks the words of its own quote, one fixed line in their place; the rest of the
/// result is laid as it was shown, the other line's words with it, and no whole-result line replaces it.
#[test]
fn an_erased_span_blanks_only_its_lines() {
    let (_d, s) = store();
    let north = said(&s, "t0", NORTH);
    let south = said(&s, "t1", SOUTH);
    let n = page(&s, [&north, &south]);
    let shown = format!(
        "{HEAD}{BEFORE}{NORTH}{}{BEFORE}{SOUTH}{}{END}",
        after("t0"),
        after("t1")
    );
    assert_eq!(lay(&s, &BTreeSet::new(), &n), shown, "as shown");
    s.append(&[north.0.erased(AT_MS, "a test").record().unwrap()])
        .unwrap();
    let laid = lay(&s, &BTreeSet::new(), &n);
    assert_eq!(
        laid,
        format!(
            "{HEAD}{BEFORE}{ERASED_QUOTE}{}{BEFORE}{SOUTH}{}{END}",
            after("t0"),
            after("t1")
        )
    );
    assert!(!laid.contains("north"), "{laid}");
    assert_ne!(laid, ERASED_RESULT);
}

/// A quoted source marked wrong is said once, by the rendering, under the result; the result itself marked
/// wrong has its own line after that.
#[test]
fn a_quoted_source_marked_wrong_is_said_once_by_its_rendering() {
    let (_d, s) = store();
    let north = said(&s, "t0", NORTH);
    let south = said(&s, "t1", SOUTH);
    let n = page(&s, [&north, &south]);
    retracted(&s, &south.0.id, SOUTH);
    let shown = format!(
        "{HEAD}{BEFORE}{NORTH}{}{BEFORE}{SOUTH}{}{END}",
        after("t0"),
        after("t1")
    );
    let day = crate::judge::spend::local_day(AT_MS);
    let line = format!(
        "[Since this was read: \u{201c}{SOUTH}\u{201d} was marked wrong on {day}: {WHY}. The words above are as \
         they were said.]"
    );
    let wrong = BTreeSet::from([south.0.id.clone()]);
    assert_eq!(lay(&s, &wrong, &n), format!("{shown}\n{line}"));
    let wrong = BTreeSet::from([south.0.id, n.id.clone()]);
    let own = "[Since this was said: it was labelled wrong or stale. The words above are as they were said.]";
    assert_eq!(lay(&s, &wrong, &n), format!("{shown}\n{line}\n{own}"));
}
