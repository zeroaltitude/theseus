//! A look back's answer as it is written (issue 49): its text and, in the same pass, the same text as parts
//! (`quotes::Part`), the code's own words and quotes of what nodes said, so its result is kept by reference
//! and stores no words of a message. Each quote is pushed as the words it shows, its form of the bytes it names,
//! so what `quotes::render` makes of the parts is the text, byte for byte, while its sources read as they were
//! written.

use crate::history_tool::open::MARGIN;
use crate::quotes::{Form, Part, Quote, UNREADABLE_QUOTE};
use crate::scrub::Cuts;
use crate::view::history::Quoted;

/// The key of a look back's meta its parts ride under, from the tool to the runtime, which takes them out
/// (`toolrun::parts`).
pub const PARTS: &str = "parts";

/// A part as it is written: code, or a quote with the whole text of its field, which a cut reads.
#[derive(Debug, Clone)]
enum Piece {
    Code(String),
    Quote { q: Quote, field: String },
}

/// An answer being written: its text, and its parts.
#[derive(Debug, Clone, Default)]
pub struct Written {
    text: String,
    pieces: Vec<Piece>,
}

impl Written {
    /// The code's own words `s`.
    pub fn code(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.text.push_str(s);
        match self.pieces.last_mut() {
            Some(Piece::Code(c)) => c.push_str(s),
            _ => self.pieces.push(Piece::Code(s.to_string())),
        }
    }

    /// `q`, `field` the whole text of its field: its form of the bytes it names. One that shows nothing is left
    /// out.
    pub fn quote(&mut self, q: Quote, field: &str) {
        let shown = (field.get(q.from as usize..q.to as usize)).map(|b| q.form.apply(b));
        match shown {
            Some(s) if s.is_empty() => {}
            Some(s) => {
                self.text.push_str(&s);
                let field = field.to_string();
                self.pieces.push(Piece::Quote { q, field });
            }
            None => self.code(UNREADABLE_QUOTE),
        }
    }

    /// `shown`, a text of the code's whose first bytes are the words `quoted` shows, when it names a quote: those
    /// as the quote, the rest (an ellipsis, a mark) as code. Text that does not begin with them shows the fixed
    /// line of a quote that cannot be read, never words.
    pub fn said(&mut self, shown: &str, quoted: Option<&Quoted>) {
        let Some(qd) = quoted else {
            return self.code(shown);
        };
        let words = (qd.field.get(qd.quote.from as usize..qd.quote.to as usize))
            .map(|b| qd.quote.form.apply(b));
        match words {
            Some(w) if shown.starts_with(&w) => {
                self.quote(qd.quote.clone(), &qd.field);
                self.code(&shown[w.len()..]);
            }
            _ => self.code(UNREADABLE_QUOTE),
        }
    }

    /// `other` after this.
    pub fn append(&mut self, other: Written) {
        for p in other.pieces {
            match p {
                Piece::Code(c) => self.code(&c),
                Piece::Quote { q, field } => self.quote(q, &field),
            }
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text, and the parts that show it.
    pub fn into_parts(self) -> (String, Vec<Part>) {
        let parts = (self.pieces.into_iter())
            .map(|p| match p {
                Piece::Code(c) => Part::Code(c),
                Piece::Quote { q, .. } => Part::Quote(q),
            })
            .collect();
        (self.text, parts)
    }

    /// What shows the first `k` bytes of the text (a character's edge): its parts up to there, a quote the edge
    /// falls in cut to the bytes whose form shows a head of its words there. That head is the text's own unless the
    /// edge falls inside a known value in the quote's words that its form hides from a cut on the text (`cuts`):
    /// the quote then stops at the value's edge, which only shows less.
    pub fn head(self, k: usize, cuts: &Cuts) -> Written {
        let mut out = Written::default();
        let mut s = 0;
        for p in self.pieces {
            if s >= k {
                break;
            }
            match p {
                Piece::Code(c) => {
                    let e = s + c.len();
                    out.code(&c[..e.min(k) - s]);
                    s = e;
                }
                Piece::Quote { q, field } => {
                    let shown = q.form.apply(&field[q.from as usize..q.to as usize]);
                    let e = s + shown.len();
                    match e <= k {
                        true => out.quote(q, &field),
                        false => out.cut_quote(q, &field, &shown[..k - s], cuts),
                    }
                    s = e;
                }
            }
        }
        out
    }

    /// The head of `q` (its field `field`) that shows `head`, a head of its words: the longest run of its bytes
    /// from its start whose form begins `head`, then of the rest of `head` only an open's line break and margin
    /// before its next line's words, as code.
    fn cut_quote(&mut self, q: Quote, field: &str, head: &str, cuts: &Cuts) {
        let (from, to) = (q.from as usize, q.to as usize);
        let bytes = &field[from..to];
        let edges: Vec<usize> = (bytes.char_indices().map(|(i, _)| i))
            .chain([bytes.len()])
            .collect();
        let shows = |i: usize| q.form.apply(&bytes[..edges[i]]);
        // The last edge whose form is no longer than the head (the form of a longer run is never shorter)...
        let mut i = edges.partition_point(|e| q.form.apply(&bytes[..*e]).len() <= head.len());
        i = i.saturating_sub(1);
        // ...stepped back to one whose form the head begins with.
        let mut steps = 0;
        while i > 0 && !head.starts_with(&shows(i)) {
            i -= 1;
            steps += 1;
            if steps > 16 {
                i = 0;
            }
        }
        let p = edges[i];
        let guarded = cuts.before(field, from + p).saturating_sub(from);
        let end = guarded.min(p);
        self.quote(
            Quote {
                to: (from + end) as u32,
                ..q.clone()
            },
            field,
        );
        if end == p {
            let rest = &head[shows(i).len()..];
            let margin = format!("\n{MARGIN}").starts_with(rest) || MARGIN.starts_with(rest);
            if q.form == Form::Margin1 && margin {
                self.code(rest);
            }
        }
    }
}

/// The words `field` holds `range` of, quoted from `node`'s record at `position` in `form`: none when the range
/// shows none of them.
pub fn quoted(
    (node, position): (&str, u64),
    field: (crate::quotes::Field, &str),
    range: std::ops::Range<usize>,
    form: Form,
) -> Option<Quoted> {
    (!range.is_empty()).then(|| Quoted {
        quote: Quote {
            node: node.to_string(),
            position,
            field: field.0,
            from: range.start as u32,
            to: range.end as u32,
            form,
        },
        field: field.1.to_string(),
    })
}

#[cfg(test)]
mod tests;
