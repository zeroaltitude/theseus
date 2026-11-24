//! Where a cut of text may fall: never inside a span a known value takes,
//! verbatim or in any encoding the scrubber reads (`Substitution::spans`),
//! so what a cut keeps holds each value whole or not at all.
//!
//! A [`Cuts`] is taken from the [`Scrubber`] and holds the values it knew
//! then. It answers positions only: it has no `Debug`, `Clone` or
//! `Serialize`, writes nothing, and its values are zeroized when it drops.
//! The spans are found on the whole text, never a window of it: a folded
//! escape can stretch a value's encoding over any number of bytes.

use zeroize::Zeroizing;

use super::{merged, Scrubber, Substitution, MIN_VALUE};
use crate::secrets::SecretState;

/// The values known when it was taken, for placing a cut.
pub struct Cuts {
    values: Vec<(Zeroizing<String>, String)>,
    /// No value was still loading.
    settled: bool,
}

impl Scrubber {
    /// Where a cut may fall in text, for the values the scrubber knows now.
    pub fn cuts(&self) -> Cuts {
        let mut values: Vec<(Zeroizing<String>, String)> = self
            .exact
            .iter()
            .map(|(v, n)| (Zeroizing::new(v.clone()), n.clone()))
            .collect();
        let mut settled = true;
        if let Some(board) = &self.board {
            for (name, s) in board.states().iter() {
                match s {
                    SecretState::Ready(v) => {
                        let v = v.expose().trim();
                        if v.len() >= MIN_VALUE {
                            values.push((Zeroizing::new(v.to_string()), name.clone()));
                        }
                    }
                    SecretState::Resolving => settled = false,
                    SecretState::Failed(_) => {}
                }
            }
        }
        Cuts { values, settled }
    }
}

impl Cuts {
    /// Knows no value: a cut falls where it is asked. For text a mask has
    /// already passed.
    pub fn none() -> Self {
        Self {
            values: Vec::new(),
            settled: true,
        }
    }

    /// The spans the known values take in `text`, sorted, overlapping ones
    /// merged.
    fn spans(&self, text: &str) -> Vec<(usize, usize)> {
        if self.values.is_empty() {
            return Vec::new();
        }
        let sub = Substitution::new(
            self.values
                .iter()
                .map(|(v, n)| (v.as_str(), n.as_str()))
                .collect(),
        );
        merged(text, sub.spans(text))
            .0
            .into_iter()
            .map(|(a, b, _)| (a, b))
            .collect()
    }

    /// Where a head of `text` that would end at byte `at` ends: the greatest
    /// character boundary at or before `at` that is not inside a known
    /// value's span. While a value is still loading it also falls at
    /// whitespace (or at the start when there is none before `at`): a value
    /// with no whitespace never spans such a place, one that holds
    /// whitespace may.
    pub fn before(&self, text: &str, at: usize) -> usize {
        self.back(text, at, &self.spans(text))
    }

    /// Where a tail of `text` that would start at byte `at` starts: the
    /// least character boundary at or after `at` that is not inside a known
    /// value's span, at whitespace (or the end) too while a value is still
    /// loading, as [`Cuts::before`].
    pub fn after(&self, text: &str, at: usize) -> usize {
        self.on(text, at, &self.spans(text))
    }

    /// [`Cuts::before`] of `head` and [`Cuts::after`] of `tail`, the text
    /// read once for both.
    pub fn around(&self, text: &str, head: usize, tail: usize) -> (usize, usize) {
        let spans = self.spans(text);
        (self.back(text, head, &spans), self.on(text, tail, &spans))
    }

    fn back(&self, text: &str, at: usize, spans: &[(usize, usize)]) -> usize {
        let mut at = floor(text, at.min(text.len()));
        loop {
            let was = at;
            if let Some((a, _)) = spans.iter().find(|(a, b)| *a < at && at < *b) {
                at = *a;
            }
            if !self.settled {
                while !at_edge(text, at) {
                    at = floor(text, at - 1);
                }
            }
            if at == was {
                return at;
            }
        }
    }

    fn on(&self, text: &str, at: usize, spans: &[(usize, usize)]) -> usize {
        let mut at = ceil(text, at.min(text.len()));
        loop {
            let was = at;
            if let Some((_, b)) = spans.iter().find(|(a, b)| *a < at && at < *b) {
                at = *b;
            }
            if !self.settled {
                while !at_edge(text, at) {
                    at = ceil(text, at + 1);
                }
            }
            if at == was {
                return at;
            }
        }
    }

    /// Where a head of `text` ends that a rewrite changing the characters
    /// `changes` names may take: before the first known value's span that
    /// holds such a character, so every value the rewritten head keeps is
    /// still whole and as written; the text's end when there is none.
    pub fn unchanged(&self, text: &str, changes: impl Fn(char) -> bool) -> usize {
        self.spans(text)
            .into_iter()
            .find(|(a, b)| text[*a..*b].chars().any(&changes))
            .map_or(text.len(), |(a, _)| a)
    }

    /// `text`'s first `max` characters, ending where [`Cuts::before`] says,
    /// and whether anything was cut.
    pub fn head<'t>(&self, text: &'t str, max: usize) -> (&'t str, bool) {
        match text.char_indices().nth(max) {
            None => (text, false),
            Some((at, _)) => (&text[..self.before(text, at)], true),
        }
    }

    /// The text's first line that is not blank, trimmed: whole when it has
    /// at most `max` characters and no known value runs on past its end,
    /// else its head of at most `keep` characters that stops before any
    /// known value, perhaps empty. A value holding a line break is seen
    /// whole, since its spans are found in the whole text. The flag says
    /// whether the line was cut.
    pub fn first_line<'t>(&self, text: &'t str, max: usize, keep: usize) -> (&'t str, bool) {
        let Some(line) = text.lines().find(|l| !l.trim().is_empty()) else {
            return ("", false);
        };
        let line = line.trim();
        let start = line.as_ptr() as usize - text.as_ptr() as usize;
        let end = start + line.len();
        let at = if line.chars().count() > max {
            line.char_indices()
                .nth(keep)
                .map_or(end, |(i, _)| start + i)
        } else {
            end
        };
        let cut = self.before(text, at);
        if cut >= end {
            (line, false)
        } else {
            (&text[start..cut.max(start)], true)
        }
    }
}

/// A tool's plan cuts its summary with the core's cut (`ToolCtx::cut`).
impl theseus_tools::Cut for Cuts {
    fn head<'t>(&self, text: &'t str, max: usize) -> (&'t str, bool) {
        Cuts::head(self, text, max)
    }

    fn first_line<'t>(&self, text: &'t str, max: usize, keep: usize) -> (&'t str, bool) {
        Cuts::first_line(self, text, max, keep)
    }
}

/// Whether a cut at `at` falls at whitespace or an end: a value with no
/// whitespace in it never spans it.
fn at_edge(text: &str, at: usize) -> bool {
    at == 0
        || at == text.len()
        || text[at..].starts_with(char::is_whitespace)
        || text[..at].ends_with(char::is_whitespace)
}

/// The greatest character boundary at or before `at`.
fn floor(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The least character boundary at or after `at`.
fn ceil(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}
