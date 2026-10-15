//! A thread carries only its own working set (theseus-h4a7, thread-context;
//! the owner, 2026-10-09). The layers a request is made of are `compiler::layers`
//! and the card is `compiler::card`; this is what the turn does with them:
//!
//! - **The card's snapshot.** A thread's session renders the thread's card at
//!   the head of its messages. The card a compilation took is kept with it,
//!   as the META record `thread.card.<compilation id>` written in the
//!   compilation's own frame (`card_record`; a new META key, no store format
//!   change), so every append renders the same bytes, and a recompile takes
//!   the card as the thread stands (`TurnRunner::thread_spec`).
//! - **`thread.note {line}`**, the model's own line on the card, for a
//!   threaded session only. It rides the call's completion frame with the
//!   thread's record, one version further; the prefix a request replays is
//!   the snapshot's, so the line shows in the next recompile's card (and, in
//!   the meantime, in the call itself).
//! - **The cold resume** (`TurnRunner::cold_spec`): a thread's next message
//!   after its cache window (`stubs::cold`, as the stubs decide it) is
//!   compiled small, `compiler::cold`. Never for an unthreaded session (that
//!   is `cold-compact`, a separate decision), and a warm thread never.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::threads::{ThreadInfo, MAX_CARD, MAX_LINE_CHARS};
use theseus_store::{kinds, NewRecord};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use crate::compiler::card::{self, Card};
use crate::compiler::cold::Cold;
use crate::compiler::{Compiled, RequestSpec};
use crate::fact::thread::{Change, ThreadEdited};
use crate::toolrun::TurnCtx;
use crate::turn::TurnRunner;

pub const NOTE: &str = "thread.note";

/// The META key of the card a compilation took.
pub fn card_key(compilation_id: &str) -> String {
    format!("thread.card.{compilation_id}")
}

/// The cards compilations took, in memory beside their records.
#[derive(Default)]
pub struct Cards(Mutex<HashMap<String, String>>);

impl Cards {
    fn get(&self, id: &str) -> Option<String> {
        self.0.lock().unwrap().get(id).cloned()
    }

    fn put(&self, id: &str, card: &str) {
        let mut m = self.0.lock().unwrap();
        // Compilations are many and cards are small; the newest are the ones read.
        if m.len() > 256 {
            m.clear();
        }
        m.insert(id.to_string(), card.to_string());
    }
}

/// The record of the card a new compilation took, for the frame that writes
/// the compilation. None for a session with no card, or an append.
pub fn card_record(compiled: &Compiled) -> Option<NewRecord> {
    let card = compiled
        .card
        .as_ref()
        .filter(|_| compiled.new_compilation)?;
    NewRecord::json(kinds::META, Some(&card_key(&compiled.compilation.id)), card).ok()
}

impl TurnRunner {
    /// Give a threaded session's spec its card: the thread as it stands (what a
    /// recompile takes) and the snapshot of the compilation in effect (what an
    /// append renders). Nothing for a session no thread's place runs on.
    pub(crate) fn thread_spec(
        &self,
        spec: &mut RequestSpec,
        session_id: &str,
        compilation_id: Option<&str>,
    ) {
        let board = self
            .cfg
            .threads
            .enabled
            .then(|| self.threads.all(&self.store).ok())
            .flatten();
        let Some(t) = board.as_ref().and_then(|b| b.of_session(session_id)) else {
            // The tools are a thread's own.
            let own = [NOTE, crate::thread_split::SPLIT].map(theseus_tools::wire_name);
            spec.tools.retain(|d| !own.iter().any(|n| d["name"] == n.as_str()));
            return;
        };
        let now = card::text(t);
        let recorded = compilation_id.map(|c| {
            self.cards.get(c).or_else(|| {
                self.store
                    .get_meta::<String>(&card_key(c))
                    .ok()
                    .flatten()
                    // A compilation made before its card was kept: the card as
                    // it first stands is its, from here on.
                    .or_else(|| Some(now.clone()))
                    .inspect(|card| self.cards.put(c, card))
            })
        });
        spec.card = Some(Card::new(now, recorded.flatten()));
    }

