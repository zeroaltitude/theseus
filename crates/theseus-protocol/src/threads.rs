//! Threads (theseus-nhew; the owner, 2026-10-09: separate concerns, each
//! addressable, none burdening another's context). A thread is a durable
//! record bound to a **virtual place** `thread:<slug>`, so it reuses what a
//! place has: a chain of sessions, supersession on `/new`, the states, the
//! outbox, and the place rule. A thread is private always.
//!
//! This file is the seam the rows built on the record read (`thread-context`,
//! `thread-recall`, `thread-route`): extend it only by adding.

use serde::{Deserialize, Serialize};

use crate::DiscordOrigin;

/// A slug's longest length, and the most topics and card lines a thread holds.
pub const SLUG_MAX: usize = 40;
pub const MAX_TOPICS: usize = 3;
pub const MAX_CARD: usize = 12;
/// A card line, a title or a goal is a line or two of words, not a document.
pub const MAX_LINE_CHARS: usize = 300;

/// The prefix of a thread's virtual place.
pub const PLACE_PREFIX: &str = "thread:";

/// A slug is `[a-z0-9][a-z0-9-]{0,39}`.
pub fn slug_ok(s: &str) -> bool {
    let mut chars = s.chars();
    let first = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    first
        && s.len() <= SLUG_MAX
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The virtual place of a thread: `thread:<slug>`.
pub fn place_of(slug: &str) -> String {
    format!("{PLACE_PREFIX}{slug}")
}

/// The slug of a virtual place, when `place` is one (`thread:<slug>`, or the
/// outbox's `discord:thread:<slug>` spelling of the same place).
pub fn slug_of_place(place: &str) -> Option<&str> {
    let p = place.strip_prefix("discord:").unwrap_or(place);
    p.strip_prefix(PLACE_PREFIX).filter(|s| slug_ok(s))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ThreadState {
    /// Takes messages and is offered to the router.
    #[default]
    Active,
    /// Takes no message and is offered to no one until resumed.
    Paused,
    /// Finished: its session retired, its card kept in memory.
    Done,
}

impl ThreadState {
    pub fn as_str(self) -> &'static str {
        match self {
            ThreadState::Active => "active",
            ThreadState::Paused => "paused",
            ThreadState::Done => "done",
        }
    }
}

/// What a thread touches: the router and the context read it. Every list is
/// a set of plain strings, in the order given, none repeated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkingSet {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<String>,
    /// PR, issue and Beads ids.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hosts: Vec<String>,
}

impl WorkingSet {
    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
            && self.repos.is_empty()
            && self.branches.is_empty()
            && self.refs.is_empty()
            && self.files.is_empty()
            && self.hosts.is_empty()
    }
}

/// A seal on a thread's boundary: who set it, when, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Seal {
    pub by: String,
    pub at_ms: u64,
    pub reason: String,
}

/// One thread, as stored (`thread:<slug>` in META) and as the wire shows it.
/// `session_id` and `last_active_ms` are read when it is shown: the stored
/// record keeps neither current (a turn writes no record of the thread).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadInfo {
    pub slug: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    /// Topics (ontology topic ids), at most `MAX_TOPICS`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<String>,
    #[serde(default, skip_serializing_if = "WorkingSet::is_empty")]
    pub working_set: WorkingSet,
    /// The thread's card: where it stands, one line each, at most `MAX_CARD`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub card: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    #[serde(default)]
    pub state: ThreadState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub sealed: Option<Seal>,
    /// A seal proposed and not yet set: the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub seal_pending: Option<String>,
    /// Places the thread is shown on, as `discord:channel:<id>` or `cli`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub merged_into: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub split_from: Option<String>,
    /// Threads the owner linked to this one: their notes reach this thread's recall with no
    /// distance; a seal still holds them out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// The place's current session; none until the thread's first message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    #[serde(default)]
    pub last_active_ms: u64,
    #[serde(default)]
    pub created_at_ms: u64,
}

impl ThreadInfo {
    /// The thread's virtual place.
    pub fn place(&self) -> String {
        place_of(&self.slug)
    }
}

/// A thread as `thread.list` and `thread.show` give it: the record, and what
/// its current session holds and spent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadRow {
    pub thread: ThreadInfo,
    /// The session's last compile, in tokens; none before its first turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub context_tokens: Option<u64>,
    /// What the thread's sessions spent since local midnight.
    #[serde(default)]
    pub today_usd: f64,
    /// The prompt cache's lifetime for the profile the session's last turn ran
    /// on, in seconds (theseus-j0yh): a client reads warm or cold from this and
    /// the thread's `last_active_ms`. None before the session's first turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cache_ttl_secs: Option<u64>,
    /// The day's cold resumes, warm hits and keep-warm reads of the session
    /// (theseus-ytt8); none while it has none of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cache: Option<crate::thread_cache::ThreadCacheStats>,
    /// The threads merged into this one, and those split from it
    /// (theseus-vt9k): the other ends of `merged_into` and `split_from`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged_from: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub split_to: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadNewParams {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topics: Option<Vec<String>>,
    /// Made sealed (theseus-f2g0): its notes never reach another thread.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sealed: bool,
    /// The thread this one splits from: a split from a sealed thread
    /// starts sealed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub split_from: Option<String>,
    /// The place the thread is born in (a bound place's target, such as
    /// `discord:channel:<id>`): the thread's persona is that place's, when
    /// the place names one the config has (theseus-a8n7). `thread.edit
    /// { persona }` changes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadListParams {
    /// Only threads in this state; none: active and paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub state: Option<ThreadState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadListResult {
    pub threads: Vec<ThreadRow>,
    /// How many the daemon holds in all, whatever their state.
    pub total: u32,
}

