//! The always-on channel (theseus-j7cje; §1.8.4): an open question reaches the
//! owner's phone. A question shows at once, silently, on every Theseus screen
//! and on the DM's board; if it is still open after `[notify]
//! escalate_after_secs` with no screen open (`Bus::interactive_watchers`), or
//! after `escalate_with_screen_secs` with one, the core posts one
//! **needs-you** to the owner's Home DM (outbox kind `needs_you`), which pings.
//!
//! - **On the driver's tick, from the store's truth.** The questions are read
//!   as the expiry reads them (`Kernel::pending_confirms`, by the planned
//!   state), not from the push's board: the board is seeded by the first
//!   client that watches, and the case that matters most is the one with no
//!   client at all. A question is read whole (a transcript) only once it is
//!   due and has no post.
//! - **One post per question**, however long it waits, for any kind: an
//!   approval, a budget's, a layer-1 change, an `owner_ask`. The outbox index
//!   holds it (`Outbox::pinged`), rebuilt from the posts at a start, so a
//!   restart posts none again.
//! - **A question asked in a Discord place posts its card there at once**,
//!   pinging as it always has: it has a card (`Outbox::has_card`), and no
//!   second post is made for it.
//! - **Settled, no ping**: the post is indexed as the question's card, so
//!   whoever closes the question (`Core::card_closed`, or the heartbeat's
//!   `reconcile_cards` for a close no event said) writes the settle that
//!   edits it to how the question closed.
//! - **Nothing without a DM**: with no DM bound for the owner there is no
//!   phone to ping, and nothing is written.

use serde_json::json;
use theseus_kernel::Action;
use theseus_protocol::term::words::cut_marked;
use theseus_protocol::work::QuestionKind;
use theseus_protocol::ConfirmRequest;

use crate::rpc::Core;

/// The outbox post's kind.
pub const KIND: &str = "needs_you";

impl Core {
    /// The driver's tick for the questions that wait: escalate the ones that
    /// have waited long enough to the owner's DM, and remind those at half
    /// their life (the policy's rule 2). No timer, no read of history.
    pub fn tick_questions(&self) {
        let now = self.kernel.now_ms();
        self.escalate_questions(now);
        self.push.remind_questions(self, now);
    }

    /// Escalate at a given time (a test's). Returns how many questions it
    /// pinged for.
    pub fn escalate_questions(&self, now: u64) -> usize {
        let Ok(pending) = self.kernel.pending_confirms() else {
            return 0;
        };
        if pending.is_empty() || !self.runner.place_rule.owner_dm_bound() {
            return 0;
        }
        let wait = self.cfg.notify.wait_ms(self.bus.interactive_watchers() > 0);
        let ttl = self.kernel.config().confirm_ttl_ms;
        let due: Vec<Action> = pending
            .into_iter()
            .filter(|a| {
                // A question that expires now says so itself.
                let expires = match a.tool == theseus_kernel::BUDGET_TOOL {
                    true => u64::MAX,
                    false => crate::ask::expiry_of(a, ttl),
                };
                now >= a.planned_at_ms.saturating_add(wait)
                    && now < expires
                    && !self.outbox.has_card(&a.correlation_id)
                    && !self.outbox.pinged(&a.correlation_id)
            })
            .collect();
        // The one pacing rule (`pacer.rs`): a post nobody asked for goes out
        // at most once a window, and what is due meanwhile waits for it and
        // goes in the same post.
        let pace = 1000 * self.cfg.notify.unprompted_pace_secs;
        if due.is_empty() || !self.outbox.pacer.open(now, pace) {
            return 0;
        }
        let asks: Vec<(Action, ConfirmRequest)> = due
            .into_iter()
            .filter_map(|a| match self.whole_question(&a) {
                Ok(Some(req)) => Some((a, req)),
                Ok(None) => None,
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), question = %a.correlation_id, "a question could not be read for its ping");
                    None
                }
            })
            .collect();
        if asks.is_empty() {
            return 0;
        }
        match self.post_needs_you(&asks) {
            Ok(()) => asks.len(),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "a ping for the questions that wait was not written");
                0
            }
        }
    }

    /// A question as the owner sees it; None when it closed meanwhile.
    fn whole_question(&self, a: &Action) -> anyhow::Result<Option<ConfirmRequest>> {
        Ok(self
            .pending_confirms(&a.session_id)?
            .into_iter()
            .find(|r| r.correlation_id == a.correlation_id))
    }

    /// The post, in a frame of its own (the tick's): one question's own, with
    /// its options, or the several that were due together.
    fn post_needs_you(&self, asks: &[(Action, ConfirmRequest)]) -> anyhow::Result<()> {
        let named: Vec<(&Action, &ConfirmRequest, String)> = asks
            .iter()
            .map(|(a, req)| (a, req, self.asker_name(req)))
            .collect();
        let body = match named.as_slice() {
            [(a, req, who)] => body_of(a, req, who),
            many => batch_body_of(many),
        };
        let (post, frame) = self.outbox.stage_to_operator(None, body)?;
        self.store.append(&frame)?;
        self.outbox.posted(&post);
        Ok(())
    }

    /// Who asks: the task's title, else its short id, else the conversation's
    /// title.
    fn asker_name(&self, req: &ConfirmRequest) -> String {
        if let Some(t) = &req.task {
            return t
                .title
                .clone()
                .unwrap_or_else(|| format!("task {}", t.short));
        }
        self.store
            .get_session::<crate::session::SessionRecord>(&req.session_id)
            .ok()
            .flatten()
            .and_then(|r| r.title)
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| "Theseus".to_string())
    }
}

