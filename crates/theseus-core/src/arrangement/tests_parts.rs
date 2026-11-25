//! A piece kept by reference (issue 49): a person's message or a reply is quoted by its record's position, cut
//! where its text is cut, and the stored piece keeps none of its words but its first line; the child's compile
//! reads them as first written, and an erased one shows the renderer's fixed line. All names and words invented.

use serde_json::{json, Value};

use super::*;
use crate::quotes::{ERASED_QUOTE, UNREADABLE_QUOTE};
use crate::scrub::Scrubber;
use crate::store::Store;

const AT_MS: u64 = 1_790_000_000_000;
const SID: &str = "ses_northlight";
const ASKED: &str = "The lamp room.\nRepaint it in signal white, two coats of marine enamel.";
const SAID: &str = "Noted.\nI will sand the walls between the coats.";
const READ: &str = "enamel: 4 tins, brushes: 2";
/// A value the vault knows, which a cut keeps whole or not at all.
const VALUE: &str = "kelp-ferry-0417-vault-token";

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(&dir.path().join("store")).unwrap();
    (dir, s)
}

/// `n` stored: its position.
fn put(s: &Store, n: &Node) -> u64 {
    s.append(&[n.record().unwrap()]).unwrap()[0]
}

/// The session as the store holds it: each node at its own position.
fn transcript(s: &Store, nodes: &[Node]) -> Vec<(u64, crate::stub::Stub)> {
    (nodes.iter())
        .map(|n| (put(s, n), n.clone().into()))
        .collect()
}