/// `thread.show`, `thread.pause`, `thread.resume`, `thread.done` and
/// `thread.reopen` name a thread and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadNameParams {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `thread.link` and `thread.unlink`: the owner's word that two threads belong together.
/// The result is `slug`'s thread, as `thread.edit` gives it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadLinkParams {
    pub slug: String,
    pub other: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// `thread.edit`: each field given replaces what the thread held.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadEditParams {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub topics: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub working_set: Option<WorkingSet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub card: Option<Vec<String>>,
    /// A persona's name; an empty string clears it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    /// The places the thread is shown on (`discord:channel:<id>`, `cli`),
    /// whole: `/thread bind` and `unbind` edit it (theseus-sjzm).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub surfaces: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// Who is speaking and where the answer goes, for a turn that runs in a
/// thread (theseus-sjzm): the Discord binding names the person and place the
/// core judges (a thread is the owner's, from a private place), and the
/// place the reply is posted to when it is not the thread's own surface (a
/// message in the owner's DM answered there, `discord:dm:<user>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Speaking {
    pub discord: DiscordOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub answer_at: Option<String>,
}

/// `thread.changed`: a thread was made, edited, paused, resumed, finished or
/// reopened, or opened its session (its first message). To every connection,
/// so a surface that shows threads (the Discord binding's one Discord thread
/// each) follows them without polling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadChanged {
    pub slug: String,
    pub title: String,
    pub state: ThreadState,
    pub sealed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surfaces: Vec<String>,
    /// `created`, `edited`, `paused`, `resumed`, `done`, `reopened`, `linked`,
    /// `unlinked` or `opened` (the thread's session was opened by its first message).
    pub change: String,
    pub who: String,
}

impl ThreadChanged {
    /// The notification for `t` after `change`.
    pub fn of(t: &ThreadInfo, change: &str, who: &str) -> Self {
        Self {
            slug: t.slug.clone(),
            title: t.title.clone(),
            state: t.state,
            sealed: t.sealed.is_some(),
            persona: t.persona.clone(),
            surfaces: t.surfaces.clone(),
            change: change.into(),
            who: who.into(),
        }
    }
}

/// The surface string of a Discord channel or Discord thread.
pub fn channel_surface(channel: u64) -> String {
    format!("discord:channel:{channel}")
}

/// The Discord channel a surface string names.
pub fn surface_channel(surface: &str) -> Option<u64> {
    surface.strip_prefix("discord:channel:")?.parse().ok()
}

/// `thread.seal` (theseus-f2g0; the owner seals, Jev only recommends): the
/// thread's notes stop reaching any other thread's recall; `redirect` also
/// leaves a note in each session that already used them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadSealParams {
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub redirect: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// A session that used a sealed thread's notes before it was sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadCrossed {
    pub session_id: String,
    /// The thread that session belongs to; none for an unthreaded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub thread: Option<String>,
    /// That thread's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// How many of the sealed thread's notes it was given.
    pub facts: u32,
    /// Whether a redirect note now stands in that session.
    #[serde(default)]
    pub redirected: bool,
}

/// `thread.seal`'s answer: the thread as sealed, and what had crossed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadSealResult {
    pub thread: ThreadRow,
    pub crossed: Vec<ThreadCrossed>,
}

/// The word a seal's report says for `crossed`: "2 facts were already used
/// in «pr round»", one clause for each session.
pub fn crossed_words(c: &[ThreadCrossed]) -> Vec<String> {
    c.iter()
        .map(|x| {
            let facts = if x.facts == 1 {
                "fact was"
            } else {
                "facts were"
            };
            let n = x.facts;
            match (&x.title, &x.thread) {
                (Some(t), _) => format!("{n} {facts} already used in «{t}»"),
                (None, Some(s)) => format!("{n} {facts} already used in «{s}»"),
                (None, None) => {
                    format!("{n} {facts} already used in the session {}", x.session_id)
                }
            }
        })
        .collect()
}

/// `thread.merge`: thread `from` is folded into thread `into` (theseus-vt9k).
/// Nothing is deleted: `from` is retired with `merged_into = into`, its card
/// quoted at the end of `into`'s, its working set and topics joined. The
/// owner's act, from a private place.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadMergeParams {
    pub from: String,
    pub into: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What `thread.merge` and `thread.unmerge` did: both threads as they stand,
