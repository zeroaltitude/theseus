//! What capture keeps between messages (theseus-slvl.5): the below-band
//! corrections it is watching for a second, the asks waiting on the owner,
//! the "said it again" times of each record, and which judgment kept which
//! record (so an undo can label it). One META record, `standing.capture`
//! (no store format: a new key), rewritten whole in the frame of whatever
//! changed it; read once, by the first capture, never on a turn's path.

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use theseus_protocol::StandingAsk;
use theseus_store::{kinds, NewRecord};

use crate::store::Store;

pub const KEY: &str = "standing.capture";

pub const WEEK_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// A repeat is counted for the week; older ones are dropped at a month.
const REPEAT_KEEP_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const MAX_CANDIDATES: usize = 20;
const MAX_ASKS: usize = 20;
const MAX_KEPT: usize = 200;

/// A correction below the asking band, watched for a second.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub node: String,
    pub line: String,
    pub at_ms: u64,
}

/// An ask, with the judgment that raised it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Open {
    pub ask: StandingAsk,
    pub judgment: String,
    pub session: String,
}

/// A record `directive.v1` kept, and the judgment that said to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub record: String,
    pub judgment: String,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub candidates: Vec<Candidate>,
    #[serde(default)]
    pub asks: Vec<Open>,
    #[serde(default)]
    pub repeats: BTreeMap<String, Vec<u64>>,
    #[serde(default)]
    pub kept: Vec<Kept>,
}

impl State {
    /// Drop what has lapsed at `now`: candidates and asks past a week, repeats
    /// past a month; and cap each list.
    pub fn trim(&mut self, now: u64) {
        self.candidates
            .retain(|c| now.saturating_sub(c.at_ms) <= WEEK_MS);
        self.asks
            .retain(|o| now.saturating_sub(o.ask.at_ms) <= WEEK_MS);
        for v in self.repeats.values_mut() {
            v.retain(|t| now.saturating_sub(*t) <= REPEAT_KEEP_MS);
        }
        self.repeats.retain(|_, v| !v.is_empty());
        drop_oldest(&mut self.candidates, MAX_CANDIDATES);
        drop_oldest(&mut self.asks, MAX_ASKS);
        drop_oldest(&mut self.kept, MAX_KEPT);
    }

    /// How many times `record` was said again in the week up to `now`.
    pub fn week_of(&self, record: &str, now: u64) -> usize {
        self.repeats.get(record).map_or(0, |v| {
            v.iter()
                .filter(|t| now.saturating_sub(**t) <= WEEK_MS)
                .count()
        })
    }

    /// The record as a META record, to ride the frame that changed it.
    pub fn row(&self) -> Result<NewRecord> {
        NewRecord::json(kinds::META, Some(KEY), self)
    }
}

fn drop_oldest<T>(v: &mut Vec<T>, max: usize) {
    if v.len() > max {
        v.drain(..v.len() - max);
    }
}

/// The state in memory, and the lock whoever changes it holds across its
/// write.
#[derive(Default)]
pub struct Capture {
    state: Mutex<Option<State>>,
}

/// The state, locked: read from the store if nothing has yet.
pub struct Held<'a>(MutexGuard<'a, Option<State>>);

impl std::ops::Deref for Held<'_> {
    type Target = State;
    fn deref(&self) -> &State {
        self.0.as_ref().expect("loaded")
    }
}

impl Held<'_> {
    /// Hold `next` as the state, once its frame is written.
    pub fn set(&mut self, next: State) {
        *self.0 = Some(next);
    }
}

impl Capture {
    /// Lock the state. Blocks on the store once, for the first read: call it
    /// from a blocking context, never a turn's path.
    pub fn lock(&self, store: &Store) -> Result<Held<'_>> {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            *g = Some(store.get_meta::<State>(KEY)?.unwrap_or_default());
        }
        Ok(Held(g))
    }

    /// The state as it is held now, if it has been read: a read for a list,
    /// never a turn.
    pub fn peek(&self, store: &Store) -> Result<State> {
        Ok((*self.lock(store)?).clone())
    }
}
