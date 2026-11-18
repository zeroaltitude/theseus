//! A long text on a card, built by the daemon from the two texts themselves
//! (issue 23): its size and fingerprint, the lines that changed, whole, with a
//! little context, and the whole text before and after for the owner to read.
//!
//! The card is never built from anything the model wrote about the change. It
//! never shows cut text as if it were whole: the changed lines either fit the
//! card's room and are all there, or the card is a summary and says so. A
//! character that cannot be seen (a control character, a direction mark, a
//! zero-width or hidden one) is written as `\u{..}` and counted wherever the
//! text is shown, and a backslash is shown doubled so that a written `\u{..}`
//! can never be taken for an escape.

use sha2::{Digest, Sha256};
use similar::{ChangeTag, TextDiff};

/// Lines of context around a changed line.
pub const CONTEXT: usize = 2;

/// The fingerprint's length, in hexadecimal digits of the SHA-256: 64 bits,
/// the length a context file's digest has in the prompt's manifest.
pub const DIGITS: usize = 16;

/// Whether `c` cannot be seen where it sits: a control character, a
/// line or paragraph separator, a zero-width, joining, direction or other
/// format character, or one of the tag and variation characters that hide
/// text. Written as `\u{..}` wherever text is shown.
pub fn unseen(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{0600}'..='\u{0605}'
                | '\u{061C}'
                | '\u{06DD}'
                | '\u{070F}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'..='\u{206F}'
                | '\u{FE00}'..='\u{FE0E}'
                | '\u{FEFF}'
                | '\u{FFF9}'..='\u{FFFB}'
                | '\u{E0000}'..='\u{E007F}'
                | '\u{E0100}'..='\u{E01EF}'
        )
}

/// One line of text made safe to show: each unseen character as `\u{..}`, each
/// backslash doubled. And how many unseen characters it held.
pub fn escape_line(s: &str) -> (String, usize) {
    let mut out = String::with_capacity(s.len());
    let mut hidden = 0;
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            c if unseen(c) => {
                hidden += 1;
                out.push_str(&format!("\\u{{{:04x}}}", c as u32));
            }
            c => out.push(c),
        }
    }
    (out, hidden)
}

/// The first [`DIGITS`] hexadecimal digits of the text's SHA-256.
pub fn fingerprint(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))[..DIGITS].to_string()
}

/// What a text is, in the words a card gives of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    pub bytes: usize,
    pub lines: usize,
    pub fingerprint: String,
    /// Unseen characters in the whole text.
    pub unseen: usize,
}

pub fn facts(text: &str) -> Facts {
    Facts {
        bytes: text.len(),
        lines: text.lines().count(),
        fingerprint: fingerprint(text),
        // A line break is what lines are made of, not a hidden character.
        unseen: text.chars().filter(|c| *c != '\n' && unseen(*c)).count(),
    }
}

impl Facts {
    /// `4,210 bytes, 118 lines, fingerprint 3f9a1c20b7e4d581`.
    pub fn words(&self) -> String {
        format!(
            "{}, {}, fingerprint {}",
            crate::narrative::count(self.bytes as u64, "byte", "bytes"),
            crate::narrative::count(self.lines as u64, "line", "lines"),
            self.fingerprint
        )
    }
}

/// What a row of the difference is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Context,
    Removed,
    Added,
}

/// One line of the difference, already escaped for showing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub text: String,
    /// The line has no line break after it (only the last line of a file).
    pub no_break: bool,
}

/// A run of changed lines with their context, and where it sits in each text
/// (the first line's number, from 1, and how many lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: (usize, usize),
    pub new: (usize, usize),
    pub rows: Vec<Row>,
}

/// The difference between two texts, line by line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
    /// Unseen characters on the lines the hunks show.
    pub unseen: usize,
}

pub fn diff(before: &str, after: &str) -> Diff {
    let d = TextDiff::from_lines(before, after);
    let mut out = Diff::default();
    for group in d.grouped_ops(CONTEXT) {
        let mut hunk = Hunk {
            old: (group[0].old_range().start + 1, 0),
            new: (group[0].new_range().start + 1, 0),
            rows: Vec::new(),
        };
        for op in &group {
            for ch in d.iter_changes(op) {
                let (kind, count) = match ch.tag() {
                    ChangeTag::Equal => (Kind::Context, (1, 1)),
                    ChangeTag::Delete => (Kind::Removed, (1, 0)),
                    ChangeTag::Insert => (Kind::Added, (0, 1)),
                };
                hunk.old.1 += count.0;
                hunk.new.1 += count.1;
                let line = ch.value();
                let (body, hidden) = escape_line(line.strip_suffix('\n').unwrap_or(line));
                out.unseen += hidden;
                match kind {
                    Kind::Added => out.added += 1,
                    Kind::Removed => out.removed += 1,
                    Kind::Context => {}
                }
                hunk.rows.push(Row {
                    kind,
                    text: body,
                    no_break: !line.ends_with('\n'),
                });
            }
        }
        out.hunks.push(hunk);
    }
    out
}

impl Diff {
    /// Where the changes are, in the new text's lines: `lines 12-19, 40`.
    pub fn places(&self) -> String {
        let spans: Vec<String> = self
            .hunks
            .iter()
            .map(|h| {
                let (from, n) = h.new;
                match n {
                    0 | 1 => format!("{from}"),
                    n => format!("{from}-{}", from + n - 1),
                }
            })
            .collect();
        format!(
            "{} {}",
            if spans.len() == 1 && !spans[0].contains('-') {
                "line"
            } else {
                "lines"
            },
            spans.join(", ")
        )
    }

    /// The changed lines with their context, all of them, as a fenced block
    /// in the form of a unified difference. `None` when they cannot be shown
    /// whole: more than `room` characters, or a line holding a triple
    /// backtick, which would close the block early.
    pub fn block(&self, room: usize) -> Option<String> {
        let mut out = String::from("```diff\n");
        for h in &self.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                h.old.0, h.old.1, h.new.0, h.new.1
            ));
            for r in &h.rows {
                if r.text.contains("```") {
                    return None;
                }
                out.push(match r.kind {
                    Kind::Context => ' ',
                    Kind::Removed => '-',
                    Kind::Added => '+',
                });
                out.push_str(&r.text);
                out.push('\n');
                if r.no_break {
                    out.push_str("\\ no line break at the end of the file\n");
                }
            }
        }
        out.push_str("```\n");
        (out.chars().count() <= room).then_some(out)
    }
}

/// A text whole, for the owner to read before the yes: its facts, then every
/// line, escaped as everywhere else. Never cut.
pub fn whole(label: &str, text: &str) -> String {
    let f = facts(text);
    let mut out = format!(
        "===== {label}: {}{} =====\n",
        f.words(),
        match f.unseen {
            0 => String::new(),
            n => format!(
                ", {} (shown as \\u{{..}})",
                crate::narrative::count(n as u64, "unseen character", "unseen characters")
            ),
        }
    );
    for line in text.split_inclusive('\n') {
        let (body, _) = escape_line(line.strip_suffix('\n').unwrap_or(line));
        out.push_str(&body);
        out.push('\n');
        if !line.ends_with('\n') {
            out.push_str("\\ no line break at the end of the file\n");
        }
    }
    out
}

#[cfg(test)]
mod tests;
