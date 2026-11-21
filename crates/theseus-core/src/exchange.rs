//! Exchanges, touches and the owner's mark (issue 44, step 1; the design note
//! is `docs/design/exchanges.md`): small records written beside the
//! conversation, never in it, so that later steps have something true to
//! read. Nothing here is sent to a model, and nothing here decides who may
//! act: the mark records what `places::owner_in_person_of` said, it asks
//! nothing new of it.
//!
//! - **The owner's mark** (`said.<node>`): for each person's message, whether
//!   the owner's in-person rule held for it, whether a chat author holds an
//!   owner handle, what the daemon could tell of the process behind it
//!   (`Peer`), and the place it arrived in, taken before thread routing can
//!   send it elsewhere. Written as the message's node is, never after: a
//!   restart forgets the live proof, and an old message cannot be marked.
//! - **An exchange** (`exchange.<turn>`): one turn of a conversation's place,
//!   from what opened it to its last reply, by node ids, never by text. Its
//!   opening half rides the turn's first frames, so a message still being
//!   answered is found at once; its end rides the turn's last frame, as the
//!   same key again.
//! - **A touch** (`touch.<root>␁<newest first>␁<turn>`): "this exchange touched
//!   this root". The time is inverted in the key, so the newest `n` of a root
//!   are the first `n` keys of its prefix (`Store::first_with_prefix`).
//! - **The measuring row** (`context.turn`, [`ledger`]): one per turn in a
//!   private place, ids, kinds and counts only.
//!
//! All of it rides frames the turn writes anyway (`Store::defer`): a turn
//! writes no frame more for it. New META families need no store-format step;
//! no field is added to any stored record.

use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use theseus_store::{kinds, NewRecord, Store as _};

use crate::approval::Surface;
use crate::peer_proc::{self, Behind};
use crate::store::Store;

pub mod ledger;
pub mod rebuild;

/// The owner's mark of a node: `said.<node id>`.
pub const SAID_PREFIX: &str = "said.";
/// An exchange: `exchange.<turn id>`.
pub const EXCHANGE_PREFIX: &str = "exchange.";
/// A touch: `touch.<root>␁<u64::MAX - ms, 20 digits>␁<turn id>`.
pub const TOUCH_PREFIX: &str = "touch.";
/// Between a touch key's parts: no root holds it.
const SEP: char = '\u{1}';
/// A thing's term longer than this is not a root (a key stays small).
const THING_MAX: usize = 200;

/// What the daemon could tell of the process behind an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Peer {
    /// The terminal with no agent on its process chain, or the web UI's page
    /// with none: a person at this machine, by the light guard of `peer_proc`.
    Person,
    /// An agent host on the process chain (`[policy] agent_processes`).
    Agent,
    /// A process under the daemon itself: a job a turn ran.
    Job,
    /// No controlling terminal: a service, a cron, a gateway's exec.
    NoTerminal,
    /// The process could not be read.
    Untraced,
    /// A chat transport, which names its author itself.
    Transport,
    /// A surface with no process to read (MCP, a connection no listener named).
    Unknown,
}

impl Peer {
    /// The class of `behind`, on `surface`: `peer_proc`'s own names.
    pub fn of(surface: Surface, behind: &Behind) -> Self {
        match (surface, behind) {
            (Surface::Discord, _) => Peer::Transport,
            (Surface::Cli | Surface::Web, Behind::Owner) => Peer::Person,
            (Surface::Cli | Surface::Web, Behind::Agent(name)) => match name.as_str() {
                peer_proc::JOB => Peer::Job,
                peer_proc::NO_TERMINAL => Peer::NoTerminal,
                peer_proc::UNTRACED => Peer::Untraced,
                _ => Peer::Agent,
            },
            _ => Peer::Unknown,
        }
    }
}

/// The owner's mark of one message (`said.<node>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerMark {
    /// The message's node.
    pub node: String,
    /// `places::owner_in_person_of` held for it: the owner, in person.
    pub in_person: bool,
    /// On a chat transport, whether its author holds an owner handle; none
    /// on the owner's own surfaces.
    pub owner_handle: Option<bool>,
    pub peer: Peer,
    /// The agent `peer_proc` named, when it named one.
    pub agent: Option<String>,
    /// `cli`, `web`, `discord`, `voice`, `mcp`.
    pub surface: String,
    /// Where it arrived ([`place_of`]), before any thread routing.
    pub place: String,
    /// The place's own session it arrived at, and the one it ran in (a
    /// thread's, when routing took it).
    pub arrived_session: String,
    pub ran_session: String,
    /// Its chat author's id (`discord:<id>`), when a transport named one.
    pub author: Option<String>,
    pub at_ms: u64,
}

