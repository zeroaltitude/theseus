//! Old results cleared by tokens (theseus-5lzox): the trigger and the keep,
//! the small window's line, the stub's range and note, and a model-free
//! replay of a long bug hunt through the real compiler.

use serde_json::{json, Value};

use super::tests_ring::{answer, closed, result, spec};
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

/// A session of `n` reads of `tokens` each on a 1M window, appended one
/// exchange at a time, as a turn loops. Each loop's compile is given the
/// last one's compilation; what it makes is returned in order.
fn long_session(n: usize, tokens: u64, window: u64) -> Vec<Compiled> {
    let per_byte = per_byte();
    let mut nodes: Vec<(u64, Stub)> =
        vec![(1, Node::user("s", None, "web", "find the bugs").into())];
    let mut out: Vec<Compiled> = Vec::new();
    for k in 0..n {
        let id = format!("c{k}");
        let p = nodes.len() as u64;
        let text = format!("Checked file {k} and found nothing wrong there. More words follow.");
        nodes.push((
            p + 1,
            text_answer(&text, vec![read_call(&id, &format!("src/f{k}.rs"))]).into(),
        ));
        let body = read_of(tokens, per_byte, 400, k as u64);
        nodes.push((p + 2, result(&id, &body).into()));
        let c = compile_nodes(&nodes, out.last().map(|c| &c.compilation), window);
        out.push(c);
    }
    out
}

fn compile_nodes(nodes: &[(u64, Stub)], current: Option<&Compilation>, window: u64) -> Compiled {
    compile_capped(nodes, current, window, 128_000)
}

