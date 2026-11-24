//! What a page's cut kept of its text: the bytes it says it kept, rewritten as Flat1, and its ellipsis, are
//! exactly the line it shows, which is the line it showed before it said so. All values invented.

use super::super::open::{margin1, margined};
use super::super::page::{cut, cut_kept, flat, flat1};
use crate::scrub::Cuts;
use crate::view::text::kept::sample::{self, Draw};

/// `flat` as it was written before Flat1 was named: the oracle the line is held to.
fn flat_as_it_was(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{ab}' => '\u{2039}',
            '\u{bb}' => '\u{203a}',
            '[' => '(',
            ']' => ')',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `cut` as it was written before it said what it kept.
fn cut_as_it_was(text: &str, max: usize, cuts: &Cuts) -> String {
    let kept = cuts.unchanged(text, |c| {
        matches!(c, '\u{ab}' | '\u{bb}' | '[' | ']') || c.is_control() || c.is_whitespace()
    });
    let short = kept < text.len();
    let text = flat_as_it_was(&text[..kept]);
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}\u{2026}", &text[..cuts.before(&text, at)]),
        None if short => format!("{text}\u{2026}"),
        None => text,
    }
}

/// Over random text (characters of one to four bytes, control characters, brackets, runs of blanks; shorter
/// than, as long as and longer than the cap; a known value before, across and after it, one across a line
/// break), with the cuts a scrubber gives: the line is today's; it is Flat1 of the bytes the cut says it kept,
/// then an ellipsis exactly when words are left after them; and those bytes split no character and no
/// known value.
#[test]
fn cut_reports_the_bytes_it_kept() {
    let mut draw = Draw::new(0x2545_f491_4f6c_dd1d);
    for case in 0..1_500 {
        let max = [0, 1, 3, 8, 20, 60, 120][draw.below(7)];
        let (text, known) = draw.case(max, 2 * max + 12);
        let (cuts, known) = match draw.below(6) {
            0 => (Cuts::none(), Vec::new()),
            _ => (sample::cuts(&known), known),
        };
        assert_eq!(flat(&text), flat_as_it_was(&text), "{text:?}");
        let (shown, kept) = cut_kept(&text, max, &cuts);
        assert_eq!(shown, cut_as_it_was(&text, max, &cuts), "case {case}: {text:?}");
        assert_eq!(cut(&text, max, &cuts), shown);
        sample::whole(&text, &kept, &known);
        let left = text[kept.end..]
            .chars()
            .any(|c| !(c.is_control() || c.is_whitespace()));
        let ellipsis = if left { "\u{2026}" } else { "" };
        assert_eq!(
            format!("{}{ellipsis}", flat1(&text[kept.clone()])),
            shown,
            "case {case}: {text:?} at {max}, kept {kept:?}"
        );
    }
}

/// The cases the property draws at random, by hand: a cut that ends in a space keeps one blank after its
/// last word; one that stops before a value keeps none of it; quote marks of two bytes become three.
#[test]
fn a_cut_says_which_bytes_its_line_is() {
    let v = sample::VALUE;
    let cuts = sample::cuts(&[v, sample::BROKEN]);
    let (shown, kept) = cut_kept("  ab \t\n cd ef", 3, &cuts);
    assert_eq!((shown.as_str(), kept), ("ab \u{2026}", 2..5));
    let text = format!("said {v} later");
    assert_eq!(cut_kept(&text, 7, &cuts), ("said \u{2026}".into(), 0..5));
    let text = "x kelp-one-0417\nferry-two-0417 y";
    assert_eq!(cut_kept(text, 200, &cuts), ("x\u{2026}".into(), 0..1));
    let text = "\u{ab}q\u{bb} [z]";
    assert_eq!(
        cut_kept(text, 200, &cuts),
        ("\u{2039}q\u{203a} (z)".into(), 0..text.len())
    );
}

/// `margined` as it was written before Margin1 was named.
fn margined_as_it_was(text: &str) -> String {
    text.trim()
        .lines()
        .map(|l| {
            let l: String = l
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            format!("\u{2502} {l}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// An open's margined words are today's, and they are Margin1 of the text's bytes from its first character
/// that is not whitespace to its last.
#[test]
fn margined_is_margin1_of_its_trimmed_bytes() {
    let mut draw = Draw::new(0x3c6e_f372_fe94_f82b);
    for _ in 0..800 {
        let n = draw.below(60);
        let text = draw.text(n);
        let from = text.len() - text.trim_start().len();
        let to = from + text.trim().len();
        assert_eq!(margined(&text), margined_as_it_was(&text), "{text:?}");
        assert_eq!(margin1(&text[from..to]), margined(&text), "{text:?}");
    }
}
