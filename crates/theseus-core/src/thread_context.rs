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
//! - **`thread.note`**, the model's own line on the card, went with threads
//!   on 2026-11-22 (issue 44); its name stays below only so a session that is
//!   no thread's is offered no such tool.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;
use theseus_store::{kinds, NewRecord};

use crate::compiler::card::{self, Card};
use crate::compiler::{Compiled, RequestSpec};
use crate::turn::TurnRunner;

/// The name the card tool had.
pub const NOTE: &str = "thread.note";
/// The name the model's split tool had.
const SPLIT: &str = "thread.split";

/// The META key of the card a compilation took.
pub fn card_key(compilation_id: &str) -> String {
    format!("thread.card.{compilation_id}")
}

/// The cards compilations took, in memory beside their records.
#[derive(Default)]
pub struct Cards(Mutex<HashMap<String, String>>);

impl Cards {
    /// The card held for compilation `id`, for a test that a read installs none.
    #[cfg(test)]
    pub(crate) fn held(&self, id: &str) -> Option<String> {
        self.get(id)
    }

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
        self.helper_spec(spec, session_id);
        let board = self
            .cfg
            .threads
            .enabled
            .then(|| self.threads.all(&self.store).ok())
            .flatten();
        let Some(t) = board.as_ref().and_then(|b| b.of_session(session_id)) else {
            // The tools are a thread's own.
            let own = [NOTE, SPLIT].map(theseus_tools::wire_name);
            spec.tools = spec.tools.reduced("not_a_threads", |all| {
                let mine = |d: &&Value| !own.iter().any(|n| d["name"] == n.as_str());
                all.iter().filter(mine).cloned().collect()
            });
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
}