/// What the post says: structured for the binding to dress, and a plain
/// `text` for anything that shows it as it is.
pub fn body_of(a: &Action, req: &ConfirmRequest, who: &str) -> serde_json::Value {
    let ask = crate::ask::input_of(a);
    let prompt = match &ask {
        Some(i) => i.question.clone(),
        None => cut_marked(req.reason.trim(), 300),
    };
    let choices = ask.as_ref().map(|i| i.choices()).unwrap_or_default();
    let kind = crate::rpc::answer::kind_of(req);
    let short: String = a
        .correlation_id
        .chars()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let how = match kind {
        QuestionKind::Ask => format!("answer with `theseus answer {short} <number>`"),
        _ => format!("answer with `theseus answer {short} approve` or `decline`, or open Theseus"),
    };
    let mut text = format!("● {who} needs you: {prompt}");
    for (i, c) in choices.iter().enumerate() {
        text.push_str(&format!("\n{}. {c}", i + 1));
    }
    text.push_str(&format!("\n-# {how}"));
    json!({
        "kind": KIND,
        "question": a.correlation_id,
        "text": text,
        "asked_by": who,
        "prompt": prompt,
        "choices": choices,
        "expires_at_ms": (req.expires_at_ms > 0).then_some(req.expires_at_ms),
    })
}

/// One post for several questions that were due together (the pacer): who
/// asks what, and how to answer each. It names every question
/// (`questions`) so none pings again, and has no buttons and no settle: it is
/// a nudge to open Theseus, not a card.
pub fn batch_body_of(asks: &[(&Action, &ConfirmRequest, String)]) -> serde_json::Value {
    const LISTED: usize = 10;
    let mut text = format!("🔔 **{} questions need you**", asks.len());
    for (a, req, who) in asks.iter().take(LISTED) {
        let prompt = match crate::ask::input_of(a) {
            Some(i) => i.question,
            None => cut_marked(req.reason.trim(), 120),
        };
        let short: String = {
            let mut tail: Vec<char> = a.correlation_id.chars().rev().take(8).collect();
            tail.reverse();
            tail.into_iter().collect()
        };
        text.push_str(&format!(
            "\n- **{who}**: {} (`{short}`)",
            cut_marked(prompt.trim(), 120)
        ));
    }
    if asks.len() > LISTED {
        text.push_str(&format!("\n- and {} more", asks.len() - LISTED));
    }
    text.push_str("\n-# answer with `theseus answer <id> <number or approve>`, or open Theseus");
    json!({
        "kind": KIND,
        "text": text,
        "questions": asks.iter().map(|(a, _, _)| a.correlation_id.clone()).collect::<Vec<_>>(),
    })
}