impl OwnerMark {
    /// The person of the message: `owner` when the mark proves it, a chat
    /// author by id otherwise, none for a process that is no person.
    pub fn person(&self) -> Option<String> {
        if self.in_person || self.owner_handle == Some(true) {
            return Some(OWNER.into());
        }
        match self.peer {
            Peer::Transport => self.author.clone(),
            _ => None,
        }
    }
}

/// The owner as a person root.
pub const OWNER: &str = "owner";

/// The place a message arrived in: its binding's target, else this machine
/// (`local`) in the directory the session works in.
pub fn place_of(binding: Option<&str>, dir: Option<&str>) -> String {
    match (binding, dir) {
        (Some(b), _) => b.to_string(),
        (None, Some(d)) => format!("local:{d}"),
        (None, None) => "local".into(),
    }
}

/// What opened an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Opener {
    /// A typed message (whose, the mark says).
    Message,
    /// An MCP server's prompt the owner ran.
    Prompt,
    /// A wake that came due.
    Wake,
    /// A task's report.
    Report,
    /// The driver's continuation: a late result, a job's end.
    Continuation,
    /// Not known: an exchange rebuilt from nodes that said nothing of it.
    Unknown,
}

/// One exchange (`exchange.<turn>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    pub turn_id: String,
    pub session_id: String,
    /// [`place_of`]; `unknown` for history no record proves.
    pub place: String,
    /// The place's class was private for this turn.
    pub private: bool,
    pub opened_by: Opener,
    /// [`OwnerMark::person`], or what the rebuild proved.
    pub person: Option<String>,
    pub start_ms: u64,
    /// None while it is still being answered.
    pub end_ms: Option<u64>,
    /// The message that opened it, and the final reply, by node id.
    pub ask: Option<String>,
    pub reply: Option<String>,
    /// Every message of the person's in it, steers included, in order.
    pub messages: Vec<String>,
    /// How it ended (`end_turn`, `stopped`, …).
    pub stop: Option<String>,
    /// Written by `exchange.rebuild` from the nodes, not as it happened.
    pub rebuilt: bool,
}

/// A root an exchange touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root<'a> {
    Place(&'a str),
    Person(&'a str),
    /// A term of the index's extractor, `type:value`.
    Thing(&'a str),
}

impl Root<'_> {
    pub fn key(&self) -> String {
        match self {
            Root::Place(p) => format!("place:{p}"),
            Root::Person(p) => format!("person:{p}"),
            Root::Thing(t) => format!("thing:{t}"),
        }
    }
}

/// One touch's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Touch {
    pub exchange: String,
    pub session_id: String,
    pub at_ms: u64,
}

fn touch_key(root: &str, at_ms: u64, turn: &str) -> String {
    format!(
        "{TOUCH_PREFIX}{root}{SEP}{:020}{SEP}{turn}",
        u64::MAX - at_ms
    )
}

/// The touch of `root` by exchange `turn` at `at_ms`, as a record; none for a
/// thing's term too long to be a key.
pub fn touch(root: Root<'_>, turn: &str, session: &str, at_ms: u64) -> Option<NewRecord> {
    if matches!(root, Root::Thing(t) if t.len() > THING_MAX || t.contains(SEP)) {
        return None;
    }
    let value = Touch {
        exchange: turn.into(),
        session_id: session.into(),
        at_ms,
    };
    NewRecord::json(
        kinds::META,
        Some(&touch_key(&root.key(), at_ms, turn)),
        &value,
    )
    .ok()
}

/// The newest `limit` touches of `root`, newest first: one read of the
/// index's first keys under its prefix, whatever its history.
pub fn newest(store: &Store, root: Root<'_>, limit: usize) -> anyhow::Result<Vec<Touch>> {
    let prefix = format!("{TOUCH_PREFIX}{}{SEP}", root.key());
    store
        .inner()
        .first_with_prefix(kinds::META, &prefix, limit)?
        .iter()
        .map(|r| r.decode())
        .collect()
}

