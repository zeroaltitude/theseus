//! A proposed setting's `source`: where the model says it found a fact its maker
//! publishes (issue 26; `docs/design/public-facts.md`).
//!
//! A model may look a published fact up (a model's exact name, a provider's
//! address and protocol, a server's command) instead of asking the owner for
//! it, and say where it found it. That text is **the model's word and nothing
//! more**: the daemon fetched nothing and checked nothing, and the card says so
//! in the same line. It is one line, cut at [`SOURCE_CHARS`] characters before
//! its unseen characters are written out, drawn after a fixed label inside
//! quotes that it cannot close, so it can end no line, begin none and pass for
//! none of the card's own. It never replaces the card's other lines: the value
//! itself, the line that a turn read outside text, the second proof and the
//! reason are built as they were.

use super::diff::{self, Setting};
use crate::config::sensitivity;

/// The longest a source is shown, in characters; a longer one is cut and says so.
pub const SOURCE_CHARS: usize = 200;

/// A source the model gave for the setting at `path` (the path as it named it:
/// a table covers every setting inside it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sourced {
    pub path: Vec<String>,
    pub text: String,
}

/// The model's source on one line, cut at [`SOURCE_CHARS`], or `None` when it
/// gave none. Characters that cannot be seen are written out when it is drawn.
pub fn clean(raw: Option<&str>) -> Option<String> {
    let one = raw?.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.is_empty() {
        return None;
    }
    Some(crate::scrub::safe::head_marked(&one, SOURCE_CHARS))
}

/// The source the model gave for the setting `path`, as the card keeps it.
pub fn of(path: &[String], raw: Option<&str>) -> Option<Sourced> {
    Some(Sourced {
        path: path.to_vec(),
        text: clean(raw)?,
    })
}

/// The card's line for a source: its own label, which says whose word it is and
/// that nothing here checked it, then the text as an escaped one-line literal.
pub fn line(text: &str) -> String {
    format!(
        "  Theseus's source for this, in its own words (nothing here checked it): \"{}\"\n",
        diff::escaped(text)
    )
}

/// The sources that cover the setting `path`: those named at it, or at a table
/// that holds it.
pub fn covering<'a>(
    sources: &'a [Sourced],
    path: &'a [String],
) -> impl Iterator<Item = &'a Sourced> {
    sources.iter().filter(move |s| path.starts_with(&s.path))
}

/// Whether the setting is an address that Theseus sends to. A source on one is
/// shown before the others: the address is where a secret goes.
pub fn names_destination(path: &[String]) -> bool {
    path.last()
        .and_then(|k| sensitivity::key_reason(k))
        .is_some_and(|why| why == sensitivity::ADDRESS_REASON)
}

/// The settings in the order a card lists them: those that name where a secret
/// is sent and carry a source first, the rest as they were.
pub fn in_card_order<'a>(settings: &'a [Setting], sources: &[Sourced]) -> Vec<&'a Setting> {
    let first =
        |s: &&Setting| names_destination(&s.path) && covering(sources, &s.path).next().is_some();
    let (head, rest): (Vec<&Setting>, Vec<&Setting>) = settings.iter().partition(first);
    head.into_iter().chain(rest).collect()
}

#[cfg(test)]
mod tests;
