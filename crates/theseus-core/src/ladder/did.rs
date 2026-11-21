//! The `did` line (`did.<turn>`): what Theseus did in one exchange, at most
//! [`MAX_CHARS`] characters, written once after the exchange ends, from the
//! person's message and the final reply alone. Tool results are never read
//! for its words; they are read only for the exchange's marks, which the line
//! carries. A source the owner sealed or veiled is not summarised.

use serde::{Deserialize, Serialize};
use theseus_store::{blocking, kinds, NewRecord};

use super::writer::{Ask, Work, Writer, WRITER_VERSION};
use super::{cut, Day, Marks, DAY_KEY};
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
    marks.outside = store
        .session_nodes(&ex.session_id)?
        .iter()
        .any(|(_, n)| n.turn_id.as_deref() == Some(turn) && outside(n));
    Ok(Some(Read {
        ex,
        ask,
        reply,
        marks,
    }))
}

/// What the writer reads for one exchange.
pub fn material(r: &Read) -> String {
    let ask = r.ask.as_deref().map_or(
        "(no message: it was opened by a wake, a report or a late result)".into(),
        |a| cut(a, ASK_CHARS),
    );
    let reply = cut(r.reply.as_deref().unwrap_or(""), REPLY_CHARS);
    format!("The person's message:\n{ask}\n\nTheseus's final reply:\n{reply}")
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
    /// Its source is sealed or veiled.
    Hidden,
    /// No line: why (the day's line, a failed call).
    Failed(String),
}

/// Write exchange `turn`'s line with `writer`, within `cap_usd` of the day's
/// line (or within the day's line alone), unless `hidden` says its session
/// is sealed or veiled. The line and the day's spend are one frame.
pub async fn write(
    store: &Store,
    writer: &dyn Writer,
    turn: &str,
    hidden: &(dyn Fn(&str) -> bool + Sync),
    cap_usd: Option<f64>,
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
    if hidden(&r.ex.session_id) {
        return Ok(Outcome::Hidden);
    }
    let now = theseus_protocol::now_unix_ms();
    let today = crate::judge::spend::local_day(now);
    let day: Day = blocking(|| store.get_meta(DAY_KEY))?.unwrap_or_default();
    let left = cap_usd.unwrap_or_else(|| day.left(&today));
    let ask = Ask {
        work: Work::Did,
        text: material(&r),
        sources: 1,
    };
    let w = writer.write(ask, left).await;
    let line = w.text.as_deref().and_then(line_of);
    let mut frame = Vec::new();
    if w.cost_usd > 0.0 {
        frame.push(NewRecord::json(
            kinds::META,
            Some(DAY_KEY),
            &day.spend(&today, w.cost_usd),
        )?);
    }
    let out = match line {
        Some(line) => {
            let d = DidLine {
                turn_id: turn.to_string(),
                session_id: r.ex.session_id.clone(),
                line,
                model: w.model.clone(),
                writer: WRITER_VERSION,
                marks: r.marks.clone(),
                cost_usd: w.cost_usd,
                at_ms: now,
            };
            frame.push(did_record(&d)?);
            Outcome::Written(d)
        }
        None => Outcome::Failed(w.failed.unwrap_or_else(|| "the answer held no line".into())),
    };
    if !frame.is_empty() {
        blocking(|| store.append(&frame))?;
    }
    Ok(out)
}
