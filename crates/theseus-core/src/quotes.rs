//! A rendering kept by reference (issue 44): what a look back showed, stored as parts, the code's own words
//! ([`Part::Code`]) and quotes of what nodes said ([`Part::Quote`]), never a second copy of a node's words.
//!
//! - **What was shown then.** A quote names its node's record by its WAL position, the field of it, the bytes of
//!   that field shown after every cut, and the frozen [`Form`] they were shown in; the frame around them
//!   (headings, times, places, marks, the cap's seam) is `Code`, as rendered. So a rendering repeats its bytes:
//!   nothing is cut again and no query is run again when it is laid, whatever has changed in the code since.
//! - **What has been said since.** The one change: a quote whose node is erased now shows [`ERASED_QUOTE`] in
//!   place of its words, and a quoted node marked wrong has the since line after the rendering
//!   (`copies::since`), each once. A quote that cannot be read as it was written shows [`UNREADABLE_QUOTE`],
//!   never a guess: no other record's words, no newer text under its id.
//! - **One renderer.** [`render`] is the only reader of parts. A result that holds parts stores no words of its
//!   own (`content` empty); one stored before them holds its words and no parts, and reads as it always did.

use std::collections::HashSet;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_store::{kinds, Store as _};

use crate::copies::since::Since;
use crate::ladder::did::{DidLine, DID_PREFIX};
use crate::node::{AttachmentContent, Body, Node};
use crate::scrub::Cuts;
use crate::store::Store;

pub mod cap;

/// The one reader of a stored result kept by reference, under the plan's name (`crate::quoted`).
pub use crate::quoted::laid;

/// What a quote of a node that was erased since shows, in place of its words.
pub const ERASED_QUOTE: &str = "[words since erased]";
/// What a quote that cannot be read as it was written shows, in place of its words.
pub const UNREADABLE_QUOTE: &str = "[words that cannot be read now]";

/// One part of a rendering kept by reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// The code's own words, exactly as rendered: never a node's.
    Code(String),
    /// A node's words, by reference.
    Quote(Quote),
}

/// The bytes of one field of one record that a rendering showed, and the form it showed them in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    /// The original's id: a node's, or for [`Field::Did`] its exchange's turn.
    pub node: String,
    /// Its record's WAL position: the words are read there, as first written.
    pub position: u64,
    pub field: Field,
    /// The bytes of the field's text shown, `[from, to)`, after every cut.
    pub from: u32,
    pub to: u32,
    pub form: Form,
}

/// Which text of a record a quote reads. Frozen once stored: what a field reads never changes; a change adds a
/// new field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    /// A person's message's `text`, without its files.
    Text,
    /// A reply's text blocks, in order, joined with nothing between them: no thinking, no tool traffic.
    ReplyText,
    /// An exchange's `did` line, the `did.<turn>` META record's `line`.
    Did,
    /// A node's text as the index reads it: a person's message with each text file after a line break, a reply's
    /// text blocks, an imported message's, a summary's or a synthesis's text.
    Chunk,
}

/// How a quote's bytes are shown. Each is a pure function, frozen once a stored reference names it: a change adds
/// a new numbered form, never edits one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// As they are.
    Verbatim,
    /// One line of data, a history page's (`history_tool::page::flat1`).
    Flat1,
    /// One line, the look's (`look::work::clip1`).
    Clip1,
    /// Each line behind an open's margin (`history_tool::open::margin1`).
    Margin1,
    /// A line of an open without its margin (`history_tool::open::line1`): what a cut through a `Margin1`
    /// quote keeps of the line it cut.
    Line1,
}

impl Form {
    /// `text` in this form.
    pub fn apply(self, text: &str) -> String {
        match self {
            Form::Verbatim => text.to_string(),
            Form::Flat1 => crate::history_tool::page::flat1(text),
            Form::Clip1 => crate::look::clip1(text),
            Form::Margin1 => crate::history_tool::open::margin1(text),
            Form::Line1 => crate::history_tool::open::line1(text),
        }
    }
}

/// What a quote's record reads as now.
pub(crate) enum Read {
    /// The whole text of its field, as first written.
    Field(String),
    /// Its node's newest record is a tombstone.
    Erased,
    /// It cannot be read as it was written.
    Unreadable,
}

