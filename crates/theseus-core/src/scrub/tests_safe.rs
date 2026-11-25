//! The value-aware cuts (`scrub::safe`) through the live board: a head, a
//! tail, both ends, a byte head and a first line each keep a known value
//! whole or drop it, in every form the scrubber reads. The plain forms read
//! every board alive in the process; `cargo nextest` gives each test its own
//! process, so a test here sees only its own boards. All values invented.

use std::time::Instant;

use super::safe::{ends, first_line, head, head_bytes, head_marked, tail};
use super::*;
use crate::secrets::Secret;
use crate::tests_cut_sites::{knowing_core, whole_or_absent};

/// A value with characters JSON escapes, so its escaped form differs.
const V: &str = "kqzv\"0417\\wjxp-9932-hmfb";
const W: &str = "plum-ladder-5521-orbit";

fn b64(s: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(s)
}

fn percent(s: &str) -> String {
    s.bytes().map(|c| format!("%{c:02X}")).collect()
}

/// The value verbatim, in base64, percent-encoded and JSON-escaped.
fn forms(value: &str) -> [(&'static str, String); 4] {
    [
        ("verbatim", value.to_string()),
        ("base64", b64(value)),
        ("percent", percent(value)),
        ("escaped", serde_json::to_string(value).unwrap()),
    ]
}

/// A board alive in the process that holds `value`: every plain cut knows it
/// while the board is kept.
fn live_board(value: &str) -> Arc<SecretBoard> {
    let board = SecretBoard::empty();
    board.hold("cut_test", Secret::new(value.into()));
    board
}

/// The spans the scrubber finds for `values` in `text`, merged.
fn spans_of(text: &str, values: &[&str]) -> Vec<(usize, usize)> {
    let sub = Substitution::new(values.iter().map(|v| (*v, "v")).collect());
    merged(text, sub.spans(text))
        .0
        .into_iter()
        .map(|(a, b, _)| (a, b))
        .collect()
}

/// Byte `at` of `text` lies inside no span of `value`, in any form.
fn outside(what: &str, text: &str, at: usize, value: &str) {
    for (s, e) in spans_of(text, &[value]) {
        assert!(!(s < at && at < e), "{what}: {at} inside ({s}, {e})");
    }
}

/// Byte `at` lies inside a span of `value`: a plain cut there would split it.
fn inside(text: &str, at: usize, value: &str) -> bool {
    spans_of(text, &[value])
        .iter()
        .any(|(s, e)| *s < at && at < *e)
}

fn byte_of(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(i, _)| i)
}

#[test]
fn safe_head_never_ends_inside_a_value_in_any_form() {
    let _board = live_board(V);
    for (form, enc) in forms(V) {
        let text = format!("{}{enc} after", ".".repeat(40));
        let max = 40 + enc.chars().count() / 2;
        assert!(
            inside(&text, byte_of(&text, max), V),
            "{form}: a plain cut splits it"
        );
        let (kept, cut) = head(&text, max);
        assert!(cut, "{form}");
        assert!(text.starts_with(kept));
        outside(form, &text, kept.len(), V);
        whole_or_absent(form, kept, V);
    }
}

#[test]
fn safe_head_marked_marks_a_cut_that_stops_before_a_value() {
    let _board = live_board(V);
    let text = format!("{}{V} after", "x".repeat(50));
    let out = head_marked(&text, 60);
    assert!(out.ends_with('…'), "{out:?}");
    whole_or_absent("the marked head", &out, V);
    assert_eq!(head_marked("short", 60), "short");
}

#[test]
fn safe_tail_never_starts_inside_a_value() {
    let _board = live_board(V);
    for (form, enc) in forms(V) {
        let text = format!("before {enc}{}", ".".repeat(40));
        let max = 40 + enc.chars().count() / 2;
        let at = text.len()
            - text
                .chars()
                .rev()
                .take(max)
                .map(char::len_utf8)
                .sum::<usize>();
        assert!(inside(&text, at, V), "{form}: a plain cut splits it");
        let (kept, cut) = tail(&text, max);
        assert!(cut, "{form}");
        assert!(text.ends_with(kept));
        outside(form, &text, text.len() - kept.len(), V);
        whole_or_absent(form, kept, V);
    }
}

