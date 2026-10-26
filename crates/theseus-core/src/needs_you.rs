//! The always-on channel (theseus-j7cje; §1.8.4): an open question reaches the
//! owner's phone. A question shows at once, silently, on every Theseus screen
//! and on the DM's board; if it is still open after `[notify]
//! escalate_after_secs` with no screen open (`Bus::interactive_watchers`), or
//! after `escalate_with_screen_secs` with one, the core posts one
//! **needs-you** to the owner's Home DM (outbox kind `needs_you`), which pings
//! while the pacer's window is open and is silent while it is closed: the
//! window paces the buzz, never the card, so a question always has its card
//! before it expires (theseus-27p6w).
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
//! - **A question the owner prompted** (`Ask again` on an expired card,
//!   theseus-27p6w) is due at once and pings whatever the window says: the
//!   owner just asked for it.
//! - **Nothing without a DM**: with no DM bound for the owner there is no
//!   phone to ping, and nothing is written.

use serde_json::json;
use theseus_kernel::Action;
use theseus_protocol::term::words::cut_marked;
use theseus_protocol::work::QuestionKind;
use theseus_protocol::ConfirmRequest;

use crate::rpc::Core;

mod plain;
mod words;

pub use plain::{plain, rule_clause, Gloss};

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
                // A question that expires now says so itself. A call's is
                // held past its life, never expired (theseus-v2dnc), so it
                // is carded however long it has waited: the daemon may have
                // been down, or the DM unbound, through its first life.
                let expires = match a.tool == theseus_kernel::BUDGET_TOOL
                    || crate::rpc::held_past_life(a)
                {
                    true => u64::MAX,
                    false => crate::ask::expiry_of(a, ttl),
                };
                (now >= a.planned_at_ms.saturating_add(wait)
                    || self.outbox.prompted.has(&a.session_id))
                    && now < expires
                    && !self.outbox.has_card(&a.correlation_id)
                    && !self.outbox.pinged(&a.correlation_id)
            })
            .collect();
        if due.is_empty() {
            return 0;
        }
        // The one pacing rule (`pacer.rs`) paces the buzz, never the card: a
        // due question posts on this tick, and the window decides only
        // whether the post pings (and takes the window) or is silent.
        let pace = 1000 * self.cfg.notify.unprompted_pace_secs;
        let prompted = due.iter().any(|a| self.outbox.prompted.has(&a.session_id));
        let loud = prompted || self.outbox.pacer.open(now, pace);
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
        match self.post_needs_you(&asks, loud) {
            Ok(()) => {
                for (a, _) in &asks {
                    self.outbox.prompted.take(&a.session_id);
                }
                asks.len()
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "a ping for the questions that wait was not written");
                0
            }
        }
    }

    /// What a question wants, as its card says it (`words::words`): the line
    /// "Ask again" tells the session. None when its call is not in the record.
    pub(crate) fn what_of(&self, a: &Action) -> Option<String> {
        let rec = self
            .store
            .get_session::<crate::session::SessionRecord>(&a.session_id)
            .ok()??;
        let nodes = self.store.session_nodes(&a.session_id).ok()?;
        let req = self.confirm_request(a, &rec, &nodes)?;
        let asked = crate::ask::input_of(a).map(|i| i.question);
        Some(words::words(asked, &req, &self.asker_name(&req)).1.what)
    }

    /// An approval's words, once, for every surface (theseus-cudar): who
    /// asks, what, the model's reason, the risk, and what Yes and No do. None
    /// for the other kinds, which have words of their own (`words::words`).
    pub(crate) fn plain_of(
        &self,
        req: &ConfirmRequest,
        nodes: &[(u64, crate::node::Node)],
    ) -> Option<theseus_protocol::PlainWords> {
        use crate::node::Body;
        if crate::rpc::answer::kind_of(req) != QuestionKind::Approval || req.tool.is_empty() {
            return None;
        }
        let model_why = nodes.iter().find_map(|(_, n)| match &n.body {
            Body::ToolCall {
                correlation_id: Some(c),
                assistant_node,
                ..
            } if *c == req.correlation_id => Some(assistant_node.clone()),
            _ => None,
        });
        let model_why = model_why.and_then(|id| {
            nodes.iter().find_map(|(_, n)| match &n.body {
                Body::AssistantMessage { blocks, .. } if n.id == id => {
                    plain::reason_line(&crate::provider::text_of(blocks))
                }
                _ => None,
            })
        });
        let state = self.cfg.state_dir();
        let own_task = crate::rsi::own_tree::item_slug(&self.store, &req.session_id);
        Some(plain(
            req,
            &Gloss {
                state,
                who: self.asker_name(req),
                own_task,
                model_why,
            },
        ))
    }

    /// A question as the owner sees it; None when it closed meanwhile.
    fn whole_question(&self, a: &Action) -> anyhow::Result<Option<ConfirmRequest>> {
        Ok(self
            .pending_confirms(&a.session_id)?
            .into_iter()
            .find(|r| r.correlation_id == a.correlation_id))
    }

    /// The posts, each in a frame of its own (the tick's). With `[notify]
    /// dm_cards` on, every question is a card of its own (theseus-suvpe), and
    /// several that were due together are one header, which pings, and a card
    /// each, which post silently; off, one question's own post or the one
    /// post for the several, as before. With `loud` false (the pacer's window
    /// is closed) every post is silent, so none takes the window.
    fn post_needs_you(&self, asks: &[(Action, ConfirmRequest)], loud: bool) -> anyhow::Result<()> {
        let named: Vec<(&Action, &ConfirmRequest, String)> = asks
            .iter()
            .map(|(a, req)| (a, req, self.asker_name(req)))
            .collect();
        let bodies = match (self.cfg.notify.dm_cards, named.as_slice()) {
            (true, [(a, req, who)]) => vec![card_body(a, req, who, None)],
            (true, many) => {
                let mut v = vec![header_body(many)];
                v.extend(many.iter().enumerate().map(|(i, (a, req, who))| {
                    let mut b = card_body(a, req, who, Some((i + 1, many.len())));
                    b["silent"] = json!(true);
                    b
                }));
                v
            }
            (false, [(a, req, who)]) => vec![body_of(a, req, who)],
            (false, many) => vec![batch_body_of(many)],
        };
        for mut body in bodies {
            if !loud {
                body["silent"] = json!(true);
            }
            let (post, frame) = self.outbox.stage_to_operator(None, body)?;
            self.store.append(&frame)?;
            self.outbox.posted(&post);
        }
        Ok(())
    }

    /// Who asks: a self task as "Theseus's self-repair: <the backlog item's
    /// title>", any other task by its own record's title, else its short id,
    /// else the thread's title for a thread's session, else the conversation's
    /// title (theseus-suvpe).
    pub(crate) fn asker_name(&self, req: &ConfirmRequest) -> String {
        if req.task.is_some() {
            let own = crate::task_graph::all(&self.store).ok().and_then(|all| {
                all.into_iter()
                    .find(|t| t.session.as_deref() == Some(req.session_id.as_str()))
            });
            if let Some(t) = own.filter(|t| !t.title.trim().is_empty()) {
                return match t.origin.principal == crate::rsi::SELF_PRINCIPAL {
                    true => format!(
                        "Theseus's self-repair: {}",
                        crate::rsi::backlog::plain_title(&t.title)
                    ),
                    false => t.title,
                };
            }
        }
        if let Some(t) = &req.task {
            return t
                .title
                .clone()
                .unwrap_or_else(|| format!("task {}", t.short));
        }
        // A thread's session is asked by the thread's title, not by the
        // first message it happened to get.
        let thread = self.runner.threads.held().and_then(|b| {
            b.of_session(&req.session_id)
                .map(|t| t.title.clone())
                .filter(|t| !t.trim().is_empty())
        });
        if let Some(t) = thread {
            return t;
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

/// The last eight characters of a question's id, which `theseus answer` takes.
fn short_id(corr: &str) -> String {
    let mut tail: Vec<char> = corr.chars().rev().take(8).collect();
    tail.reverse();
    tail.into_iter().collect()
}

/// One question as its own card's post (theseus-suvpe): the words, from the
/// request's structured fields, under `card`; `text` is the same in plain
/// words for anything that shows the post as it is, with how to answer it.
pub fn card_body(
    a: &Action,
    req: &ConfirmRequest,
    who: &str,
    nth: Option<(usize, usize)>,
) -> serde_json::Value {
    let (kind, w) = words::words(crate::ask::input_of(a).map(|i| i.question), req, who);
    let choices = crate::ask::input_of(a).map_or_else(Vec::new, |i| i.choices());
    let head = match nth {
        Some((n, of)) => format!("{n} of {of}"),
        None => "needs you".to_string(),
    };
    let mut text = match w.who.is_empty() {
        true => format!("● {head}: {}\n{}\n{}", w.what, w.why, w.yes),
        false => {
            let why = match w.why.is_empty() {
                true => String::new(),
                false => format!(" · why: {}", w.why),
            };
            format!(
                "● {head}: {} wants to {}\nrisk: {}{why}\n{} · {}",
                w.who, w.what, w.risk, w.yes, w.no
            )
        }
    };
    for (i, c) in choices.iter().enumerate() {
        text.push_str(&format!("\n{}. {c}", i + 1));
    }
    let short = short_id(&a.correlation_id);
    text.push_str(&match kind {
        QuestionKind::Ask => format!("\n-# answer with `theseus answer {short} <number>`"),
        _ => format!("\n-# answer with `theseus answer {short} approve` or `decline`"),
    });
    json!({
        "kind": KIND,
        "question": a.correlation_id,
        "text": text,
        "asked_by": who,
        "prompt": w.what,
        "choices": choices,
        "expires_at_ms": (req.expires_at_ms > 0).then_some(req.expires_at_ms),
        "card": words::card_of(kind, &w, req.external_text.is_some(), nth),
    })
}

/// The header of several cards due together: it pings, and names every
/// question (`questions`, in the cards' order) so none pings again.
pub fn header_body(asks: &[(&Action, &ConfirmRequest, String)]) -> serde_json::Value {
    json!({
        "kind": KIND,
        "text": format!("🔔 **{} questions need you**", asks.len()),
        "questions": asks.iter().map(|(a, _, _)| a.correlation_id.clone()).collect::<Vec<_>>(),
        "choices": asks
            .iter()
            .map(|(a, _, _)| crate::ask::input_of(a).map_or_else(Vec::new, |i| i.choices()))
            .collect::<Vec<_>>(),
        "trust": asks.iter().map(|(_, r, _)| r.external_text.is_some()).collect::<Vec<_>>(),
        "header": true,
    })
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
