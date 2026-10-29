//! A question that waited past its life is noticed, never cancelled
//! (theseus-v2dnc; the owner, 2026-10-25 13:58: "we've moved away from
//! approve most everywhere, and towards notify. i want there to be precious
//! few hard gates /anywhere/ in theseus", and 14:16: "A simple admission
//! timeout that halts so much is certainly too blunt."). A clock is never,
//! by itself, a reason to stop work: a call's question that nobody answered
//! in `[kernel] confirm_ttl_secs` stays pending, and one quiet notice (a
//! post to the owner's DM that never pings, `quiet`) says what waits and for
//! how long; the next comes a life later, so at 15 minutes, 30, 45.
//!
//! What counts the notices is a META record per question
//! (`question.noticed.<correlation id>`, no store format change), written in
//! the notice's own frame: a restart does not tell a life twice, and a sweep
//! that finds several lives past (the daemon was down) tells the latest once.
//! Ended by what always ended a question: the owner's answer, a stop or
//! cancel of its execution, or its session's end. The record ends with it
//! (theseus-1kw2v): the sweep that reads the pending questions ends the count
//! of every question no longer among them, whichever way it ended.
//!
//! Not here, and as they were: a budget question holds until it is answered
//! (`expire_questions` never reads it); an `owner_ask` applies its default at
//! its own life (`expire_ask`); a proposed extension's ack and a pack's
//! promotion, which hold no running call, still expire declined.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use theseus_kernel::Action;
use theseus_store::{kinds, NewRecord, Store as _};

use super::Core;

/// The META key that counts a question's notices.
const NOTICED_PREFIX: &str = "question.noticed.";

/// The shortest life a notice repeats at: a configured life of zero must not
/// make the driver notice on every tick.
const MIN_LIFE_MS: u64 = 1_000;

#[derive(Serialize, Deserialize)]
struct Noticed {
    /// The latest life, counted from the question's plan, that was told.
    lives: u64,
}

/// Whether a question past its life is told about and held, rather than
/// expired: a call's. The `owner_ask` applies its default, and an extension's
/// ack and a pack's promotion are declined as before.
pub(crate) fn held_past_life(a: &Action) -> bool {
    a.tool != crate::ask::OWNER_ASK
        && a.tool != crate::extend::ACK
        && a.tool != super::packs::PROMOTE
}

/// A wait in words: `15 minutes`, `1 hour 30 minutes`, `2 hours`.
pub(crate) fn waited_words(ms: u64) -> String {
    let minutes = (ms / 60_000).max(1);
    let plural = |n: u64, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
    match (minutes / 60, minutes % 60) {
        (0, m) => plural(m, "minute"),
        (h, 0) => plural(h, "hour"),
        (h, m) => format!("{} {}", plural(h, "hour"), plural(m, "minute")),
    }
}

impl Core {
    /// End the notice count of every question that is not in `pending`
    /// (theseus-1kw2v): an answered, stopped, or cancelled question's, and a
    /// retired session's. A question's count is written only after it was
    /// pending, so one that is not pending now never needs it again.
    pub(crate) fn forget_noticed(&self, pending: &[Action]) {
        match self.end_noticed(pending) {
            Ok(0) => {}
            Ok(n) => tracing::debug!(
                ended = n,
                "question notice counts ended with their questions"
            ),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "question notice counts could not be ended")
            }
        }
    }

    fn end_noticed(&self, pending: &[Action]) -> Result<usize> {
        let live: std::collections::HashSet<&str> =
            pending.iter().map(|a| a.correlation_id.as_str()).collect();
        let ended: Vec<String> = self
            .store
            .inner()
            .latest_with_prefix(kinds::META, NOTICED_PREFIX)?
            .into_iter()
            .filter_map(|r| r.key)
            .filter(|k| !live.contains(&k[NOTICED_PREFIX.len()..]))
            .collect();
        self.store.delete_meta_keys(&ended)?;
        Ok(ended.len())
    }

    /// `question_waits`, or a minute from now when it could not be told.
    pub(crate) fn question_waits_or_soon(&self, a: &Action, ttl_ms: u64, now: u64) -> u64 {
        self.question_waits(a, ttl_ms, now).unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), correlation_id = %a.correlation_id, "a question that waited could not be told");
            now + 60_000
        })
    }

    /// `a`, a question past its life, told once for the life it is in. Returns
    /// when the next life ends: where the driver reads the questions again.
    pub(crate) fn question_waits(&self, a: &Action, ttl_ms: u64, now: u64) -> Result<u64> {
        let life = ttl_ms.max(MIN_LIFE_MS);
        let waited = now.saturating_sub(a.planned_at_ms);
        let lives = (waited / life).max(1);
        let next = a.planned_at_ms.saturating_add((lives + 1) * life);
        let key = format!("{NOTICED_PREFIX}{}", a.correlation_id);
        let told: Option<Noticed> = self.store.get_meta(&key)?;
        if told.is_some_and(|t| t.lives >= lives) {
            return Ok(next);
        }
        let mut records = vec![NewRecord::json(
            kinds::META,
            Some(&key),
            &Noticed { lives },
        )?];
        // A headless daemon has no DM to tell; the life still counts, so the
        // next sweep is a life away rather than this tick.
        let mut post = None;
        if self.runner.place_rule.owner_dm_bound() {
            let text = format!(
                "⏳ `{}` has waited {} for your answer. It is held, not cancelled: it runs or is \
                 declined when you answer (`theseus answer {}`).",
                a.tool,
                waited_words(waited),
                crate::task::short(&a.correlation_id),
            );
            let body = json!({
                "kind": "notice", "text": text, "quiet": true,
                "purpose": "question_waiting", "correlation_id": a.correlation_id,
            });
            let (staged, more) = self.outbox.stage_to_operator(Some(&a.session_id), body)?;
            records.extend(more);
            post = Some(staged);
        }
        self.store.append(&records)?;
        if let Some(p) = &post {
            self.outbox.posted(p);
        }
        tracing::info!(correlation_id = %a.correlation_id, tool = %a.tool, waited_ms = waited,
            "a question waits past its life; told, not cancelled");
        Ok(next)
    }
}