/// The newest `limit` touches of `root` strictly older than `before_ms`,
/// newest first: the next page back through its timeline, one bounded read
/// from the key that time sorts to.
pub fn newest_before(
    store: &Store,
    root: Root<'_>,
    before_ms: u64,
    limit: usize,
) -> anyhow::Result<Vec<Touch>> {
    let prefix = format!("{TOUCH_PREFIX}{}{SEP}", root.key());
    // Past every key of `before_ms` itself: its time, then a byte above `SEP`.
    let from = format!("{prefix}{:020}\u{2}", u64::MAX - before_ms);
    store
        .inner()
        .first_with_prefix_from(kinds::META, &prefix, &from, limit)?
        .iter()
        .map(|r| r.decode())
        .collect()
}

/// An exchange's record.
pub fn exchange_record(e: &Exchange) -> anyhow::Result<NewRecord> {
    NewRecord::json(
        kinds::META,
        Some(&format!("{EXCHANGE_PREFIX}{}", e.turn_id)),
        e,
    )
}

/// The exchange of turn `turn`, as last written.
pub fn exchange(store: &Store, turn: &str) -> anyhow::Result<Option<Exchange>> {
    store.get_meta(&format!("{EXCHANGE_PREFIX}{turn}"))
}

/// A node's mark, if it was marked.
pub fn mark(store: &Store, node: &str) -> anyhow::Result<Option<OwnerMark>> {
    store.get_meta(&format!("{SAID_PREFIX}{node}"))
}

/// A mark's record.
pub fn mark_record(m: &OwnerMark) -> anyhow::Result<NewRecord> {
    NewRecord::json(kinds::META, Some(&format!("{SAID_PREFIX}{}", m.node)), m)
}

/// How long a staged arrival waits for its node, and how many wait at once.
const LIFE: Duration = Duration::from_secs(600);
const KEPT: usize = 64;

/// The arrivals `turn.submit` staged for the inbound point (the mark is made
/// where the live proof is, and written where the node is): by session and
/// the message's words, held as a hash. At most [`KEPT`], each for [`LIFE`].
#[derive(Default)]
pub struct Arrivals(Mutex<VecDeque<(String, u64, Instant, OwnerMark)>>);

fn digest(text: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl Arrivals {
    /// `mark`, for the message `text` that will run in `session`.
    pub fn stage(&self, session: &str, text: &str, mark: OwnerMark) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        q.retain(|(_, _, at, _)| at.elapsed() < LIFE);
        if q.len() >= KEPT {
            q.pop_front();
        }
        q.push_back((session.into(), digest(text), Instant::now(), mark));
    }

    /// The arrival staged for `text` in `session`, taken.
    pub fn take(&self, session: &str, text: &str) -> Option<OwnerMark> {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let d = digest(text);
        let i = q.iter().position(|(s, h, _, _)| s == session && *h == d)?;
        q.remove(i).map(|(_, _, _, m)| m)
    }
}

/// The places a reply's things wait for, at most.
const REPLIES_KEPT: usize = 256;

/// A final reply's things being named, for the place's next exchange to touch.
pub struct Pending {
    pub turn: String,
    pub session: String,
    pub at_ms: u64,
    pub rx: tokio::sync::oneshot::Receiver<Vec<String>>,
    /// Its numbered items, named apart (`names::items`).
    pub items: Option<tokio::sync::oneshot::Receiver<Vec<String>>>,
}

/// What the turns share of their exchanges: the arrivals staged for the
/// inbound point, and each place's last reply being named. Small, bounded,
/// and lost at a restart, which loses only what `exchange.rebuild` restores.
#[derive(Default)]
pub struct Board {
    pub arrivals: Arrivals,
    replies: Mutex<std::collections::HashMap<String, Pending>>,
}

impl Board {
    pub fn put_reply(&self, place: &str, p: Pending) {
        let mut m = self.replies.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() >= REPLIES_KEPT && !m.contains_key(place) {
            m.clear();
        }
        m.insert(place.into(), p);
    }

    pub fn take_reply(&self, place: &str) -> Option<Pending> {
        self.replies
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(place)
    }
}

#[cfg(test)]
mod tests;
