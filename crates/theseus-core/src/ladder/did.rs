//! The `did` line (`did.<turn>`): what Theseus did in one exchange, at most
//! [`MAX_CHARS`] characters, written once after the exchange ends, from the
//! person's message and the final reply alone. Tool results are never read
//! for its words; they are read only for the exchange's marks, which the line
//! carries.

use serde::{Deserialize, Serialize};
use theseus_store::{blocking, kinds, NewRecord};

use super::writer::{Ask, Work, Writer, WRITER_VERSION};
use super::{cut, Marks};
use crate::exchange::{self, Exchange};
use crate::node::{Body, Node};
use crate::store::Store;

/// A `did` line: `did.<turn id>`.
pub const DID_PREFIX: &str = "did.";
/// A line's characters at most.
pub const MAX_CHARS: usize = 200;
/// What a reader shows for an exchange with no line, beside its handle.
pub const NOT_SUMMARISED: &str = "reply not summarised";
/// The most of the message, and of the reply, the writer reads.
pub const ASK_CHARS: usize = 4_000;
pub const REPLY_CHARS: usize = 4_000;

/// One exchange's `did` line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DidLine {
    /// The exchange, by its turn: its handle.
    pub turn_id: String,
    /// Its source session, by which its audience is the source's.
    pub session_id: String,
    pub line: String,
    /// The model that wrote it, as the provider named it, and the version
    /// of the instructions it was written under.
    pub model: String,
    pub writer: u32,
    /// The exchange's marks.
    pub marks: Marks,
    pub cost_usd: f64,
    pub at_ms: u64,
}

/// What a reader shows of an exchange: its line, or that it has none.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    Line(DidLine),
    /// No line was written: shown as [`NOT_SUMMARISED`] and the handle.
    NotSummarised {
        handle: String,
    },
}

impl Said {
    pub fn handle(&self) -> &str {
        match self {
            Said::Line(d) => &d.turn_id,
            Said::NotSummarised { handle } => handle,
        }
    }

    /// The line's words, or [`NOT_SUMMARISED`].
    pub fn text(&self) -> &str {
        match self {
            Said::Line(d) => &d.line,
            Said::NotSummarised { .. } => NOT_SUMMARISED,
        }
    }
}

/// The line of exchange `turn`, if one was written.
pub fn did(store: &Store, turn: &str) -> anyhow::Result<Option<DidLine>> {
    store.get_meta(&format!("{DID_PREFIX}{turn}"))
}

/// What a reader shows of exchange `turn`.
pub fn said(store: &Store, turn: &str) -> anyhow::Result<Said> {
    Ok(match did(store, turn)? {
        Some(d) => Said::Line(d),
        None => Said::NotSummarised {
            handle: turn.to_string(),
        },
    })
}

/// A line's record.
pub fn did_record(d: &DidLine) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&format!("{DID_PREFIX}{}", d.turn_id)), d)
}

/// An exchange as the summaries read it: the record, the person's message's
/// words, the final reply's text, and the marks of everything in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub ex: Exchange,
    pub ask: Option<String>,
    pub reply: Option<String>,
    pub marks: Marks,
    /// Who said the message, by its own mark (`view::sources_store::author_of`): never the owner's on
    /// anything less; none, or a process, is author unknown.
    pub author: crate::view::Author,
}

/// The words of a person's message or of a reply; none for any other node.
pub fn words(n: &Node) -> Option<String> {
    match &n.body {
        Body::UserMessage { text, .. } => Some(text.clone()),
        Body::AssistantMessage { blocks, .. } => Some(super::writer::text_of(blocks)),
        _ => None,
    }
}

/// Whether a node is a tool result whose text came from outside.
pub fn outside(n: &Node) -> bool {
    matches!(
        &n.body,
        Body::ToolResult {
            external: Some(_),
            ..
        }
    )
}

/// Exchange `turn` read from the store; none when no exchange was written.
pub fn read(store: &Store, turn: &str) -> anyhow::Result<Option<Read>> {
    read_with(store, turn, |ex| {
        Ok(store
            .session_nodes(&ex.session_id)?
            .iter()
            .any(|(_, n)| n.turn_id.as_deref() == Some(turn) && outside(n)))
    })
}

/// [`read`], whether the exchange read outside text told by `read_outside` (a reader of many
/// exchanges scans each session once).
pub fn read_with(
    store: &Store,
    turn: &str,
    read_outside: impl FnOnce(&Exchange) -> anyhow::Result<bool>,
) -> anyhow::Result<Option<Read>> {
    let Some(ex) = exchange::exchange(store, turn)? else {
        return Ok(None);
    };
    let text = |id: &Option<String>| -> anyhow::Result<Option<String>> {
        Ok(match id {
            Some(id) => store.get_node(id)?.and_then(|(_, n)| words(&n)),
            None => None,
        })
    };
    let (ask, reply) = (text(&ex.ask)?, text(&ex.reply)?);
    let mut marks = super::marks_of(&ex);
    marks.outside = read_outside(&ex)?;
    let author = match &ex.ask {
        Some(id) => crate::view::sources_store::author_of(store, id),
        None => crate::view::Author::Unknown,
    };
    Ok(Some(Read {
        ex,
        ask,
        reply,
        marks,
        author,
    }))
}

