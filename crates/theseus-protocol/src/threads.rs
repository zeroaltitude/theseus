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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
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