    /// The topics of the thread a session serves.
    pub(crate) fn thread_topics(&self, session_id: &str) -> Option<Vec<String>> {
        let board = self.threads.all(&self.store).ok()?;
        board.of_session(session_id).map(|t| t.topics.clone())
    }

    /// The spec of a turn's first loop when its thread resumes cold: its next
    /// message came after the cache window (`cold`: the turn's first compile
    /// decided it), so the compile rewrites it small. None: the spec stands.
    pub(crate) fn cold_spec(&self, spec: &RequestSpec, cold: Option<bool>) -> Option<RequestSpec> {
        if cold != Some(true) || spec.card.is_none() {
            return None;
        }
        let t = &self.cfg.threads;
        let mut s = spec.clone();
        s.cold = Some(Cold {
            keep_turns: t.resume_keep_turns,
            working_tokens: t.working_tokens,
        });
        Some(s)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteInput {
    line: String,
}

/// `thread.note`: one line on the thread's card.
pub struct ThreadNote;

impl Tool for ThreadNote {
    fn name(&self) -> &'static str {
        NOTE
    }

    fn description(&self) -> &'static str {
        "Add one line to this thread's card, where it stands (a decision, a finding, what is \
         next); the card keeps the newest 12 and shows at the head of the thread after its \
         next rewrite. Only in a thread."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {"line": {"type": "string", "description": "One sentence."}},
            "required": ["line"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Write
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        // Run again after a crash, the line would be there twice.
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i: NoteInput = parse(input)?;
        Ok(Plan {
            summary: format!("note on the thread's card: {}", i.line.trim()),
            ..Default::default()
        })
    }
}

/// The card with `line` added: the newest `MAX_CARD` lines; a line the card
/// already ends with is not added again.
fn with_line(card: &[String], line: &str) -> Vec<String> {
    let mut next = card.to_vec();
    if next.last().is_none_or(|l| l != line) {
        next.push(line.to_string());
    }
    let over = next.len().saturating_sub(MAX_CARD);
    next.drain(..over);
    next
}

/// Run `thread.note` for the turn `tc`. The thread's new record and its row
/// are returned for the frame that settles the call, and the board holds the
/// new card at once.
pub fn note(tc: &TurnCtx<'_>, input: &Value) -> Result<((String, Value), Vec<NewRecord>), String> {
    let i: NoteInput = parse(input)?;
    let line = i.line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return Err("Not noted: give one sentence.".into());
    }
    if line.chars().count() > MAX_LINE_CHARS {
        return Err(format!(
            "Not noted: a card line is at most {MAX_LINE_CHARS} characters."
        ));
    }
    let threads = tc.threads;
    let (next, records) = threads
        .noted(tc.store, tc.session_id, |t| {
            let next = ThreadInfo {
                card: with_line(&t.card, &line),
                ..t.clone()
            };
            let row = crate::fact::row(
                &ThreadEdited(Change {
                    thread: &next,
                    who: "model",
                    via: NOTE,
                    changed: &["card"],
                }),
                Some(tc.session_id),
                Some(tc.turn_id),
            )?;
            let stored = ThreadInfo {
                session_id: None,
                ..next.clone()
            };
            let rec =
                NewRecord::json(kinds::META, Some(&crate::threads::key(&next.slug)), &stored)?;
            Ok((next, vec![rec, row]))
        })
        .map_err(|e| format!("Not noted: {e:#}"))?;
    Ok((
        (
            format!(
                "Noted on the card of {} ({} of {MAX_CARD} lines). It shows at the head of this \
                 thread after its next rewrite.",
                next.slug,
                next.card.len()
            ),
            json!({"thread": next.slug, "lines": next.card.len()}),
        ),
        records,
    ))
}