/// What the writer reads for one exchange: the message and the reply, inside
/// the outside-text wrapper when the exchange carries the outside mark (a
/// reply may quote what a page said).
pub fn material(r: &Read) -> String {
    let ask = r.ask.as_deref().map_or(
        "(no message: it was opened by a wake, a report or a late result)".into(),
        |a| cut(a, ASK_CHARS),
    );
    let reply = cut(r.reply.as_deref().unwrap_or(""), REPLY_CHARS);
    let body = format!(
        "{}:\n{ask}\n\nTheseus's final reply:\n{reply}",
        said_by(r.author)
    );
    match r.marks.outside {
        true => super::writer::outside(Some("a tool's result this exchange read"), &body),
        false => body,
    }
}

/// Who said a message, as a summary's writer is told it: the owner's words as before; another person's,
/// and words of unknown authorship, said to be neither his nor his instruction.
pub fn said_by(author: crate::view::Author) -> &'static str {
    use crate::view::Author;
    match author {
        Author::His { .. } => "The person's message",
        Author::Other => "Another person's message (not the owner's)",
        Author::Unknown | Author::Theseus => {
            "A message whose author is unknown (not the owner's, nor his instruction)"
        }
    }
}

/// What an earlier writer's line (before [`super::writer::AUTHORSHIP_SINCE`]) reads as wherever it would
/// be shown, unless every message of its exchange is proven the owner's.
pub const UNAUTHORED_LINE: &str = "(summarised before authorship was recorded; not shown)";

/// A line as it may be shown: as written, unless its writer was not told each message's author and not
/// every message of its exchange is proven the owner's by its own mark (a missing exchange or message
/// proves nothing), when it is [`UNAUTHORED_LINE`]. The history page and the view's line forms read it so.
pub fn shown_line(store: &Store, d: &DidLine) -> String {
    if d.writer >= super::writer::AUTHORSHIP_SINCE {
        return d.line.clone();
    }
    let his = |id: &String| {
        matches!(
            crate::view::sources_store::author_of(store, id),
            crate::view::Author::His { .. }
        )
    };
    let proven = match exchange::exchange(store, &d.turn_id) {
        Ok(Some(ex)) => !ex.messages.is_empty() && ex.messages.iter().all(his),
        _ => false,
    };
    match proven {
        true => d.line.clone(),
        false => UNAUTHORED_LINE.into(),
    }
}

/// The line of a writer's answer: its first line, cut by code.
pub fn line_of(answer: &str) -> Option<String> {
    let first = answer.lines().map(str::trim).find(|l| !l.is_empty())?;
    Some(cut(first, MAX_CHARS))
}

/// What a `did` job did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Written(DidLine),
    /// It had one already: a line is written once.
    Had,
    /// No such exchange, or it is still being answered.
    NotEnded,
    /// Its exchange has no final reply.
    NoReply,
    /// Its place has no ladder (`laddered`).
    NoLadder,
    /// No line: why (the allowance, a failed call), and what the call cost.
    Failed {
        why: String,
        cost_usd: f64,
    },
}

impl Outcome {
    /// What the job spent.
    pub fn cost_usd(&self) -> f64 {
        match self {
            Outcome::Written(d) => d.cost_usd,
            Outcome::Failed { cost_usd, .. } => *cost_usd,
            _ => 0.0,
        }
    }
}

/// Write exchange `turn`'s line with `writer`, its call reserved on `purse`.
pub async fn write(
    store: &Store,
    writer: &dyn Writer,
    turn: &str,
    purse: &dyn super::Purse,
) -> anyhow::Result<Outcome> {
    let _one = super::WRITING.lock().await;
    let Some(r) = blocking(|| read(store, turn))? else {
        return Ok(Outcome::NotEnded);
    };
    if blocking(|| did(store, turn))?.is_some() {
        return Ok(Outcome::Had);
    }
    if r.ex.end_ms.is_none() {
        return Ok(Outcome::NotEnded);
    }
    if r.reply.is_none() {
        return Ok(Outcome::NoReply);
    }
    if !super::laddered(&r.ex.place) {
        return Ok(Outcome::NoLadder);
    }
    let ask = Ask {
        work: Work::Did,
        text: material(&r),
        sources: 1,
    };
    let w = writer.write(ask, purse).await;
    let Some(line) = w.text.as_deref().and_then(line_of) else {
        return Ok(Outcome::Failed {
            why: w.failed.unwrap_or_else(|| "the answer held no line".into()),
            cost_usd: w.cost_usd,
        });
    };
    let d = DidLine {
        turn_id: turn.to_string(),
        session_id: r.ex.session_id.clone(),
        line,
        model: w.model.clone(),
        writer: WRITER_VERSION,
        marks: r.marks,
        cost_usd: w.cost_usd,
        at_ms: theseus_protocol::now_unix_ms(),
    };
    let rec = did_record(&d)?;
    blocking(|| store.append(&[rec]))?;
    Ok(Outcome::Written(d))
}
