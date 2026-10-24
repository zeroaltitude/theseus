//! The live questions in the owner's DM (theseus-6f8t0, the owner's call O1:
//! yes). The nightly live pass writes questions (`CardProposal`s with
//! `observations`); this posts them: at most `[card] dm_per_day` a local day,
//! never before `[initiative] morning_hour`, and only when the pacer's window
//! is open (a closed window waits, it never posts silently: a question is not
//! a card the owner asked for). Each is one outbox post of kind
//! `card_question` (a post nobody asked for, so `pacer::UNPROMPTED`), and the
//! post and the proposal's `posted_ms` are one frame, so a crash posts and
//! marks both or neither and a question is never posted twice. A proposal the
//! owner answered first in the CLI is not open, so it is never posted.
//!
//! Off the turn path: a small tender, started beside the card tender, looks
//! every ten minutes; a tick that has nothing to post reads one META prefix
//! and writes nothing. The Discord binding draws the post and its buttons
//! (`theseus-discord`'s `runtime/card_question.rs`); the answer is
//! `card.answer` as the presser.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::json;
use theseus_protocol::{CardAsk, CardObservation, CardProposal, ProposalState};

use super::state;
use crate::config::card::CardConfig;
use crate::owner_note::{self, Kind};
use crate::rpc::Core;
use crate::wake::local;

/// How often the tender looks.
const EVERY: Duration = Duration::from_secs(600);

/// A local day, as the day count's key.
fn day_of(unix_ms: u64) -> (i32, u32, u32) {
    let l = local(unix_ms);
    (l.year, l.month, l.day)
}

/// The proposals to post now, oldest first. Pure: the conditions in the
/// order they are asked, so each test names the one that holds a post back.
/// The pacer and the DM binding are the caller's.
pub fn due<'a>(
    all: &'a [CardProposal],
    cfg: &CardConfig,
    morning_hour: u32,
    now_ms: u64,
) -> Vec<&'a CardProposal> {
    if cfg.dm_per_day == 0 || !cfg.observe || local(now_ms).hour < morning_hour {
        return Vec::new();
    }
    let today = day_of(now_ms);
    let posted = all
        .iter()
        .filter_map(|p| p.posted_ms)
        .filter(|ms| day_of(*ms) == today)
        .count();
    let left = (cfg.dm_per_day as usize).saturating_sub(posted);
    all.iter()
        .filter(|p| {
            p.state == ProposalState::Open
                && !p.observations.is_empty()
                && p.posted_ms.is_none()
                && p.expires_ms > now_ms
        })
        .take(left)
        .collect()
}

/// A post's title and the line under it, by what the question asks.
fn words(ask: CardAsk) -> (&'static str, &'static str) {
    match ask {
        CardAsk::Add => (
            "Add this to your card?",
            "Something you said in your recent conversations",
        ),
        CardAsk::Pattern => (
            "A pattern across your conversations",
            "The same thing came up in several of them",
        ),
        CardAsk::Stale => (
            "Is this still true?",
            "A recent conversation seems to contradict a record on your card",
        ),
    }
}

/// The message's text: the owner-note head, the question as `theseus card`
/// shows it, and the messages it rests on as `m:` handles.
pub fn text_of(p: &CardProposal, obs: &HashMap<String, CardObservation>) -> String {
    let (title, trigger) = words(p.ask);
    let mut nodes: Vec<&str> = p.sources.iter().map(|s| s.node.as_str()).collect();
    if nodes.is_empty() {
        nodes = p
            .observations
            .iter()
            .filter_map(|id| obs.get(id))
            .flat_map(|o| o.nodes.iter().map(String::as_str))
            .collect();
    }
    nodes.dedup();
    let handles = theseus_protocol::cite::handles_for(&nodes).join(" ");
    let mut body = p.question.clone();
    if !handles.is_empty() {
        body.push_str(&format!("\nFrom {handles}"));
    }
    owner_note::note(Kind::Decision, title, trigger, &body)
}

impl Core {
    /// Both card tenders, after serving: the nightly run's and the DM poster's.
    pub fn card_tenders_after_serving(self: &Arc<Self>) {
        self.card_after_serving();
        self.card_dm_after_serving();
    }

    /// The poster, after serving: nothing while the DM questions or live
    /// learning are off.
    pub fn card_dm_after_serving(self: &Arc<Self>) {
        if self.cfg.card.dm_per_day == 0 || !self.cfg.card.observe {
            return;
        }
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(EVERY).await;
                let Some(c) = core.upgrade() else { return };
                match c.card_dm_tick(theseus_protocol::now_unix_ms()).await {
                    Ok(0) => {}
                    Ok(n) => tracing::info!(posted = n, "card: live questions went to the DM"),
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "card: the DM questions were not posted")
                    }
                }
            }
        });
    }

    /// One look: post what is due, once. How many went. The conditions are
    /// `due`'s, then a DM with the owner bound and the pacer's window open;
    /// the proposals are read again just before the frame, which narrows (does
    /// not close) the window in which an answer that landed meanwhile is
    /// overwritten: the writer handshake counts writers, it does not exclude them.
    pub async fn card_dm_tick(&self, now_ms: u64) -> Result<usize> {
        let cfg = &self.cfg.card;
        let hour = self.cfg.initiative.morning_hour;
        if cfg.dm_per_day == 0
            || !cfg.observe
            || local(now_ms).hour < hour
            || !self.runner.place_rule.owner_dm_bound()
            || !self
                .outbox
                .pacer
                .open(now_ms, 1000 * self.cfg.notify.unprompted_pace_secs)
        {
            return Ok(0);
        }
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        if due(&all, cfg, hour, now_ms).is_empty() {
            return Ok(0);
        }
        let w = self.runner.pass.writing().await;
        let posted = theseus_store::blocking(|| self.card_dm_write(now_ms));
        drop(w);
        posted
    }

    /// The frame: the posts and the proposals marked, written once.
    fn card_dm_write(&self, now_ms: u64) -> Result<usize> {
        // The handshake can wait minutes: a needs-you or digest may have taken
        // the pacer's window meanwhile, and these would be a second buzz in it.
        if !self
            .outbox
            .pacer
            .open(now_ms, 1000 * self.cfg.notify.unprompted_pace_secs)
        {
            return Ok(0);
        }
        let all = state::proposals(&self.store)?;
        let hour = self.cfg.initiative.morning_hour;
        let mut frame = Vec::new();
        let mut staged = Vec::new();
        let obs: HashMap<String, CardObservation> = state::observations(&self.store, None)?
            .into_iter()
            .map(|o| (o.id.clone(), o))
            .collect();
        for p in due(&all, &self.cfg.card, hour, now_ms) {
            let body = json!({
                "kind": "card_question",
                "text": text_of(p, &obs),
                "card": {"id": p.id, "ask": p.ask},
            });
            let (post, more) = self.outbox.stage_to_operator(None, body)?;
            frame.extend(more);
            let mut marked = p.clone();
            marked.posted_ms = Some(now_ms);
            frame.push(state::record(&marked)?);
            staged.push(post);
        }
        if staged.is_empty() {
            return Ok(0);
        }
        self.store.append(&frame)?;
        for post in &staged {
            self.outbox.posted(post);
        }
        Ok(staged.len())
    }
}