/// `compile_nodes` for a model whose output cap is `max_tokens`.
fn compile_capped(
    nodes: &[(u64, Stub)],
    current: Option<&Compilation>,
    window: u64,
    max_tokens: u32,
) -> Compiled {
    let last = nodes.last().map_or(0, |(p, _)| *p);
    let mut s = spec();
    s.model = "claude-sonnet-5-5".into();
    s.max_tokens = max_tokens;
    compile(CompileInput {
        session_id: "s",
        current,
        nodes,
        last_position: last,
        spec: &s,
        catalog: &Catalog::builtin(),
        force: None,
        window_override: Some(window),
        blobs: None,
        hidden: &[],
        strip: None,
        overflowed: None,
        sources: &Default::default(),
        signals: None,
        assembled: None,
        situation: &situation::Situation::Continuation,
        kept: None,
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

/// A long session of 12-25K-token reads on a 1M window clears at about
/// 150K tokens of results, not at 50 % of the budget (about 430K), each
/// pass leaves about 40K of results, and the appends between passes render
/// the same prefix byte for byte (the cache holds).
#[test]
fn a_long_session_clears_at_the_token_line_and_keeps_its_prefix_between_passes() {
    let cs = long_session(60, 15_000, 1_000_000);
    let passes: Vec<usize> = cs
        .iter()
        .enumerate()
        .filter(|(_, c)| c.cleared.is_some())
        .map(|(i, _)| i)
        .collect();
    assert!(passes.len() >= 3, "{passes:?}");
    let first = cs[passes[0]].cleared.unwrap();
    assert!(
        first.tokens_before > CLEAR_AT_TOKENS_FLOOR && first.tokens_before < 200_000,
        "the first pass is at the token line, not the percent line: {first:?}"
    );
    for &i in &passes {
        let pass = cs[i].cleared.unwrap();
        assert!(pass.tokens_after < 110_000, "{pass:?}");
        assert!(pass.tokens_after < pass.tokens_before);
    }
    // Between passes: each request begins with the one before it.
    for w in cs
        .windows(2)
        .enumerate()
        .filter(|(i, _)| !passes.contains(&(i + 1)))
    {
        let (a, b) = (&w.1[0].request.messages, &w.1[1].request.messages);
        assert_eq!(&b[..a.len() - 1], &a[..a.len() - 1], "loop {}", w.0 + 1);
        assert_eq!(b[a.len() - 1]["role"], a[a.len() - 1]["role"]);
    }
    for c in &cs {
        closed(c).unwrap();
        assert!(c.est_tokens < 434_000, "never past the percent line");
    }
}

/// The token line a first pass must pass (the test's own floor, below
/// `clearing::CLEAR_AT_TOKENS` by the request's overhead).
const CLEAR_AT_TOKENS_FLOOR: u64 = 120_000;

/// A 200K-window session clears no later than at 50 % of its budget: the
/// token line alone would wait for a request the window cannot hold.
#[test]
fn a_small_window_clears_no_later_than_at_the_percent_line() {
    let (window, per_byte) = (200_000, per_byte());
    let budget = window - 1_000 - 4_096;
    let mut nodes: Vec<(u64, Stub)> = vec![(1, Node::user("s", None, "web", "go").into())];
    let mut out: Vec<Compiled> = Vec::new();
    for k in 0..40 {
        let id = format!("c{k}");
        let p = nodes.len() as u64;
        let text = format!("Read {k}.");
        nodes.push((
            p + 1,
            text_answer(&text, vec![read_call(&id, "src/a.rs")]).into(),
        ));
        let body = read_of(3_000, per_byte, 100, k as u64);
        nodes.push((p + 2, result(&id, &body).into()));
        out.push(compile_capped(
            &nodes,
            out.last().map(|c| &c.compilation),
            window,
            1_000,
        ));
    }
    let first = out.iter().find_map(|c| c.cleared).expect("a pass");
    assert!(
        first.tokens_before > budget * 50 / 100 && first.tokens_before < budget * 60 / 100,
        "{first:?}: at the percent line"
    );
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

/// A session for the percent line's guard: `large` reads of about 10K tokens
/// each (the oldest), then results of 1,900 characters (never cleared) that
/// sum to about `small` tokens, after a first message sized so that the
/// request, before any pass, is `target` tokens by the estimator's own count
/// (the estimator charges each call's ids and framing, so the sizes alone
/// would miss). With the nodes, the large reads' ids. The small results come
/// last, or first when `small_first`, which leaves the large reads the newest.
fn mixed_session(
    target: u64,
    large: usize,
    small: u64,
    small_first: bool,
) -> (Vec<(u64, Stub)>, Vec<String>) {
    let pb = per_byte();
    let build = |lead: u64| {
        let mut nodes: Vec<(u64, Stub)> = Vec::new();
        let mut ids = Vec::new();
        let mut push = |n: Node| nodes.push((nodes.len() as u64 + 1, n.into()));
        push(Node::user(
            "s",
            None,
            "web",
            &"word ".repeat((lead as f64 / pb) as usize / 5 + 1),
        ));
        let order = if small_first {
            [false, true]
        } else {
            [true, false]
        };
        for large_now in order {
            match large_now {
                true => {
                    for k in 0..large {
                        let id = format!("L{k}");
                        let text = format!("Read large file {k} and found nothing wrong there.");
                        push(text_answer(
                            &text,
                            vec![read_call(&id, &format!("src/l{k}.rs"))],
                        ));
                        let r = result(&id, &read_of(10_000, pb, 400, k as u64));
                        ids.push(r.id.clone());
                        push(r);
                    }
                }
                false => {
                    for k in 0..(small as f64 / (1_900.0 * pb)).round() as usize {
                        let id = format!("S{k}");
                        push(text_answer(
                            "Checked a short file.",
                            vec![read_call(&id, &format!("src/s{k}.rs"))],
                        ));
                        push(result(&id, &format!("{k:>6}\t{}", "y".repeat(1_890))));
                    }
                }
            }
        }
        (nodes, ids)
    };
    // The request before a pass: the estimate of a compile that made none,
    // else the pass's own `tokens_before`.
    let before = |nodes: &[(u64, Stub)]| {
        let c = compile_nodes(nodes, None, 1_000_000);
        c.cleared.map_or(c.est_tokens, |p| p.tokens_before)
    };
    let without = before(&build(0).0);
    build(target.saturating_sub(without))
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

/// Past the percent line the new rule never leaves a request larger than the
/// 20-exchange rule did (theseus-5lzox join fix 1). A 1M window holds a
/// request of 480K tokens (past its 50 % line, 434K): 14 old reads of 10K, 160K
/// tokens of results under 2,000 characters, which no pass clears but which the
/// token rule's guard counts, and the rest first message and framing. The old
/// rule cleared the 14 reads and ended under its 42.5 % line (369K). The token
/// rule alone wants the pass to end about 95K above the request's other
/// content, which the small results alone pass, so it refuses, and the request
/// climbs to the ring at 100 %: the 868K failure the rule exists to remove.
/// Past the percent line a refused pass is tried again keeping only the newest
/// exchange whole, and taken under the old rule's line.
#[test]
fn past_the_percent_line_small_results_do_not_stop_the_pass_the_old_rule_made() {
    let (nodes, large) = mixed_session(480_000, 14, 160_000, false);
    let c = compile_nodes(&nodes, None, 1_000_000);
    let budget = 1_000_000 - 128_000 - 4_096;
    assert!(
        c.cleared.is_some() || c.est_tokens > budget * 50 / 100,
        "setup: the session must pass the percent line ({} of {})",
        c.est_tokens,
        budget * 50 / 100
    );
    let pass = c.cleared.unwrap_or_else(|| {
        panic!(
            "no pass at {} tokens, past the percent line {}: the request would climb to the ring",
            c.est_tokens,
            budget * 50 / 100
        )
    });
    assert!(pass.tokens_before > budget * 50 / 100, "{pass:?}");
    assert!(
        pass.tokens_after <= budget * (clearing::CLEAR_AT + clearing::CLEAR_TO) / 200,
        "{pass:?}: ends under the old rule's line"
    );
    assert_eq!(cleared_ids(&c), large, "the old reads, and no small result");
    let text = serde_json::to_string(&c.request.messages).unwrap();
    assert_eq!(text.matches("Result cleared to save context").count(), 14);
    closed(&c).unwrap();
}

/// The retry keeps only the newest exchange whole, not the token rule's 40K:
/// that is what makes it clear everything the 20-exchange rule could. Here the
/// small results are the oldest and the 14 large reads the newest, so a keep of
/// 40K would hold the newest four whole and end above the old rule's line,
/// while keep 0 clears 13 of the 14 (the newest exchange's read stays) and ends
/// under it.
#[test]
fn the_retry_clears_everything_but_the_newest_exchange() {
    let (nodes, large) = mixed_session(470_000, 14, 150_000, true);
    let c = compile_nodes(&nodes, None, 1_000_000);
    let budget = 1_000_000 - 128_000 - 4_096;
    let pass = c.cleared.unwrap_or_else(|| {
        panic!(
            "no pass at {} tokens, past the percent line {}",
            c.est_tokens,
            budget * 50 / 100
        )
    });
    assert!(pass.tokens_before > budget * 50 / 100, "{pass:?}");
    assert!(
        pass.tokens_after <= budget * (clearing::CLEAR_AT + clearing::CLEAR_TO) / 200,
        "{pass:?}: ends under the old rule's line"
    );
    assert_eq!(
        cleared_ids(&c),
        large[..13].to_vec(),
        "every large read but the newest exchange's"
    );
    closed(&c).unwrap();
}

/// The retry is for the percent line only: under it the old rule did not
/// clear, so a refused token pass stays refused and is not taken at keep 0
/// (which would clear what the keep exists to hold).
#[test]
fn under_the_percent_line_a_refused_token_pass_is_not_retried_at_keep_zero() {
    let (nodes, _) = mixed_session(390_000, 10, 160_000, false);
    let c = compile_nodes(&nodes, None, 1_000_000);
    let budget = 1_000_000 - 128_000 - 4_096;
    assert!(
        c.est_tokens > 150_000 && c.est_tokens < budget * 50 / 100,
        "setup: past the token line, under the percent line ({})",
        c.est_tokens
    );
    assert!(c.cleared.is_none(), "{:?}", c.cleared);
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
