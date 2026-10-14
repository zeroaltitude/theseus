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

    /// Escalate at a given time (a test's).
    pub fn escalate_questions(&self, now: u64) -> usize {
        let Ok(pending) = self.kernel.pending_confirms() else {
            return 0;
        };
        if pending.is_empty() || !self.runner.place_rule.owner_dm_bound() {
            return 0;
        }
        let wait = self.cfg.notify.wait_ms(self.bus.interactive_watchers() > 0);
        let ttl = self.kernel.config().confirm_ttl_ms;
        let mut posted = 0;
        for a in pending {
            // A question that expires now says so itself.
            let expires = match a.tool == theseus_kernel::BUDGET_TOOL {
                true => u64::MAX,
                false => crate::ask::expiry_of(&a, ttl),
            };
            if now < a.planned_at_ms.saturating_add(wait)
                || now >= expires
                || self.outbox.has_card(&a.correlation_id)
                || self.outbox.pinged(&a.correlation_id)
            {
                continue;
            }
            match self.post_needs_you(&a) {
                Ok(true) => posted += 1,
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), question = %a.correlation_id, "a question's ping was not written");
                }
            }
        }
        posted
    }

    /// The post, in a frame of its own (the tick's). False when the question
    /// can no longer be read whole (it closed meanwhile).
    fn post_needs_you(&self, a: &Action) -> anyhow::Result<bool> {
        let Some(req) = self
            .pending_confirms(&a.session_id)?
            .into_iter()
            .find(|r| r.correlation_id == a.correlation_id)
        else {
            return Ok(false);
        };
        let who = self.asker_name(&req);
        let body = body_of(a, &req, &who);
        let (post, frame) = self.outbox.stage_to_operator(None, body)?;
        self.store.append(&frame)?;
        self.outbox.posted(&post);
        Ok(true)
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
        None => clip(&req.reason, 300),
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

fn clip(s: &str, max: usize) -> String {
    let s = s.trim();
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}
