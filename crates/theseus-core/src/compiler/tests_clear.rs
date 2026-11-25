//! Old results cleared by tokens (theseus-5lzox), on the view's compile (issue 44): the newest exchange's
//! results stay whole, and the stub's range and note.

use serde_json::{json, Value};

use super::tests_parts::{answer, closed, result, spec};
use super::*;
use crate::stub::Stub;

/// Bytes of a numbered read that the estimator counts as `tokens`.
fn read_of(tokens: u64, per_byte: f64, lines: usize, seed: u64) -> String {
    let width = ((tokens as f64 / per_byte) / lines as f64).max(12.0) as usize;
    (1..=lines)
        .map(|i| {
            let body: String = (0..width.saturating_sub(8))
                .map(|k| (b'a' + ((i as u64 * 7 + k as u64 * 13 + seed) % 26) as u8) as char)
                .collect();
            format!("{i:>6}\t{body}\n")
        })
        .collect()
}

fn read_call(id: &str, path: &str) -> Value {
    json!({"type": "tool_use", "id": id, "name": "fs_read", "input": {"path": path}})
}

fn text_answer(text: &str, calls: Vec<Value>) -> Node {
    let mut blocks = vec![json!({"type": "text", "text": text})];
    blocks.extend(calls);
    answer(blocks)
}

fn compile_nodes(nodes: &[(u64, Stub)], current: Option<&Compilation>, window: u64) -> Compiled {
    let max_tokens = 128_000;
    let last = nodes.last().map_or(0, |(p, _)| *p);
    let mut s = spec();
    s.model = "claude-sonnet-5-5".into();
    s.max_tokens = max_tokens;
    super::tests_parts::on_view(CompileInput {
        session_id: "s",
        current,
        nodes,
        last_position: last,
        spec: &s,
        catalog: &Catalog::builtin(),
        force: false,
        window_override: Some(window),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        situation: &situation::Situation::Continuation,
    })
}

/// The estimator's tokens per request byte, measured on a plain request.
fn per_byte() -> f64 {
    let nodes = vec![(
        1,
        Node::user("s", None, "web", &"word ".repeat(20_000)).into(),
    )];
    let c = compile_nodes(&nodes, None, 1_000_000);
    c.est_tokens as f64 / c.request.json_bytes() as f64
}

/// An `fs_read` stub says the lines its result held (a window's footer, not
/// the call's input), the file's length, and the model's note, cut at a word.
#[test]
fn a_reads_stub_names_the_lines_its_result_held_and_the_note_is_cut_on_a_word() {
    let per_byte = per_byte();
    let mut body = read_of(2_000, per_byte, 400, 1);
    body.push_str("[showing lines 1-400 of 1210; pass offset=401 to read on]\n");
    let r = result("c1", &body);
    let input = json!({"path": "src/big.rs"});
    let s = clearing::stub_noted("fs_read", &input, &r, Some("it only wraps the store"));
    assert!(
        s.starts_with(
            "[Result cleared to save context: fs_read src/big.rs, lines 1-400 of 1,210 ("
        ),
        "{s}"
    );
    assert!(
        s.ends_with("After it you wrote: \"it only wraps the store\".]"),
        "{s}"
    );
    // The same result with a call that names a range: the result decides.
    let with = json!({"path": "src/big.rs", "offset": 5, "limit": 10});
    assert_eq!(
        clearing::stub_noted("fs_read", &with, &r, None)
            .split(" (")
            .next(),
        Some("[Result cleared to save context: fs_read src/big.rs, lines 1-400 of 1,210")
    );
    // A note is the first sentence, at most 120 characters, cut at a word.
    let long = "alpha ".repeat(40);
    let note = clearing::remember::note_of(&[json!({"type": "text", "text": long})]).unwrap();
    assert!(
        note.chars().count() <= 121 && note.ends_with("alpha…"),
        "{note}"
    );
    let two =
        clearing::remember::note_of(&[json!({"type": "text", "text": "Read it.  Then more."})]);
    assert_eq!(two.as_deref(), Some("Read it."));
    assert!(clearing::remember::note_of(&[json!({"type": "tool_use", "id": "x"})]).is_none());
}

/// A note's last space inside a known value is no place to cut: the note
/// stops before the value.
#[test]
fn a_notes_last_space_inside_a_known_value_is_no_place_to_cut() {
    let spaced = "alpha-0417 beta-9932";
    let board = crate::secrets::SecretBoard::empty();
    board.hold("spaced", crate::secrets::Secret::new(spaced.into()));
    let text = format!("{} {spaced}{}", "w".repeat(90), "q".repeat(60));
    let note = clearing::remember::note_of(&[json!({"type": "text", "text": text})]).unwrap();
    assert_eq!(note, format!("{}…", "w".repeat(90)));
    crate::tests_cut_sites::whole_or_absent("the note", &note, spaced);
}

/// The ids of the results a compilation cleared, in order.
fn cleared_ids(c: &Compiled) -> Vec<String> {
    c.budget
        .dropped
        .iter()
        .filter(|d| d.tier == "cleared")
        .map(|d| d.node_id.clone().unwrap())
        .collect()
}

/// The newest exchange's results stay whole however large, and the older ones
/// are cleared (theseus-5lzox join fix 2): a pass over six reads of 25K tokens
/// and a newest read of 60K, over the 40K keep. The floor is `pass`'s own
/// (`*p < last_answer`), which the no-pass test never reaches: it has no older
/// result, so `any_old` stops it first.
#[test]
fn a_newest_result_over_the_keep_stays_whole_and_the_older_ones_clear() {
    let pb = per_byte();
    let mut nodes: Vec<(u64, Stub)> =
        vec![(1, Node::user("s", None, "web", "find the bugs").into())];
    let (mut older, mut newest) = (Vec::new(), String::new());
    for k in 0..7 {
        let (id, tokens) = (format!("c{k}"), if k < 6 { 25_000 } else { 60_000 });
        let p = nodes.len() as u64;
        let text = format!("Checked file {k} and found nothing wrong there.");
        nodes.push((
            p + 1,
            text_answer(&text, vec![read_call(&id, &format!("src/f{k}.rs"))]).into(),
        ));
        let r = result(&id, &read_of(tokens, pb, 400, k as u64));
        match k < 6 {
            true => older.push(r.id.clone()),
            false => newest = r.id.clone(),
        }
        nodes.push((p + 2, r.into()));
    }
    let c = compile_nodes(&nodes, None, 1_000_000);
    let pass = c.cleared.expect("a pass at the token line");
    assert!(pass.tokens_after < pass.tokens_before, "{pass:?}");
    let ids = cleared_ids(&c);
    assert!(
        !ids.contains(&newest),
        "the newest exchange's result was cleared: {ids:?}"
    );
    assert_eq!(ids, older, "the six older reads, oldest first");
    let text = serde_json::to_string(&c.request.messages).unwrap();
    assert_eq!(text.matches("Result cleared to save context").count(), 6);
    closed(&c).unwrap();
}
