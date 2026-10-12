//! A reply's citations as the CLI and the terminal UI draw them
//! (theseus-ejdk): each handle the reply cites (`[m:a1b2c3]`) becomes a
//! numbered mark (`[1]`), and a footnote block under the reply says whose
//! message it was, where, and when, and the command that opens it. A handle
//! with no source (the turn was not given it, or a shared place may not show
//! it) stays as the model wrote it. The builders are plain functions of text
//! and citations, so another renderer can call them.

pub use theseus_protocol::cite::mark_handles as mark;
use theseus_protocol::cite::{rewrite, Citation, HANDLE_PREFIX};

use super::{push, Line, Tag};

/// The command that opens a cited source.
pub fn open_command(handle: &str) -> String {
    format!("theseus history {handle}")
}

/// The footnote block under a reply: each source's number, its frozen header
/// (whose, where, when, as of which position), and the command that opens it.
pub fn footnotes(order: &[&Citation]) -> Vec<Line> {
    let mut out = Vec::new();
    for (i, c) in order.iter().enumerate() {
        push(&mut out, Tag::Dim, &format!("  [{}] {}", i + 1, c.header));
        push(
            &mut out,
            Tag::Dim,
            &format!("      {}", open_command(&c.handle)),
        );
    }
    out
}

/// A reply's text as it streams, with each handle in it as a numbered mark
/// as it completes: a handle that arrives in two pieces is held until it is
/// whole. Every well-formed handle is numbered, in order, since the
/// citations come only when the turn ends; `notes` then says which were
/// sources.
#[derive(Debug, Default)]
pub struct Marker {
    held: String,
    seen: Vec<String>,
}

impl Marker {
    /// Whether `s` could still become a handle: `[`, `[m`, `[m:`, and `[m:`
    /// with its characters so far.
    fn could_be_a_handle(s: &str) -> bool {
        let Some(rest) = s.strip_prefix('[') else {
            return false;
        };
        let want = HANDLE_PREFIX;
        match rest.strip_prefix(want) {
            Some(tail) => {
                tail.len() <= 64 && tail.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            }
            None => want.starts_with(rest),
        }
    }

    /// What to print now for `chunk`; a possible handle's start is held.
    pub fn push(&mut self, chunk: &str) -> String {
        let text = std::mem::take(&mut self.held) + chunk;
        let cut = text
            .rfind('[')
            .filter(|&i| Self::could_be_a_handle(&text[i..]))
            .unwrap_or(text.len());
        self.held = text[cut..].to_string();
        let seen = &mut self.seen;
        rewrite(&text[..cut], |h| {
            let i = seen.iter().position(|s| s == h).unwrap_or_else(|| {
                seen.push(h.to_string());
                seen.len() - 1
            });
            Some(format!("[{}]", i + 1))
        })
    }

    /// What is still held, as it came: the reply ended.
    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.held)
    }

    /// The footnote block for the handles that streamed past: a source by its
    /// header and its command, and a handle that was no source of this turn
    /// said so.
    pub fn notes(&self, cites: &[Citation]) -> Vec<Line> {
        let mut out = Vec::new();
        for (i, h) in self.seen.iter().enumerate() {
            match cites.iter().find(|c| &c.handle == h) {
                Some(c) => {
                    push(&mut out, Tag::Dim, &format!("  [{}] {}", i + 1, c.header));
                    push(&mut out, Tag::Dim, &format!("      {}", open_command(h)));
                }
                None => push(
                    &mut out,
                    Tag::Dim,
                    &format!("  [{}] {h}: not a source this reply was given", i + 1),
                ),
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

/// A history node's reply with its cited handles as marks, and the node's
/// footnotes; the node is cloned only when it holds a cited handle.
pub fn cited_node(
    n: &theseus_protocol::NodeInfo,
    cites: &[Citation],
) -> Option<(theseus_protocol::NodeInfo, Vec<Line>)> {
    if n.kind != "assistant_message" || cites.is_empty() {
        return None;
    }
    let (text, order) = mark(&n.text, cites);
    if order.is_empty() {
        return None;
    }
    let mut marked = n.clone();
    marked.text = text;
    Some((marked, footnotes(&order)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cite(handle: &str) -> Citation {
        Citation {
            handle: handle.into(),
            node_id: "msg_1".into(),
            session_id: "ses_1".into(),
            position: 18231,
            header: "a message from cli in #harbor, 2026-09-30 14:34 UTC (as of @18231)".into(),
            at_unix_ms: 0,
            excerpt: String::new(),
            discord: None,
        }
    }

    /// The footnote golden: marks in order of first mention, a repeated
    /// handle one number, an unknown handle left as text, and each source's
    /// header with the command that opens it.
    #[test]
    fn marks_and_footnotes_are_their_golden_bytes() {
        let cites = [cite("m:a1b2c3"), cite("m:d4e5f6")];
        let text = "You chose oak [m:d4e5f6] after pine [m:a1b2c3], then oak again [m:d4e5f6]; \
                    one more [m:ffffff].";
        let (marked, order) = mark(text, &cites);
        assert_eq!(
            marked,
            "You chose oak [1] after pine [2], then oak again [1]; one more [m:ffffff]."
        );
        let lines: Vec<String> = footnotes(&order).into_iter().map(|l| l.text).collect();
        assert_eq!(
            lines,
            [
                "  [1] a message from cli in #harbor, 2026-09-30 14:34 UTC (as of @18231)",
                "      theseus history m:d4e5f6",
                "  [2] a message from cli in #harbor, 2026-09-30 14:34 UTC (as of @18231)",
                "      theseus history m:a1b2c3",
            ]
        );
    }

    /// A handle split across the stream's pieces is held, then marked; text
    /// that only looks like a start is let go when it is not one.
    #[test]
    fn a_streamed_handle_is_marked_whole() {
        let mut m = Marker::default();
        let mut out = String::new();
        for piece in [
            "Oak [m:a1",
            "b2c3] and [",
            "m:ffffff",
            "] then [x] and [m",
            "].",
        ] {
            out.push_str(&m.push(piece));
        }
        out.push_str(&m.finish());
        assert_eq!(out, "Oak [1] and [2] then [x] and [m].");
        let notes: Vec<String> = m
            .notes(&[cite("m:a1b2c3")])
            .into_iter()
            .map(|l| l.text)
            .collect();
        assert_eq!(notes.len(), 3);
        assert!(notes[1].ends_with("theseus history m:a1b2c3"));
        assert_eq!(
            notes[2],
            "  [2] m:ffffff: not a source this reply was given"
        );
    }
}
