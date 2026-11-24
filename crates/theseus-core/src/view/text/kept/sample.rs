//! Random text for the cutters' property tests, from a fixed seed (as `scrub/tests_cut.rs` draws): letters,
//! characters of two, three and four bytes, control characters, the page's quote marks and brackets, runs of
//! blanks and line breaks, and a known value placed anywhere in it. All values invented.

/// A known value with no blank in it, and one that holds a line break.
pub const VALUE: &str = "kelp-ferry-0417-vault-token";
pub const BROKEN: &str = "kelp-one-0417\nferry-two-0417";

/// A xorshift draw from a fixed seed: the same cases every run.
pub struct Draw(u64);

impl Draw {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    /// A number below `bound` (which is not zero).
    pub fn below(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    /// Text of about `chars` pieces from the alphabet below.
    pub fn text(&mut self, chars: usize) -> String {
        const PIECES: &[&str] = &[
            "a",
            "b",
            "q",
            "Z",
            "0",
            "7",
            "-",
            ".",
            " ",
            " ",
            "  ",
            "   ",
            "\n",
            "\n\n",
            "\r\n",
            "\t",
            "\u{0}",
            "\u{7}",
            "\u{85}",
            "\u{a0}",
            "\u{2003}",
            "\u{e9}",
            "\u{3bb}",
            "\u{2026}",
            "\u{1f642}",
            "\u{ab}",
            "\u{bb}",
            "[",
            "]",
            "(",
            "\u{2039}",
            "word",
            "kelp",
            "%41",
            "\\n",
        ];
        (0..chars).map(|_| PIECES[self.below(PIECES.len())]).collect()
    }

    /// A case: text of up to `most` pieces, perhaps with a known value placed before, across or after `max`
    /// characters, and perhaps padded with blanks at either end; and the values the cuts know.
    pub fn case(&mut self, max: usize, most: usize) -> (String, Vec<&'static str>) {
        let n = self.below(most + 1);
        let mut text = self.text(n);
        let mut known = Vec::new();
        for value in [VALUE, BROKEN] {
            if self.below(3) == 0 {
                continue;
            }
            let near = match self.below(3) {
                0 => self.below(max + 1),
                1 => max.saturating_sub(self.below(VALUE.len())),
                _ => max + self.below(most + 1),
            };
            text = place(&text, value, near);
            known.push(value);
        }
        if self.below(4) == 0 {
            text = format!(" \n\t{text}  \n");
        }
        (text, known)
    }
}

/// `text` with `value` put in at the character boundary `at` characters in (its end, past it).
pub fn place(text: &str, value: &str, at: usize) -> String {
    let i = text.char_indices().nth(at).map_or(text.len(), |(i, _)| i);
    format!("{}{value}{}", &text[..i], &text[i..])
}

/// The cuts that know `values`, as a scrubber built from them gives them.
pub fn cuts(values: &[&str]) -> crate::scrub::Cuts {
    crate::scrub::Scrubber::with_values(
        values
            .iter()
            .enumerate()
            .map(|(i, v)| ((*v).to_string(), format!("v{i}")))
            .collect(),
    )
    .cuts()
}

/// Every place a known value stands in `text` verbatim, as bytes.
pub fn verbatim(text: &str, values: &[&str]) -> Vec<(usize, usize)> {
    values
        .iter()
        .flat_map(|v| text.match_indices(v).map(|(a, _)| (a, a + v.len())))
        .collect()
}

/// That `range` is a range of `text` on character boundaries, and neither end is inside a known value.
pub fn whole(text: &str, range: &std::ops::Range<usize>, values: &[&str]) {
    assert!(
        range.start <= range.end && range.end <= text.len(),
        "{range:?} of {text:?}"
    );
    assert!(
        text.is_char_boundary(range.start) && text.is_char_boundary(range.end),
        "{range:?} splits a character of {text:?}"
    );
    for (a, b) in verbatim(text, values) {
        for at in [range.start, range.end] {
            assert!(
                !(a < at && at < b),
                "{range:?} splits a known value at ({a}, {b}) of {text:?}"
            );
        }
    }
}
