//! A range a reference stores is where a cut stopped, and it is read again on every lay: it must never begin or
//! end inside a known value, verbatim or in any encoding the cut's snapshot reads. Checked against the scrubber's
//! own span finder over the raw text (the oracle), never against the cutter's own idea of where values are: so the
//! check is apart from where a cut chooses its edge, but not from how the scrubber recognises a value. It is seeded
//! coverage of the six encodings `scrub::oracle::encodings` writes, each asserted found where it was put, not a
//! proof for an encoding the scrubber does not read. All values invented.

use std::ops::Range;

use crate::history_tool::page::{cut_kept, flat1};
use crate::scrub::oracle::{encodings, spans_of, ENCODINGS};
use crate::scrub::Scrubber;

/// Values a cut must keep whole or not at all: one the page's rewrite leaves as it is, and one it would change
/// (a space, a backslash, a quote mark).
pub(super) const VALUES: [&str; 2] = [
    "kelp-ferry-0417-vault-token",
    "tern\\gull 0417/\u{ab}x\u{bb}",
];

pub(super) fn knowing(values: &[&str]) -> Scrubber {
    Scrubber::with_values(
        (values.iter().enumerate())
            .map(|(i, v)| ((*v).to_string(), format!("v{i}")))
            .collect(),
    )
}

/// A seeded draw, as `scrub/tests_cut.rs` draws.
pub(super) struct Draw(u64);

impl Draw {
    pub(super) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(super) fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    /// Words and blanks, `n` pieces long: letters, a few of 2 to 4 bytes, spaces, tabs and line breaks.
    pub(super) fn filler(&mut self, n: usize) -> String {
        const PIECES: [&str; 12] = [
            "tide",
            "weir",
            " ",
            " ",
            "\n",
            "\t",
            "caf\u{e9}",
            "\u{3bb}",
            "\u{1f30a}",
            "  ",
            "mill",
            ",",
        ];
        (0..n).map(|_| PIECES[self.below(PIECES.len())]).collect()
    }

    /// Filler with `value` in each of its encodings, each at a random place.
    pub(super) fn text_with(&mut self, value: &str) -> String {
        self.text_at(value).0
    }

    /// [`Draw::text_with`], and where each encoding was put: its index in `encodings` and its bytes.
    pub(super) fn text_at(&mut self, value: &str) -> (String, Vec<(usize, Range<usize>)>) {
        let n = 1 + self.below(6);
        let mut text = self.filler(n);
        let mut at = Vec::new();
        for (i, e) in encodings(value).into_iter().enumerate() {
            text.push(' ');
            at.push((i, text.len()..text.len() + e.len()));
            text.push_str(&e);
            text.push(' ');
            let n = self.below(8);
            text.push_str(&self.filler(n));
        }
        (text, at)
    }
}

/// Each encoding put in `text` at `at` is found: some span the oracle finds overlaps it. A finder that lost one
/// encoding fails here by the encoding's name, where a count of spans would still pass.
pub(super) fn each_found(text: &str, value: &str, at: &[(usize, Range<usize>)]) {
    let spans = spans_of(text, &[value]);
    for (i, r) in at {
        assert!(
            spans.iter().any(|(a, b)| *a < r.end && r.start < *b),
            "the oracle does not find {value:?} {} at {r:?} in {text:?}: {spans:?}",
            ENCODINGS[*i]
        );
    }
}

/// The oracle finds each encoding of each value, alone in a line of words: a lost one fails by name before any cut
/// is checked.
#[test]
fn the_oracle_finds_each_encoding_of_each_value() {
    for value in VALUES {
        for (i, e) in encodings(value).into_iter().enumerate() {
            let text = format!("tide weir {e} mill");
            each_found(&text, value, &[(i, 10..10 + e.len())]);
        }
    }
}

/// Whether `at` lies strictly inside one of `spans`.
pub(super) fn inside(spans: &[(usize, usize)], at: usize) -> Option<(usize, usize)> {
    spans.iter().copied().find(|(a, b)| *a < at && at < *b)
}

/// The page's cut, at every length: the bytes it says it kept begin and end outside every span the oracle finds in
/// the raw text, and its line is the page's form of exactly those bytes, then an ellipsis or nothing.
#[test]
fn a_page_cuts_kept_bytes_never_cut_an_encoded_known_value() {
    let mut draw = Draw::new(0x9e37_79b9_7f4a_7c15);
    let mut checked = 0;
    for round in 0..80 {
        let value = VALUES[round % VALUES.len()];
        let cuts = knowing(&[value]).cuts();
        let (text, at) = draw.text_at(value);
        assert_eq!(at.len(), ENCODINGS.len(), "every encoding is in {text:?}");
        each_found(&text, value, &at);
        let spans = spans_of(&text, &[value]);
        let chars = text.chars().count();
        for max in (1..chars + 3).step_by(1 + round % 3) {
            let (line, kept) = cut_kept(&text, max, &cuts);
            for edge in [kept.start, kept.end] {
                assert_eq!(
                    inside(&spans, edge),
                    None,
                    "max {max}: the kept bytes {kept:?} end inside a value in {text:?}"
                );
            }
            let shown = flat1(&text[kept.clone()]);
            assert!(
                line == shown || line == format!("{shown}\u{2026}"),
                "max {max}: {line:?} is not the form of {kept:?}: {shown:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 5_000, "{checked}");
}