#[test]
fn safe_ends_keep_each_value_whole_or_drop_it() {
    let board = live_board(V);
    board.hold("cut_test_2", Secret::new(W.into()));
    let text = format!(
        "{}{V}{}{W}{}",
        ".".repeat(30),
        "-".repeat(200),
        ".".repeat(30)
    );
    let head_chars = 30 + V.chars().count() / 2;
    let tail_chars = 30 + W.chars().count() / 2;
    let (h, t, cut) = ends(&text, head_chars, tail_chars);
    assert!(cut);
    assert!(text.starts_with(h) && text.ends_with(t));
    for (what, piece) in [("the head", h), ("the tail", t)] {
        whole_or_absent(what, piece, V);
        whole_or_absent(what, piece, W);
    }
    assert_eq!(ends("short", 3, 3), ("short", "", false));
}

#[test]
fn safe_head_bytes_stops_at_a_char_and_a_value_edge() {
    let _board = live_board(V);
    let text = format!("\u{e9}\u{3bb}\u{1f600}{V}\u{3bb} after");
    let start = text.find(V).unwrap();
    // Inside a character, then inside the value.
    for max in [1, 3, start + 5, start + V.len() - 1] {
        let (kept, cut) = head_bytes(&text, max);
        assert!(cut && kept.len() <= max, "{max}");
        outside("a byte head", &text, kept.len(), V);
        whole_or_absent("a byte head", kept, V);
    }
    assert_eq!(head_bytes(&text, text.len()), (text.as_str(), false));
}

/// Random text with a value in every form the scrubber reads, at random
/// places: every byte budget gives a head on a character boundary, no
/// longer than the budget, ending outside every span of the value.
#[test]
fn safe_head_bytes_property_never_panics_and_never_ends_inside_a_value() {
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut draw = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };
    let alphabet: Vec<char> = "abcXYZ019-%\\\" \n\u{e9}\u{3bb}\u{1f600}/"
        .chars()
        .collect();
    let board = SecretBoard::empty();
    let mut tried = 0;
    for _ in 0..60 {
        let len = 8 + draw(25);
        let value: String = (0..len).map(|_| alphabet[draw(alphabet.len())]).collect();
        if value.trim().len() < MIN_VALUE || value.trim() != value {
            continue;
        }
        board.hold("p", Secret::new(value.clone()));
        let mut text = String::new();
        for (_, form) in forms(&value) {
            for _ in 0..draw(8) {
                text.push(alphabet[draw(alphabet.len())]);
            }
            text.push_str(&form);
        }
        text.push_str(" end");
        for max in 0..=text.len() + 3 {
            let (kept, cut) = head_bytes(&text, max);
            assert_eq!(cut, text.len() > max);
            if cut {
                assert!(kept.len() <= max, "{max}");
            } else {
                assert_eq!(kept, text);
            }
            assert!(text.starts_with(kept));
            outside("a byte head", &text, kept.len(), &value);
        }
        tried += 1;
    }
    assert!(tried > 20, "only {tried} values tried");
}

#[test]
fn safe_first_line_never_ends_inside_a_value() {
    let _board = live_board(V);
    let text = format!("  {}{V} and on\nsecond", "x".repeat(50));
    let (line, cut) = first_line(&text, 60, 60);
    assert!(cut);
    whole_or_absent("the first line", line, V);
    assert_eq!(first_line("one\ntwo", 60, 60), ("one", false));
}

#[test]
fn a_live_board_is_known_to_every_cut_and_a_dropped_one_is_forgotten() {
    let text = format!("{}{V} after", "x".repeat(50));
    let board = live_board(V);
    whole_or_absent("a head with the board", head(&text, 60).0, V);
    drop(board);
    // Forgotten: the cut falls where it is asked again.
    assert_eq!(head(&text, 60).0.chars().count(), 60);
}

