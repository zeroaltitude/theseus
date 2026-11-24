//! The order the values were read in before the one pass: each value replaced
//! verbatim in turn, longest first, then the encodings read on what is left.
//! Where a replacement ends, a decode can read differently than on the text
//! as given: a value that ends in a backslash takes it from the escape the
//! next backslash would have started, so `\\u0061` after it reads `a` here
//! and `a` there. The spans this order finds, verbatim and encoded, are
//! mapped back to the text and merged with the one pass's, so a text is
//! masked at least wherever this order masked it.

use super::{encoded, Span};

/// A stretch of the replaced text: where it is there, and the bytes of the
/// text as given that it stands for. A copied stretch has the same length in
/// both; a stand-in stands for the whole of its range.
struct Piece {
    at: usize,
    end: usize,
    from: usize,
    to: usize,
    copied: bool,
}

/// The replaced text and where each stretch of it came from.
struct Replaced {
    text: String,
    pieces: Vec<Piece>,
}

impl Replaced {
    /// The text's offset for a span of the replaced text starting at `p`:
    /// a stand-in's start for a span that starts in it.
    fn start(&self, p: usize) -> usize {
        let k = self.pieces.partition_point(|s| s.end <= p);
        match self.pieces.get(k) {
            None => self.pieces.last().map_or(p, |s| s.to),
            Some(s) if s.copied => s.from + (p - s.at),
            Some(s) => s.from,
        }
    }

    /// The text's offset for a span of the replaced text ending at `p`: a
    /// stand-in's end for a span that ends in it.
    fn end(&self, p: usize) -> usize {
        let k = self.pieces.partition_point(|s| s.end < p);
        match self.pieces.get(k) {
            None => self.pieces.last().map_or(p, |s| s.to),
            Some(s) if s.copied => s.from + (p - s.at),
            Some(s) => s.to,
        }
    }

    /// Copies `a..b` of the replaced text to `next`, with where it came from.
    fn copy(&self, a: usize, b: usize, next: &mut Replaced) {
        if a >= b {
            return;
        }
        let first = self.pieces.partition_point(|s| s.end <= a);
        for s in self.pieces[first..].iter().take_while(|s| s.at < b) {
            let (x, y) = (s.at.max(a), s.end.min(b));
            let at = next.text.len() + (x - a);
            let end = at + (y - x);
            let (from, to) = if s.copied {
                (s.from + (x - s.at), s.from + (y - s.at))
            } else {
                (s.from, s.to)
            };
            next.pieces.push(Piece {
                at,
                end,
                from,
                to,
                copied: s.copied,
            });
        }
        next.text.push_str(&self.text[a..b]);
    }
}

/// The spans the earlier order finds in `text`, as ranges of `text`: each
/// verbatim replacement, and each encoded reading of the text that is left.
/// `values` longest first, as `Substitution` keeps them.
pub(super) fn spans<'a>(
    text: &str,
    values: &[(&str, &'a str)],
    needles: &[(String, &'a str)],
) -> Vec<Span<'a>> {
    let mut now = Replaced {
        text: text.to_string(),
        pieces: vec![Piece {
            at: 0,
            end: text.len(),
            from: 0,
            to: text.len(),
            copied: true,
        }],
    };
    let mut spans = Vec::new();
    for (v, name) in values {
        if v.is_empty() || !now.text.contains(v) {
            continue;
        }
        let stand_in = format!("[redacted:{name}]");
        let mut next = Replaced {
            text: String::with_capacity(now.text.len()),
            pieces: Vec::with_capacity(now.pieces.len()),
        };
        let mut copied = 0;
        for (at, _) in now.text.match_indices(v) {
            now.copy(copied, at, &mut next);
            let (from, to) = (now.start(at), now.end(at + v.len()));
            spans.push((from, to, *name));
            let at_next = next.text.len();
            next.text.push_str(&stand_in);
            next.pieces.push(Piece {
                at: at_next,
                end: next.text.len(),
                from,
                to,
                copied: false,
            });
            copied = at + v.len();
        }
        now.copy(copied, now.text.len(), &mut next);
        now = next;
    }
    for (a, b, name) in encoded(&now.text, values, needles, false) {
        spans.push((now.start(a), now.end(b), name));
    }
    spans
}
