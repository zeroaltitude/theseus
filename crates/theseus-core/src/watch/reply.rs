//! The `reply` check (theseus-rlnbf; the horizon report's §5.5): waiting for
//! someone to answer, with no polling. "Tell me when the harbour master
//! replies in #ops" is a watch whose condition is a message, not a value.
//!
//! - **Nothing is checked.** A reply watch is never due on the tick: its one
//!   timed look is its expiry (`Entry::seed`). The Discord binding calls
//!   `Core::watch_reply_seen` for each message it hears; with no reply watch
//!   open that is one atomic load, and with one open for another place one map
//!   lookup (`Board::reply_matches`). The index is the board itself, kept by
//!   its insert and remove, so a restart rebuilds it with the watches
//!   (`Core::watch_load`) and nothing else needs reading.
//! - **A match** is one frame, as any met watch is (`watch.met`, the message's
//!   id, its author and its first 200 characters), and a notice in a
//!   private place: the watch's own place when that is private, else the
//!   owner's Home (never the shared place the reply was written in).
//! - **Outside text.** What a stranger wrote is quoted, marked `external` in
//!   the row, and never becomes a turn or a task: a reply watch only
//!   notices (`spec::build` refuses `then: turn|task`).
//! - **Who sets one.** The owner, from a private place, as any watch
//!   (`judge_act(Act::Watch)`); the model's `watch` tool asks the owner first.

use std::sync::Arc;

use serde_json::json;
use theseus_kernel::{ProbeNews, ProbeVerdict};
use theseus_protocol::PlaceClass;

use super::spec::Check;
use super::step::{Entry, Step};
use crate::rpc::Core;

/// The most of a message a notice and its row quote.
pub const QUOTE_CHARS: usize = 200;

/// A pattern is written between slashes (`/ship.*late/`); anything else is
/// text that must appear, in any case.
fn pattern_of(words: &str) -> Option<&str> {
    words
        .strip_prefix('/')
        .and_then(|r| r.strip_suffix('/'))
        .filter(|r| !r.is_empty())
}

/// `words` as the watch is made: a pattern that does not compile is invalid
/// input.
pub fn check_words(words: &str) -> Result<(), String> {
    if words.trim().is_empty() {
        return Err("`words` is empty: give some text, or a /pattern/".into());
    }
    match pattern_of(words) {
        Some(re) => theseus_tools::rx::is_match(&format!("(?i){re}"), "").map(|_| ()),
        None => Ok(()),
    }
}

/// Whether `text` has `words`.
pub fn words_match(words: &str, text: &str) -> bool {
    match pattern_of(words) {
        Some(re) => theseus_tools::rx::is_match(&format!("(?i){re}"), text).unwrap_or(false),
        None => text.to_lowercase().contains(&words.trim().to_lowercase()),
    }
}

/// Whether the author is `from`: their user id (bare or `discord:<id>`), or
/// their name, in any case.
pub fn from_match(from: &str, author_id: &str, author: &str) -> bool {
    let f = from.trim().trim_start_matches('@');
    let f = f.strip_prefix("discord:").unwrap_or(f);
    f == author_id || f.eq_ignore_ascii_case(author.trim_start_matches('@')) || {
        let (f, a) = (f.to_lowercase(), author.to_lowercase());
        !f.is_empty() && f.chars().any(|c| !c.is_ascii_digit()) && a.contains(&f)
    }
}

/// Whether a message is the reply `check` waits for.
pub fn matches(check: &Check, author_id: &str, author: &str, text: &str) -> bool {
    let Check::Reply { from, words, .. } = check else {
        return false;
    };
    from.as_deref()
        .is_none_or(|f| from_match(f, author_id, author))
        && words.as_deref().is_none_or(|w| words_match(w, text))
}

/// `place` as the outbox keys it: a bound place's name (`#ops`), its target
/// (`discord:channel:<id>`), or a channel's id. A name no place bears is
/// refused, with how to say it.
pub fn resolve_place(rule: &crate::places::PlaceRule, place: &str) -> Result<String, String> {
    let p = place.trim();
    if let Some(b) = rule.find(p) {
        return Ok(b.target);
    }
    if p.starts_with("discord:channel:") || p.starts_with("discord:dm:") {
        return Ok(p.to_string());
    }
    if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) {
        return Ok(format!("discord:channel:{p}"));
    }
    Err(format!(
        "`{place}` is no place the Discord binding binds: say its name as `/places` shows it (`#ops`), or the channel's id"
    ))
}