fn reply(texts: &[&str]) -> Node {
    let think = json!({"type": "thinking", "thinking": "the keeper asks about paint"});
    let mut blocks = vec![think];
    blocks.extend(texts.iter().map(|t| json!({"type": "text", "text": t})));
    Node::assistant(
        SID,
        "turn_1",
        0,
        Body::AssistantMessage {
            blocks,
            model: "m".into(),
            provider: "p".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    )
}

fn result(content: &str) -> Node {
    Node::tool_result(
        SID,
        None,
        None,
        Body::ToolResult {
            tool_use_id: "tu_1".into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: content.into(),
            correlation_id: None,
            bytes_total: 0,
            truncated: false,
            full_ref: None,
            duration_ms: None,
            late: false,
            meta: Value::Null,
            image: None,
            external: None,
            payload: None,
            parts: Vec::new(),
        },
    )
}

fn user(text: &str) -> Node {
    let mut n = Node::user(SID, None, "cli", text);
    n.created_at_ms = AT_MS;
    n
}

fn input(v: Value) -> Input {
    serde_json::from_value(v).unwrap()
}

/// The arrangement node a task's session stores for `pieces`.
fn node_of(pieces: &[Piece]) -> Node {
    Node::arrangement(
        "ses_task",
        "session:ses_northlight",
        stored(pieces.to_vec()),
        false,
    )
}

/// The pieces of `n` as the child's compile lays them.
fn laid_pieces(s: &Store, n: &Node) -> Vec<Piece> {
    match laid(s, n).body {
        Body::Arrangement { pieces, .. } => pieces,
        b => panic!("not an arrangement: {b:?}"),
    }
}

/// The stored arrangement holds each quoted node's words only by reference: no word of a message or a reply past
/// its first line (which the task's surfaces show) is in the record's bytes.
#[test]
fn a_piece_stores_no_words_of_its_message() {
    let (_d, s) = store();
    let t = transcript(&s, &[user(ASKED), reply(&[SAID])]);
    let a = input(json!({"pieces": [
        {"quote": "signal white, two coats", "role": "objective"},
        {"quote": "sand the walls between", "role": "design"}]}));
    let pieces = resolve(&a, &t, None, &Cuts::none()).unwrap();
    let n = node_of(&pieces);
    let Body::Arrangement { pieces: kept, .. } = &n.body else {
        unreachable!()
    };
    for p in kept {
        assert_eq!(p.text, None, "{p:?}");
        assert!(!p.parts.is_empty(), "{p:?}");
    }
    let bytes = serde_json::to_string(&n).unwrap();
    for words in ["signal white", "marine enamel", "sand the walls"] {
        assert!(!bytes.contains(words), "{words}: {bytes}");
    }
    assert!(
        bytes.contains(&t[0].1.id) && bytes.contains(&t[1].1.id),
        "{bytes}"
    );
}

/// What the child's compile makes of a piece kept by reference is byte for byte what it made of the piece's
/// words: a message, a reply of two text blocks, and messages long enough to be cut, with a known value at the
/// cut and without.
#[test]
fn piece_parts_render_byte_identical() {
    let long = (0..2_000)
        .map(|i| format!("line {i} of the keeper's log"))
        .collect::<Vec<_>>()
        .join("\n");
    let valued = (0..900)
        .map(|i| format!("coat {i} {VALUE}"))
        .collect::<Vec<_>>()
        .join(" ");
    let scrubber = Scrubber::with_values(vec![(VALUE.to_string(), "v0".to_string())]);
    for cuts in [Cuts::none(), scrubber.cuts()] {
        let (_d, s) = store();
        let t = transcript(
            &s,
            &[
                user(ASKED),
                reply(&["I will sand", " the walls between the coats."]),
                user(&format!("LOG {long}")),
                user(&format!("VALUED {valued}")),
            ],
        );
        let a = input(json!({"pieces": [
            {"quote": "signal white, two coats", "role": "objective"},
            {"quote": "sand the walls between", "role": "design"},
            {"quote": "LOG line 0", "role": "context"},
            {"quote": "VALUED coat 0", "role": "context"}]}));
        let pieces = resolve(&a, &t, None, &cuts).unwrap();
        assert!(pieces.iter().all(|p| !p.parts.is_empty()));
        assert!(
            pieces[2].text.as_deref().unwrap().contains("not shown"),
            "the long one was cut"
        );
        let then = render(&pieces);
        let now = render(&laid_pieces(&s, &node_of(&pieces)));
        assert_eq!(now, then);
    }
}

/// The child reads a piece's words from its node as first written; once that node is erased, the piece shows the
/// renderer's fixed line in place of its words, and the other pieces stay as they were.
#[test]
fn a_piece_renders_its_quote_and_an_erased_one_does_not() {
    let (_d, s) = store();
    let asked = user(ASKED);
    let t = transcript(&s, &[asked.clone(), reply(&[SAID])]);
    let a = input(json!({"pieces": [
        {"quote": "signal white, two coats", "role": "objective"},
        {"quote": "sand the walls between", "role": "design"}]}));
    let n = node_of(&resolve(&a, &t, None, &Cuts::none()).unwrap());
    let before = render(&laid_pieces(&s, &n));
    assert!(before.contains(ASKED) && before.contains(SAID), "{before}");
    put(&s, &asked.erased(AT_MS, "a test"));
    let after = render(&laid_pieces(&s, &n));
    assert!(!after.contains("signal white"), "{after}");
    assert!(
        after.contains(&format!(" ---\n{ERASED_QUOTE}\n\n--- Piece 2 of 2")),
        "{after}"
    );
    assert!(after.ends_with(&format!(" ---\n{SAID}")), "{after}");
}

/// A piece of a tool result keeps its words, as every piece did before: no field of a quote reads a result.
#[test]
fn a_tool_results_piece_keeps_its_words() {
    let (_d, s) = store();
    let t = transcript(&s, &[user(ASKED), result(READ)]);
    let a = input(json!({"pieces": [{"quote": "enamel: 4 tins", "role": "objective"}]}));
    let pieces = stored(resolve(&a, &t, None, &Cuts::none()).unwrap());
    assert!(pieces[0].parts.is_empty(), "{:?}", pieces[0]);
    assert_eq!(pieces[0].text.as_deref(), Some(READ));
}

/// A piece kept by reference that a reader lays without reading its words shows the renderer's fixed line: never
/// nothing, and never as if it were superseded.
#[test]
fn a_piece_whose_words_were_not_read_shows_the_fixed_line() {
    let (_d, s) = store();
    let t = transcript(&s, &[user(ASKED)]);
    let a = input(json!({"pieces": [{"quote": "signal white, two coats", "role": "objective"}]}));
    let n = node_of(&resolve(&a, &t, None, &Cuts::none()).unwrap());
    let Body::Arrangement { pieces, .. } = n.body else {
        unreachable!()
    };
    let out = render(&pieces);
    let line = format!(" ---\n{UNREADABLE_QUOTE}");
    assert!(out.ends_with(&line), "{out}");
    assert!(!out.contains("): superseded"), "{out}");
}

/// A look back's result kept by reference stores no words, so where a task's quote is matched it reads as its
/// rendering (issue 49): a quote of the words it showed finds it, and its piece keeps those words, as a result's
/// piece does.
#[test]
fn a_quote_of_a_look_back_kept_by_reference_finds_it_by_its_rendering() {
    let (_d, s) = store();
    let said = user(ASKED);
    let at = put(&s, &said);
    let mut look = result("");
    if let Body::ToolResult { tool, parts, .. } = &mut look.body {
        *tool = crate::view::history::HISTORY_TOOL.into();
        *parts = vec![
            Part::Code("- ".into()),
            Part::Quote(Quote {
                node: said.id,
                position: at,
                field: Field::Text,
                from: 0,
                to: ASKED.len() as u32,
                form: Form::Flat1,
            }),
        ];
    }
    let id = look.id.clone();
    let t = transcript(&s, &[look]);
    let a = input(json!({"pieces": [{"quote": "signal white, two coats", "role": "objective"}]}));
    let pieces = resolve_shown(&s, &a, &t, None, &Cuts::none()).unwrap();
    assert_eq!(pieces[0].node, id);
    let text = pieces[0].text.clone().expect("the piece's words");
    assert!(text.contains("signal white, two coats"), "{text}");
}
