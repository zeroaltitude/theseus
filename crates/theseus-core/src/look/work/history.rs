//! `look.work`'s last part: this conversation's own messages, and no other's.

use crate::history_tool::written::{self, Written};
use crate::node::{Body, Node, Origin};
use crate::quotes::{Field, Form};
use crate::scrub::Cuts;
use crate::view::history::Quoted;
use crate::Core;

use super::{clip_kept, LINE_CHARS};

/// Who said a message, in words that are true of it. "The harness" is for its own words alone
/// (`content::harness_own`); a task's report, a wake's note and a retraction's gist carry words from
/// elsewhere and are named as what they are.
pub(super) fn who(origin: Origin, author: Option<&str>) -> &'static str {
    match origin {
        Origin::Operator => "the owner",
        Origin::Harness if crate::content::harness_own(origin, author) => "the harness",
        Origin::Harness => match author.unwrap_or("") {
            a if a.starts_with("task:") => "a task's report",
            a if a.starts_with("wake:") => "a wake's note",
            _ => "a note that carries other words",
        },
        _ => "a message",
    }
}

/// A message as the name it goes under and its words, clipped at a known value's edge (`cuts`), and whether
/// the clip kept any of its bytes; none for a node that is not a message.
#[cfg(test)]
pub(super) fn line_of(n: &Node, cuts: &Cuts) -> Option<(String, String, bool)> {
    said_of(n, 0, cuts).map(|(name, words, some, _)| (name, words, some))
}

/// [`line_of`] for the record at `position`, and the quote its words begin with: none for another profile's
/// answer, which no field of a record reads, so its words are the code's.
fn said_of(n: &Node, position: u64, cuts: &Cuts) -> Option<(String, String, bool, Option<Quoted>)> {
    let said = |name: String, text: &str, field: Option<Field>| {
        let (words, kept) = clip_kept(text, LINE_CHARS, cuts);
        let some = !kept.is_empty();
        let q =
            field.and_then(|f| written::quoted((&n.id, position), (f, text), kept, Form::Clip1));
        (name, words, some, q)
    };
    match &n.body {
        Body::UserMessage { text, .. } if !text.trim().is_empty() => Some(said(
            format!("{}: ", who(n.origin, n.author.as_deref())),
            text.as_str(),
            Some(Field::Text),
        )),
        Body::AssistantMessage { blocks, model, .. } => {
            // A reply's text blocks, as a quote of it reads them.
            let text = crate::provider::text_of(blocks);
            (!text.trim().is_empty()).then(|| {
                said(
                    format!("Theseus ({model}): "),
                    text.as_str(),
                    Some(Field::ReplyText),
                )
            })
        }
        // Another profile's answer, a message of its own (`profile.ask`).
        Body::ToolResult { meta, .. } => {
            theseus_protocol::SubagentAnswer::from_meta(meta).map(|a| {
                said(
                    format!("{} (a subagent, {}): ", a.profile, a.model),
                    a.text.as_str(),
                    None,
                )
            })
        }
        _ => None,
    }
}

/// The part's words, written with its parts, and the messages they show (what its result copies, `copies`): each
/// with where its words are in the part, in characters, `from` to `to`. A message the clip keeps none of (control
/// characters alone, or a known value from its start) shows none, and is not one of them (D6 round 6).
pub(super) fn history(
    core: &Core,
    session: &str,
    messages: usize,
    cuts: &Cuts,
) -> (Written, Vec<(String, usize, usize)>) {
    let mut w = Written::default();
    let nodes = match core.store.session_nodes(session) {
        Ok(n) => n,
        Err(_) => {
            w.code("Messages: they could not be read just now.\n");
            return (w, Vec::new());
        }
    };
    // Each line as the name it goes under and its words, apart, so where its words are is known.
    let mut lines: Vec<_> = nodes
        .iter()
        .filter_map(|(pos, n)| {
            said_of(n, *pos, cuts)
                .map(|(name, words, kept, q)| (name, words, kept, q, n.id.clone()))
        })
        .collect();
    let total = lines.len();
    let kept = lines.split_off(total.saturating_sub(messages));
    if kept.is_empty() {
        w.code("Messages: this conversation has none yet.\n");
        return (w, Vec::new());
    }
    w.code(&format!(
        "The last {} of this conversation's {total} messages:\n",
        kept.len()
    ));
    let mut shown = Vec::new();
    for (name, words, some, q, id) in kept {
        w.code(&format!("- {name}"));
        let from = w.text().chars().count();
        w.said(&words, q.as_ref());
        if some {
            shown.push((id, from, from + words.chars().count()));
        }
        w.code("\n");
    }
    (w, shown)
}
