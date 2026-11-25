//! An exchange's parts, each labelled by its own provenance ([`labels::part_label`]): never one label over
//! an exchange. The person's words are his (here or from elsewhere) only when that message's own author is
//! the owner; another person's message and each result from outside are wrapped on their own; words of
//! unknown authorship are headed "author unknown"; Theseus's replies, its calls and its tool results are the
//! conversation here in the window's real turns, and testimony anywhere else.

use crate::recall::stamp::ZoneAt;
use crate::view::render::MarkedBlock;
use crate::view::{Author, Handle, Label, Reasons, SourceRange};

use super::forms::{self, Said, Voice};
use super::labels::{self, Origin};

/// How an exchange is seen: where and when it was said, why it is shown, and whether it is this place's.
#[derive(Debug, Clone)]
pub struct Seen<'a> {
    /// Its place and time (`source` is unused: each outside result names its own).
    pub origin: Origin<'a>,
    /// Said in this place: the owner's words in it are the conversation here.
    pub here: bool,
    /// A real turn of this place's window: Theseus's side in it is the conversation here, not testimony.
    pub live: bool,
    pub reasons: &'a Reasons,
    pub range: SourceRange,
    pub handle: Handle,
    /// Who another person is, by the owner's records, to head their words with; `None` heads them as
    /// another person's.
    pub person: Option<&'a str>,
}

/// One part, labelled by its own provenance: its heading (`None` for the conversation here) and its text
/// as shown (outside text wrapped, on its own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkedPart {
    pub label: Label,
    pub heading: Option<String>,
    pub text: String,
    pub voice: Voice,
    /// The node it is read from; empty for a cut's mark.
    pub node: String,
}

impl MarkedPart {
    /// The part as one turn's text: its heading in brackets on the line above, when it has one.
    pub fn turn_text(&self) -> String {
        match &self.heading {
            Some(h) => format!("[{h}]\n{}", self.text),
            None => self.text.clone(),
        }
    }
}

/// Each part of an exchange labelled on its own. `author_of` gives the author of a person's message (the
/// owner's mark, the chat service's proof, or unknown), by the part's node; every other part is Theseus's
/// side, or outside text when its result came from outside.
pub fn mark_parts(
    parts: &[Said],
    author_of: &dyn Fn(&Said) -> Author,
    seen: &Seen<'_>,
    zone: ZoneAt<'_>,
) -> Vec<MarkedPart> {
    parts
        .iter()
        .map(|p| {
            let (author, outside) = match p.voice {
                Voice::Person => (author_of(p), false),
                // The harness's own line: no person's, and its label is its own (below).
                Voice::Harness => (Author::Unknown, false),
                Voice::Result => (Author::Theseus, p.outside.is_some()),
                Voice::Theseus | Voice::Call | Voice::Mark => (Author::Theseus, false),
            };
            let label = match p.voice {
                Voice::Harness => Label::Harness,
                _ => labels::part_label(author, outside, seen.here, seen.live),
            };
            let text = match (label, p.voice) {
                // Another person's words: wrapped on their own, as any outside text.
                (Label::OutsideText, Voice::Person) => format!(
                    "person: {}",
                    labels::wrap_outside(Some(seen.person.unwrap_or("another person")), &p.text)
                ),
                _ => forms::said_text(p),
            };
            let source = match p.voice {
                Voice::Person => Some(seen.person.unwrap_or("another person")),
                _ => p.outside.as_deref(),
            };
            let origin = Origin {
                source,
                ..seen.origin
            };
            MarkedPart {
                label,
                heading: labels::heading(label, &origin, seen.reasons, zone),
                text,
                voice: p.voice,
                node: p.node.clone(),
            }
        })
        .collect()
}

/// An exchange as marked blocks, one provenance each: [`mark_parts`], with Theseus's side run together where
/// consecutive parts share a label (a reply, its calls and their results), and each person's message a block
/// of its own.
pub fn exchange_blocks(
    parts: &[Said],
    author_of: &dyn Fn(&Said) -> Author,
    seen: &Seen<'_>,
    zone: ZoneAt<'_>,
) -> Vec<MarkedBlock> {
    let mut out: Vec<(MarkedPart, Vec<String>)> = Vec::new();
    for m in mark_parts(parts, author_of, seen, zone) {
        match out.last_mut() {
            Some((last, texts))
                if last.label == m.label
                    && last.heading == m.heading
                    && last.voice != Voice::Person
                    && m.voice != Voice::Person =>
            {
                texts.push(m.text);
            }
            _ => {
                let text = m.text.clone();
                out.push((m, vec![text]));
            }
        }
    }
    out.into_iter()
        .map(|(m, texts)| MarkedBlock {
            label: m.label,
            heading: m.heading.unwrap_or_default(),
            text: texts.join("\n"),
            sources: vec![seen.range.clone()],
            handle: Some(seen.handle.clone()),
        })
        .collect()
}