#[test]
fn with_no_live_board_the_cut_is_the_plain_cut() {
    let text = format!("{}{V}\u{3bb} after\nmore", "x".repeat(50));
    let plain: String = text.chars().take(60).collect();
    assert_eq!(head(&text, 60), (plain.as_str(), true));
    assert_eq!(head_marked(&text, 60), format!("{plain}…"));
    let n = text.chars().count();
    let last: String = text.chars().skip(n - 10).collect();
    assert_eq!(tail(&text, 10), (last.as_str(), true));
    let (h, t, cut) = ends(&text, 60, 10);
    assert_eq!((h, t, cut), (plain.as_str(), last.as_str(), true));
    assert_eq!(head_bytes(&text, 60), (&text[..60], true));
    let (line, cut) = first_line(&text, 10, 10);
    assert_eq!((line, cut), ("xxxxxxxxxx", true));
}

#[test]
fn a_resolving_board_moves_the_cut_to_whitespace() {
    let board = SecretBoard::new(["late".to_string()], Instant::now());
    let text = "alpha beta-gamma-delta";
    assert_eq!(head(text, 9), ("alpha ", true));
    // Once nothing is loading, the cut falls where it is asked.
    board.withdraw("late", "not found");
    assert_eq!(head(text, 9), ("alpha bet", true));
}

/// What `Cuts::live` knows is what the core's own scrubber knows, on a core
/// whose board holds the value (the test core's scrubber is given it
/// directly) and on the daemon's shape (a scrubber reading the board).
#[tokio::test]
async fn live_cuts_and_a_cores_scrubber_agree_on_a_knowing_core() {
    let r = knowing_core(V, |_| {}, |_| crate::provider::Scripted::text("Noted."));
    let daemon = Scrubber::from_board(r.core.secrets.clone());
    let mut text = String::from("lead ");
    for (_, form) in forms(V) {
        text.push_str(&form);
        text.push_str(" \u{3bb} ");
    }
    let (live, core, board) = (Cuts::live(), r.core.tools.scrubber.cuts(), daemon.cuts());
    for at in 0..=text.len() + 2 {
        let want = (core.before(&text, at), core.after(&text, at));
        assert_eq!(
            (live.before(&text, at), live.after(&text, at)),
            want,
            "at {at}"
        );
        assert_eq!(
            (board.before(&text, at), board.after(&text, at)),
            want,
            "at {at}"
        );
    }
    assert!(core.before(&text, 8) < 8, "the core knows the value");
}

/// One `head_bytes` of a 1 MB text through the live board with twenty known
/// values, one across the cut. Printed for the record; the bound is the
/// guard.
#[test]
fn a_head_bytes_of_a_megabyte_with_twenty_values_is_measured() {
    let board = SecretBoard::empty();
    let values: Vec<String> = (0..20)
        .map(|n| format!("fixture-{n:02}-made-up-value-0123"))
        .collect();
    for (n, v) in values.iter().enumerate() {
        board.hold(&format!("v{n}"), Secret::new(v.clone()));
    }
    let line = "   Compiling crate v0.1.0 (/work/src) 100% GET /v1/items?id=42&page=3 caf\u{e9}\n";
    let mut text = String::new();
    let mut n = 0;
    while text.len() < 1 << 20 {
        text.push_str(line);
        if text.len() % 50_000 < line.len() {
            text.push_str(&values[n % 20]);
            n += 1;
        }
    }
    let max = 1 << 19;
    let mut at = max - values[0].len() / 2;
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    text.insert_str(at, &values[0]);
    let began = Instant::now();
    let (kept, cut) = head_bytes(&text, max);
    let took = began.elapsed();
    eprintln!(
        "safe: head_bytes of a {} byte text, twenty values: {took:?}",
        text.len()
    );
    assert!(cut && kept.len() <= at, "{} of {at}", kept.len());
    outside("the byte head", &text, kept.len(), &values[0]);
    assert!(took < std::time::Duration::from_secs(30), "took {took:?}");
    drop(board);
}