/// and the topics the three-topic limit left out of the merged thread (the
/// owner picks among them with `thread.edit`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadMergeResult {
    pub from: ThreadRow,
    pub into: ThreadRow,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics_left_out: Vec<String>,
}

/// `thread.split`: turns of `from` move into a thread (theseus-vt9k, theseus-q9b85).
/// `from` is a thread's slug, a place's own session by its whole id
/// (`ses_…`, `theseus places` lists them), or `home@<place>`. The turns are
/// chosen by WAL position (the `@` numbers of `theseus history`): the
/// inclusive `range` `[first, last]`, several `ranges`, or the `positions` of
/// single nodes; whole turns only. They are quoted into a new thread `slug`,
/// or at the tail of the existing thread `into`; the source keeps every
/// node, but its compile holds one line, `moved to thread «X»: <gist>`, in
/// their place. `thread.unsplit` undoes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadSplitParams {
    pub from: String,
    /// The new thread's name; empty with `into`.
    #[serde(default)]
    pub slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub goal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub range: Option<[u64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub positions: Option<Vec<u64>>,
    /// Several inclusive ranges, each `[first, last]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<[u64; 2]>,
    /// An existing thread that takes the turns at its tail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub into: Option<String>,
    /// What the moved range is about, as its place in the source's compile says it
    /// (default: the range's first human message, cut to 80 characters).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub gist: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

/// What `thread.split` made: the thread, how many messages it quotes (and
/// the span of their positions), and whether the source's compile now holds
/// the moved line (not while a turn runs there).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadSplitResult {
    pub thread: ThreadRow,
    pub from: String,
    pub quoted: u32,
    pub first_pos: u64,
    pub last_pos: u64,
    pub noted: bool,
    /// The split's id: `thread.unsplit` takes it.
    #[serde(default)]
    pub split: String,
    /// The nodes the source's compile left out.
    #[serde(default)]
    pub moved: u32,
    /// The turns went into a thread that already existed.
    #[serde(default)]
    pub into: bool,
}

/// A request's size in tokens as `context.census` estimates it, before and
/// after a split, and the bytes and nodes that moved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SplitEstimate {
    pub before: u64,
    pub after: u64,
}

/// `thread.split_plan`: what a split would do, written nowhere: the turns,
/// nodes and bytes that would move, and the next request's tokens of the
/// source (compiled with the range left out) and of the target, before and
/// after. No model is called.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadSplitPlan {
    pub from: String,
    /// The target's slug, and whether it exists already.
    pub thread: String,
    pub into: bool,
    pub turns: u32,
    pub nodes: u32,
    pub bytes: u64,
    pub quoted: u32,
    /// What the quoted messages come to in tokens, as the thread takes them.
    pub quoted_tokens: u64,
    pub first_pos: u64,
    pub last_pos: u64,
    pub gist: String,
    pub source: SplitEstimate,
    /// The target's estimate; none for a thread that does not exist yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<SplitEstimate>,
}

/// `thread.unsplit`: a split undone, by its id (`thread.split` printed it).
/// Nothing is deleted: the source's compile reads the range again in its
/// own place, the quoted turns leave the target's, a new thread is retired.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadUnsplitParams {
    pub split: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ThreadUnsplitResult {
    pub split: String,
    pub from: String,
    pub thread: ThreadRow,
    /// The nodes the source's compile reads again.
    pub restored: u32,
    /// The target was a thread the split opened, and is done now.
    pub retired: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seals_report_names_where_the_facts_crossed() {
        let c = |facts, title: Option<&str>| ThreadCrossed {
            session_id: "ses_x".into(),
            thread: title.map(|_| "pr-round".into()),
            title: title.map(str::to_string),
            facts,
            redirected: false,
        };
        assert_eq!(
            crossed_words(&[c(2, Some("pr round")), c(1, None)]),
            [
                "2 facts were already used in «pr round»",
                "1 fact was already used in the session ses_x"
            ]
        );
    }

    #[test]
    fn a_slug_is_lower_case_words_joined_by_hyphens() {
        for ok in ["a", "0", "tide-gauge", "a1-b2", &"x".repeat(40)] {
            assert!(slug_ok(ok), "{ok}");
        }
        for bad in ["", "-a", "A", "a b", "a_b", "a:b", "é", &"x".repeat(41)] {
            assert!(!slug_ok(bad), "{bad}");
        }
    }

    #[test]
    fn a_virtual_place_names_its_thread_either_way_it_is_spelled() {
        assert_eq!(place_of("tides"), "thread:tides");
        assert_eq!(slug_of_place("thread:tides"), Some("tides"));
        assert_eq!(slug_of_place("discord:thread:tides"), Some("tides"));
        assert_eq!(slug_of_place("discord:channel:42"), None);
        assert_eq!(slug_of_place("thread:Bad Slug"), None);
    }
}