/// `p` with a reply check's place resolved (any other check is as it was).
pub fn resolved(
    p: &theseus_protocol::WatchCreateParams,
    rule: &crate::places::PlaceRule,
) -> Result<theseus_protocol::WatchCreateParams, String> {
    let mut p = p.clone();
    if let theseus_protocol::WatchCheck::Reply { place, .. } = &mut p.check {
        *place = resolve_place(rule, place)?;
    }
    Ok(p)
}

/// A message the binding heard.
pub struct Heard<'a> {
    /// The place as the outbox keys it: `discord:channel:<id>`, or
    /// `discord:dm:<user id>` for a DM.
    pub place: &'a str,
    pub author_id: &'a str,
    pub author: &'a str,
    pub message_id: &'a str,
    pub text: &'a str,
    /// A link to the message, when the binding can make one.
    pub link: Option<String>,
}

impl Entry {
    /// A reply came: the watch is met, once. Quiet if it already was.
    pub fn replied(
        &mut self,
        now: u64,
        place_name: &str,
        author: &str,
        message_id: &str,
        quote: &str,
        link: Option<&str>,
    ) -> Step {
        if self.next_ms == u64::MAX {
            return Step::Quiet;
        }
        let Check::Reply { .. } = &self.spec.check else {
            return Step::Quiet;
        };
        self.checks += 1;
        self.checked_ms = now;
        self.next_ms = u64::MAX;
        if self.first.is_none() {
            self.first = Some(quote.to_string());
        }
        self.last = Some(quote.to_string());
        let mut text = format!("👁 {author} replied in {place_name}: \"{quote}\"");
        if let Some(l) = link {
            text.push_str(&format!("\n{l}"));
        }
        Step::News(ProbeNews {
            verdict: ProbeVerdict::Met { turn: false },
            state: self.state(),
            text,
            detail: json!({"message_id": message_id, "author": author, "external": true}),
            keep: None,
        })
    }
}

/// `text` as a quote: one line, at most `QUOTE_CHARS`.
pub(super) fn quote_of(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let q: String = one.chars().take(QUOTE_CHARS).collect();
    if q.chars().count() < one.chars().count() {
        format!("{q}…")
    } else {
        q
    }
}

impl Core {
    /// The binding heard a message. One atomic load when no reply watch is
    /// open, one map lookup when none is open for `place`; a match is written
    /// off the caller's thread. Reads only: it never delays the message.
    pub fn watch_reply_seen(self: &Arc<Self>, m: &Heard<'_>) {
        for wake_id in self
            .watches
            .reply_matches(m.place, m.author_id, m.author, m.text)
        {
            let weak = Arc::downgrade(self);
            let (author, id, link) = (
                m.author.to_string(),
                m.message_id.to_string(),
                m.link.clone(),
            );
            let quote = quote_of(m.text);
            drop(tokio::task::spawn_blocking(move || {
                if let Some(c) = weak.upgrade() {
                    c.watch_reply_met(&wake_id, &author, &id, &quote, link.as_deref());
                }
            }));
        }
    }

    /// Write a reply's news: the frame, and the notice.
    pub fn watch_reply_met(
        &self,
        wake_id: &str,
        author: &str,
        message_id: &str,
        quote: &str,
        link: Option<&str>,
    ) {
        let now = self.kernel.now_ms();
        let name = match self.watches.get(wake_id).map(|e| e.spec.check) {
            Some(Check::Reply { place, .. }) => self.place_name(&place),
            _ => return,
        };
        let Some((before, step)) = self.watches.with(wake_id, |e| {
            let before = e.clone();
            let step = e.replied(now, &name, author, message_id, quote, link);
            (before, step)
        }) else {
            return;
        };
        let Step::News(news) = step else { return };
        // The watch leaves the board before its row can be read: whoever sees
        // `watch.met` never finds the watch still waiting (theseus-1b2zm).
        self.watches.remove(wake_id);
        if let Err(e) = self.watch_write(&before, &news) {
            tracing::warn!(wake_id, error = %format!("{e:#}"), "a reply watch's news was not written; it still waits");
            self.watches.insert(before);
        }
    }

    /// A place as people read it: its bound name (`#ops`), else its target.
    pub(super) fn place_name(&self, place: &str) -> String {
        self.runner
            .place_rule
            .find(place)
            .map_or_else(|| place.to_string(), |b| b.name)
    }

    /// Where a reply's notice goes: the watch's own place when that is
    /// private, else the owner's Home. Never the shared place the reply was
    /// written in, and never a shared place at all.
    pub(super) fn reply_target(
        &self,
        session_target: Option<String>,
        wake_target: Option<String>,
    ) -> Option<String> {
        [session_target, wake_target]
            .into_iter()
            .flatten()
            .find(|t| self.runner.place_rule.class(&self.cfg, Some(t)) == PlaceClass::Private)
            .or_else(|| self.home().ok().and_then(|h| h.target))
    }
}