/// The text `parts` show: each [`Part::Code`] as it is, each [`Part::Quote`] as its form of the bytes it names, read
/// by position ([`ERASED_QUOTE`] for one erased since, [`UNREADABLE_QUOTE`] for one that cannot be read); then the
/// since line of each node a quote showed that is marked wrong, once each, in the order first quoted.
pub fn render(store: &Store, parts: &[Part], since: &Since<'_>) -> Result<String> {
    let mut shown = Vec::new();
    let text = render_with(parts, |q| {
        let read = read(store, q);
        if matches!(read, Read::Field(_)) {
            shown.push(q.node.clone());
        }
        read
    });
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for id in shown {
        if seen.insert(id.clone()) {
            lines.extend(since.quoted(&id)?);
        }
    }
    Ok(match lines.is_empty() {
        true => text,
        false => format!("{text}\n{}", lines.join("\n")),
    })
}

/// The text `parts` show, each quote's record read by `read`: [`render`] without the since lines.
pub(crate) fn render_with(parts: &[Part], mut read: impl FnMut(&Quote) -> Read) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            Part::Code(c) => out.push_str(c),
            Part::Quote(q) => out.push_str(&shown(q, read(q))),
        }
    }
    out
}

/// What `q` shows, its record read as `read`.
pub(crate) fn shown(q: &Quote, read: Read) -> String {
    match read {
        Read::Field(text) => match text.get(q.from as usize..q.to as usize) {
            Some(bytes) => q.form.apply(bytes),
            None => UNREADABLE_QUOTE.to_string(),
        },
        Read::Erased => ERASED_QUOTE.to_string(),
        Read::Unreadable => UNREADABLE_QUOTE.to_string(),
    }
}

/// `q`'s record as it reads now: its field's text as first written, read at its position, unless its newest record
/// says it was erased. A position that holds another record, a record that does not decode or has no such field,
/// and a node or a line that is no longer there, are all unreadable.
pub(crate) fn read(store: &Store, q: &Quote) -> Read {
    match q.field {
        Field::Did => did(store, q),
        _ => node(store, q),
    }
}

fn node(store: &Store, q: &Quote) -> Read {
    // Erase decides first, by the node as it is now (as recall does, `recall::render::source`).
    match store.get_node(&q.node) {
        Ok(Some((_, n))) if matches!(n.body, Body::Erased { .. }) => return Read::Erased,
        Ok(Some(_)) => {}
        _ => return Read::Unreadable,
    }
    let Some(n) = at(store, q, kinds::NODE, &q.node).and_then(|r| r.decode::<Node>().ok()) else {
        return Read::Unreadable;
    };
    field_of(&n, q.field).map_or(Read::Unreadable, Read::Field)
}

fn did(store: &Store, q: &Quote) -> Read {
    let key = format!("{DID_PREFIX}{}", q.node);
    match store.inner().latest_by_key(kinds::META, &key) {
        Ok(Some(_)) => {}
        _ => return Read::Unreadable,
    }
    match at(store, q, kinds::META, &key).and_then(|r| r.decode::<DidLine>().ok()) {
        Some(d) => Read::Field(d.line),
        None => Read::Unreadable,
    }
}

/// Whether every quote of `parts` begins and ends where `cuts` lets a cut fall in its field (`Cuts::before`):
/// never inside a known value, verbatim or in any encoding the snapshot reads. A range a producer may store; a
/// quote whose field cannot be read is not one. The renderer refuses no in-bounds range, so a producer that
/// stores parts asks this first and keeps its words when it fails.
pub(crate) fn edges_hold(store: &Store, parts: &[Part], cuts: &Cuts) -> bool {
    edges_hold_with(parts, |q| read(store, q), cuts)
}

