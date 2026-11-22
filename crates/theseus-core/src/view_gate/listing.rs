//! What a list of sessions may show its reader (issue 44): `session.list` in every form (by ids, a
//! directory's scan with each session's first and last text and prompts, a page, the whole list), which
//! the CLI, the cockpit and the Discord binding read. Read once a listing: the board, and the sessions
//! messages were moved out of (each split's `thread.split.<id>` and each claim's `thread.claim.<id>`,
//! with the thread they went to).
//!
//! - A session of a thread held from the reader (sealed, seal-pending, or not proven) is not listed; for
//!   the owner in person it is listed with none of its words (title, first and last text, prompts), the
//!   gate holding a sealed thread's content from him too.
//! - A session a message was moved out of, into a thread held from the reader, shows none of its words
//!   (they may be made from that message); its row is still listed.
//! - Nothing proven is held: a board that does not read gives no session's words, and for a reader who is
//!   not the owner in person no row; a split's or a claim's record that does not read gives no session's
//!   words.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use theseus_memory::gate::Seal;
use theseus_store::{kinds, Store as _};

use crate::threads::Board;
use crate::turn::TurnRunner;

/// What a listing shows of one session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    /// Its row as it is.
    Whole,
    /// Its row with none of its words.
    Unworded,
    /// No row.
    Out,
}

/// One listing's reading: the board and where messages went, read once.
pub struct Listing {
    board: Option<Arc<Board>>,
    /// Each session messages were moved out of, and the threads they went to.
    moved: HashMap<String, BTreeSet<String>>,
    /// A split's or a claim's record did not read.
    unread: bool,
    owner: bool,
}

impl Listing {
    /// For a reader who is the owner in person (`owner`) or not.
    pub fn read(runner: &TurnRunner, owner: bool) -> Self {
        let mut moved: HashMap<String, BTreeSet<String>> = HashMap::new();
        let mut unread = false;
        for (prefix, from) in [
            ("thread.split.", "source_session"),
            ("thread.claim.", "session"),
        ] {
            let Ok(records) = runner.store.inner().latest_with_prefix(kinds::META, prefix) else {
                unread = true;
                continue;
            };
            for r in records {
                let v: Option<serde_json::Value> = r.decode().ok();
                let pair = v.as_ref().and_then(|v| {
                    Some((
                        v[from].as_str()?.to_string(),
                        v["thread"].as_str()?.to_string(),
                    ))
                });
                match pair {
                    Some((session, thread)) => {
                        moved.entry(session).or_default().insert(thread);
                    }
                    None => unread = true,
                }
            }
        }
        Self {
            board: super::board(runner, "a list of sessions"),
            moved,
            unread,
            owner,
        }
    }

    /// What this listing shows of `session`.
    pub fn of(&self, runner: &TurnRunner, session: &str) -> Shown {
        let held = match self.owner {
            true => Shown::Unworded,
            false => Shown::Out,
        };
        let Some(b) = &self.board else {
            return held;
        };
        if super::seal_on(&runner.store, b, None, session) != Seal::Clear {
            return held;
        }
        let moved_held = self
            .moved
            .get(session)
            .is_some_and(|ts| ts.iter().any(|t| super::thread_seal(b, t) != Seal::Clear));
        match moved_held || self.unread {
            true => Shown::Unworded,
            false => Shown::Whole,
        }
    }
}
