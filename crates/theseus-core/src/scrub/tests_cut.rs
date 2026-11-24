//! Where a cut falls: never inside a span a known value takes, verbatim or
//! encoded, and at whitespace while a value is still loading. All values
//! invented.

use std::time::Instant;

use super::*;
use crate::secrets::Secret;

const V: &str = "kelp-ferry-0417-vault-token";

fn knowing(values: &[&str]) -> Scrubber {
    Scrubber::with_values(
        values
            .iter()
            .enumerate()
            .map(|(i, v)| ((*v).to_string(), format!("v{i}")))
            .collect(),
    )
}

/// The spans `Substitution` finds for `values` in `text`, merged.
fn spans_of(text: &str, values: &[String]) -> Vec<(usize, usize)> {
    let sub = Substitution::new(values.iter().map(|v| (v.as_str(), "v")).collect());
    merged(text, sub.spans(text))
        .0
        .into_iter()
        .map(|(a, b, _)| (a, b))
        .collect()
}

/// Every way the scrubber reads a value, each at a random place in filler.
#[test]
fn cuts_property_never_inside_a_known_span() {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut draw = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % bound as u64) as usize
    };
    let alphabet: Vec<char> = "abcXYZ019-%\\ \n\u{e9}\u{3bb}/".chars().collect();
    let b64 = |s: &str| base64::engine::general_purpose::STANDARD.encode(s);
    for _ in 0..40 {
        let len = 8 + draw(33);
        let value: String = (0..len).map(|_| alphabet[draw(alphabet.len())]).collect();
        if value.trim().len() < MIN_VALUE || value.trim() != value {
            continue;
        }
        let wrapped = {
            let long = b64(&format!("{}{value}{}", "q".repeat(draw(3)), "r".repeat(60)));
            long.as_bytes()
                .chunks(40)
                .map(|c| std::str::from_utf8(c).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let percent: String = value.bytes().map(|c| format!("%{c:02X}")).collect();
        let json = serde_json::to_string(&value).unwrap();
        // YAML's escaped line break, inside the value.
        let mid = floor_at(&value, value.len() / 2);
        let folded = format!("\"{}\\\n    {}\"", &value[..mid], &value[mid..]);
        let forms = [value.clone(), b64(&value), wrapped, percent, json, folded];
        let mut text = String::new();
        for form in &forms {
            text.push_str(&"x".repeat(1 + draw(12)));
            text.push(' ');
            text.push_str(form);
        }
        text.push_str(" tail");
        every_cut_falls_outside(&text, &value);
    }
}

/// Every cut `before` and `after` place in `text` lies on a character
/// boundary outside every span of `value`: tried at each span's edges and
/// the bytes beside them, and at every fifth byte. (`percent_spans` holds
/// every reading the percent oracle finds: `tests_overlap`.)
fn every_cut_falls_outside(text: &str, value: &str) {
    let cuts = knowing(&[value]).cuts();
    let spans = spans_of(text, &[value.to_string()]);
    let mut tried: Vec<usize> = (0..=text.len()).step_by(5).collect();
    for (s, e) in &spans {
        tried.extend((s.saturating_sub(2)..=s + 2).chain(e.saturating_sub(2)..=e + 2));
    }
    tried.sort_unstable();
    tried.dedup();
    let tried = tried
        .into_iter()
        .filter(|i| *i <= text.len() && text.is_char_boundary(*i));
    for at in tried {
        let (b, a) = (cuts.before(text, at), cuts.after(text, at));
        assert!(b <= at && a >= at, "at {at}: {b} {a}");
        assert!(text.is_char_boundary(b) && text.is_char_boundary(a));
        for (s, e) in &spans {
            assert!(
                !(*s < b && b < *e),
                "before {b} inside ({s}, {e}) of {text:?}"
            );
            assert!(
                !(*s < a && a < *e),
                "after {a} inside ({s}, {e}) of {text:?}"
            );
        }
    }
}

fn floor_at(s: &str, mut at: usize) -> usize {
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    at
}

#[test]
fn a_head_stops_before_a_value_and_a_tail_starts_after_it() {
    let cuts = knowing(&[V]).cuts();
    let text = format!("{}{V}{}", "x".repeat(50), "y".repeat(50));
    // A head of 60 would end inside the value: it ends before it.
    assert_eq!(cuts.head(&text, 60), ("x".repeat(50).as_str(), true));
    assert_eq!(cuts.head(&text, 50), ("x".repeat(50).as_str(), true));
    assert_eq!(cuts.head(&text, 1000), (text.as_str(), false));
    // A tail that would start inside it starts after it.
    assert_eq!(cuts.after(&text, 60), 50 + V.len());
    assert_eq!(cuts.after(&text, 40), 40);
    // A cut that knows no value falls where it is asked.
    assert_eq!(Cuts::none().head(&text, 60).0.chars().count(), 60);
}

#[test]
fn a_first_line_never_ends_inside_a_value() {
    let cuts = knowing(&[V, "kelp-one-0417\nferry-two-0417"]).cuts();
    let long = format!("  {}{V} and on\nsecond", "x".repeat(50));
    assert_eq!(
        cuts.first_line(&long, 60, 60),
        ("x".repeat(50).as_str(), true)
    );
    // A value that starts the line leaves nothing of it.
    assert_eq!(
        cuts.first_line(&format!("{V}{}", "x".repeat(80)), 20, 20),
        ("", true)
    );
    // A value holding a line break is seen whole: the first line stops before it.
    let broken = "note: kelp-one-0417\nferry-two-0417 later";
    assert_eq!(cuts.first_line(broken, 60, 60), ("note: ", true));
    // A short line with no value is whole.
    assert_eq!(
        cuts.first_line("\n  short line \nmore", 60, 60),
        ("short line", false)
    );
    assert_eq!(Cuts::none().first_line("", 60, 60), ("", false));
}

/// While a value is still loading, a cut falls at whitespace, or at the
/// start when there is none before it; a known value is still stepped over.
#[test]
fn cuts_while_resolving_fall_at_whitespace() {
    let board = SecretBoard::new(["late".to_string(), "known".to_string()], Instant::now());
    board.publish(
        [("known".to_string(), Ok(Secret::new(V.into())))].into(),
        "fake",
    );
    let s = Scrubber::from_board(board.clone());
    let text = format!("alpha beta-gamma-delta {V} epsilon");
    let cuts = s.cuts();
    for at in (0..=text.len()).filter(|i| text.is_char_boundary(*i)) {
        let b = cuts.before(&text, at);
        let a = cuts.after(&text, at);
        for c in [b, a] {
            assert!(
                c == 0 || c == text.len() || text[..c].ends_with(' ') || text[c..].starts_with(' '),
                "at {at}: {c}"
            );
        }
        assert!(b <= at && a >= at);
    }
    assert_eq!(cuts.before(&text, 9), "alpha ".len());
    assert_eq!(cuts.before("nowhitespacehere", 5), 0);
    assert_eq!(cuts.after("nowhitespacehere", 5), "nowhitespacehere".len());
    // Once nothing is loading, the cut falls where it is asked.
    board.publish(
        [("late".to_string(), Err("not found".to_string()))].into(),
        "fake",
    );
    assert_eq!(s.cuts().before(&text, 9), 9);
}

/// A capped tool result of 4 MiB, through the cut that reads the whole text
/// once for its head and its tail: ordinary output with two values, and
/// percent-dense text with forty. (Measured: printed for the report; the
/// bound is the guard.)
#[test]
fn a_capped_four_mib_result_is_measured() {
    let lines = [
        "   Compiling theseus-core v0.1.0 (/work/src/crates/theseus-core)\n",
        "warning: unused import: `std::fmt` --> src/lib.rs:42:5 | 100% done\n",
        "{\"level\":\"info\",\"msg\":\"GET /v1/items?id=42&page=3\",\"ms\":12}\n",
        "aGVsbG8gd29ybGQ= dGVzdA== C:\\Users\\x\\file 50%2F50 caf\u{e9}\n",
    ];
    let mut ordinary = String::new();
    while ordinary.len() < 4 << 20 {
        for l in lines {
            ordinary.push_str(l);
        }
    }
    let line = "%61%62%63%2F%25%20%3D%26%3F%41%42%43%2f%25%20%3d\n";
    let dense = line.repeat((4 << 20) / line.len());
    let forty: Vec<String> = (0..40)
        .map(|n| format!("abc/fixture-{n:02}-encoded-value-0123456789"))
        .collect();
    let two = vec![V.to_string(), "hunter2hunter2".to_string()];
    for (what, text, values) in [
        ("ordinary output, two values", &ordinary, &two),
        ("percent-dense, forty values", &dense, &forty),
    ] {
        let cuts = Scrubber::with_values(
            values
                .iter()
                .map(|v| (v.clone(), "v".to_string()))
                .collect(),
        )
        .cuts();
        let began = Instant::now();
        let (out, cut) = crate::toolrun::cap(text, 30_000, |_| String::new(), &cuts);
        let took = began.elapsed();
        assert!(cut && out.len() < 200_000);
        eprintln!("cut: a capped {} byte result, {what}: {took:?}", text.len());
        assert!(
            took < std::time::Duration::from_secs(60),
            "{what} took {took:?}"
        );
    }
}

/// A cut asked for inside a character, or past the text's end, falls on a
/// character boundary outside every value.
#[test]
fn a_cut_inside_a_character_or_past_the_end_falls_outside_a_value() {
    let cuts = knowing(&[V]).cuts();
    let text = format!("\u{e9}{V}\u{3bb}");
    let end = 2 + V.len();
    assert_eq!((cuts.before(&text, 1), cuts.after(&text, 1)), (0, 2));
    assert_eq!((cuts.before(&text, 5), cuts.after(&text, 5)), (2, end));
    assert_eq!(
        (cuts.before(&text, end + 1), cuts.after(&text, end + 1)),
        (end, text.len())
    );
    let past = text.len() + 10;
    assert_eq!(
        (cuts.before(&text, past), cuts.after(&text, past)),
        (text.len(), text.len())
    );
    let (head, cut) = cuts.head(&text, 4);
    assert_eq!((head, cut), ("\u{e9}", true));
}

/// Two values read together (one taken first, then the other read through
/// what is left; two that overlap): a cut stops at the edge of what both
/// take.
#[test]
fn a_cut_stops_at_the_edge_of_values_read_together() {
    let cuts = knowing(&[r"prefixxx\\", "abcdefgh"]).cuts();
    let text = r"prefixxx\\\\u0061bcdefgh";
    assert_eq!(
        cuts.before(text, 15),
        0,
        "inside the second value's reading"
    );
    assert_eq!(cuts.after(text, 15), text.len());
    assert_eq!(cuts.head(text, 14), ("", true));
    let cuts = knowing(&["abcdefgh12", "12345678ij"]).cuts();
    let text = "abcdefgh12345678ij and more";
    assert_eq!((cuts.before(text, 12), cuts.after(text, 12)), (0, 18));
}