/// [`edges_hold`], each quote's record read by `read`.
pub(crate) fn edges_hold_with(
    parts: &[Part],
    mut read: impl FnMut(&Quote) -> Read,
    cuts: &Cuts,
) -> bool {
    parts.iter().all(|p| match p {
        Part::Code(_) => true,
        Part::Quote(q) => match read(q) {
            Read::Field(f) => [q.from, q.to].into_iter().all(|at| {
                let at = at as usize;
                at <= f.len() && f.is_char_boundary(at) && cuts.before(&f, at) == at
            }),
            Read::Erased | Read::Unreadable => false,
        },
    })
}

/// The quotes of `parts` whose range begins or ends strictly inside a span `values` take in its field, verbatim or
/// in any encoding the scrubber reads, as the scrubber's own finder sees the field (`scrub::oracle`); a quote that
/// cannot be read counts too. A test's check of the ranges a producer stores: apart from where any cut chose its
/// edge, not from how the scrubber finds a value.
#[cfg(test)]
pub(crate) fn cut_through(
    parts: &[Part],
    mut read: impl FnMut(&Quote) -> Read,
    values: &[&str],
) -> Vec<Quote> {
    let inside = |spans: &[(usize, usize)], at: u32| {
        (spans.iter()).any(|(a, b)| *a < at as usize && (at as usize) < *b)
    };
    (parts.iter())
        .filter_map(|p| match p {
            Part::Quote(q) => Some(q),
            Part::Code(_) => None,
        })
        .filter(|q| match read(q) {
            Read::Field(f) => {
                let spans = crate::scrub::oracle::spans_of(&f, values);
                inside(&spans, q.from) || inside(&spans, q.to)
            }
            Read::Erased | Read::Unreadable => true,
        })
        .cloned()
        .collect()
}

/// Whether `text` holds no part of `value` but whole ones: with each whole `value` taken out, no run of eight of its
/// characters is left. A test's check of what is rendered from a stored range.
#[cfg(test)]
pub(crate) fn no_part_of(text: &str, value: &str) -> bool {
    let rest = text.replace(value, "");
    let chars: Vec<char> = value.chars().collect();
    (chars.windows(8)).all(|w| !rest.contains(&w.iter().collect::<String>()))
}

/// What `q` read when it was written: its record at its position, whether erased since or not. A test's view of
/// a stored result as it was written; no reader shows it.
#[cfg(test)]
pub(crate) fn read_as_written(store: &Store, q: &Quote) -> Read {
    match q.field {
        Field::Did => did(store, q),
        _ => (at(store, q, kinds::NODE, &q.node).and_then(|r| r.decode::<Node>().ok()))
            .and_then(|n| field_of(&n, q.field))
            .map_or(Read::Unreadable, Read::Field),
    }
}

/// The record at `q`'s position, when it is of `kind` under `key`.
fn at(store: &Store, q: &Quote, kind: u16, key: &str) -> Option<theseus_store::Record> {
    let rec = store.inner().get(q.position).ok().flatten()?;
    (rec.kind == kind && rec.key.as_deref() == Some(key)).then_some(rec)
}

/// `n`'s text in `field`; none when its body has no such text.
fn field_of(n: &Node, field: Field) -> Option<String> {
    match (field, &n.body) {
        (Field::Text, Body::UserMessage { text, .. }) => Some(text.clone()),
        (Field::ReplyText | Field::Chunk, Body::AssistantMessage { blocks, .. }) => {
            Some(reply_text(blocks))
        }
        (
            Field::Chunk,
            Body::UserMessage {
                text, attachments, ..
            },
        ) => {
            let mut t = text.clone();
            for a in attachments {
                if let AttachmentContent::Text { text, .. } = &a.content {
                    t.push('\n');
                    t.push_str(text);
                }
            }
            Some(t)
        }
        (
            Field::Chunk,
            Body::Imported { text, .. }
            | Body::ImportedSummary { text, .. }
            | Body::Summary { text, .. }
            | Body::Synthesis { text, .. },
        ) => Some(text.clone()),
        _ => None,
    }
}

/// A reply's text blocks, in order, joined with nothing between them.
fn reply_text(blocks: &[serde_json::Value]) -> String {
    blocks
        .iter()
        .filter(|b| b.get("type").and_then(serde_json::Value::as_str) == Some("text"))
        .filter_map(|b| b.get("text").and_then(serde_json::Value::as_str))
        .collect()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_oracle;
