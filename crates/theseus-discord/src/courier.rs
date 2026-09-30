//! Durable delivery, the binding's side (theseus-q4v): the lanes.
//!
//! One lane per place (a text channel or a DM), and one for the operator's
//! notices, each the only writer of its messages. A lane:
//! - sends its target's outbox posts in order, each dispatched before its
//!   first call and settled with the message ids Discord gave;
//! - then serves the live progress its place's renderer sends: the streamed
//!   text and tool messages of a running turn, and typing, only the latest
//!   state of each message, never replayed. While Discord is away, live
//!   progress is dropped and the posts wait.
//!
//! A lane needs only REST, so posts go out while the gateway is down. A create
//! carries Discord's nonce, with `enforce_nonce`, derived from its message's
//! key: a retry after a crash between the send and the settle returns the
//! first message instead of posting again.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use theseus_core::outbox::{body_of, kind_of, Closed};
use theseus_kernel::{Action, ActionState, Outcome};
use theseus_protocol::TurnSubmitResult;
use tokio::sync::mpsc;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;
use twilight_model::channel::message::{AllowedMentions, Message};
use twilight_model::id::Id;

use crate::render::{self, Buttons, NoticeCard, Op, Route};
use crate::runtime::{asked_button, asked_menu, confirm_buttons, Shared};

/// How long Discord is taken to honor a nonce. Past it, a create sent again
/// after a send that may have landed says it may be a copy (`render::RESENT`);
/// within it, Discord returns the first message and nothing new is posted.
pub const NONCE_WINDOW_MS: u64 = 120_000;
/// The longest a lane waits before it tries Discord again.
const BACKOFF_MAX: Duration = Duration::from_secs(60);

/// What a lane is told.
pub(crate) enum LaneMsg {
    /// Live progress: the latest state of one message, or typing.
    Live(Op),
    /// The outbox changed: look for posts.
    Wake,
    /// The DM's channel, once a message came from it.
    Channel(u64),
    /// Who wrote the place's latest message: the DM a card prefers.
    Author(u64),
    /// The message a turn's first streamed message replies to.
    Anchor(u64),
}

/// Why a call to Discord did not go through.
#[derive(Debug)]
pub(crate) struct SendErr {
    /// Discord is away (or busy): try again later. Otherwise it refused for
    /// good, and no retry changes that.
    pub away: bool,
    /// The call may have reached Discord anyway: a timeout, a 5xx, a
    /// connection that closed after the request went out.
    pub unsure: bool,
    /// The message is gone (404 on an edit).
    pub gone: bool,
    pub message: String,
}

impl SendErr {
    fn refused(message: impl Into<String>) -> Self {
        Self {
            away: false,
            unsure: false,
            gone: false,
            message: message.into(),
        }
    }

    pub(crate) fn of(e: &twilight_http::Error) -> Self {
        use twilight_http::error::ErrorType as E;
        let message = e.to_string();
        let (away, unsure, gone) = match e.kind() {
            E::Response { status, .. } => match status.get() {
                429 => (true, false, false),
                404 => (false, false, true),
                s if s >= 500 => (true, true, false),
                _ => (false, false, false),
            },
            E::RequestError => (true, !refused_connection(e), false),
            E::RequestTimedOut | E::RequestCanceled | E::Parsing { .. } => (true, true, false),
            E::BuildingRequest | E::Json | E::Validation => (false, false, false),
            // A token that is not one, or that Discord revoked: nothing is sent,
            // and the posts wait for a restart with a good one.
            _ => (true, false, false),
        };
        Self {
            away,
            unsure,
            gone,
            message,
        }
    }
}

/// A connection Discord (or the proxy) refused: the request never went out.
fn refused_connection(e: &twilight_http::Error) -> bool {
    let mut src: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(e);
    while let Some(s) = src {
        if let Some(io) = s.downcast_ref::<std::io::Error>() {
            if io.kind() == std::io::ErrorKind::ConnectionRefused {
                return true;
            }
        }
        src = s.source();
    }
    format!("{e:?}").contains("ConnectionRefused")
}

/// A create's nonce: from its message's key, which names the turn or the
/// post it belongs to, so every send of one message carries the same one.
/// A decimal u64, under Discord's 25 characters.
pub fn nonce(key: &str) -> String {
    let d = Sha256::digest(format!("theseus/{key}").as_bytes());
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_be_bytes(b).to_string()
}

/// One message a post writes: created, or edited when its message is known.
struct Write {
    key: String,
    channel: u64,
    content: String,
    buttons: Buttons,
    reply_to: Option<u64>,
    /// The message to edit, when the post knows it (a card's settle).
    message: Option<u64>,
}

/// What a post comes to: its writes, and what its completion keeps besides
/// the messages.
struct Plan {
    writes: Vec<Write>,
    extra: Value,
}

impl Plan {
    fn nothing(why: &str) -> Self {
        Self {
            writes: vec![],
            extra: json!({"skipped": why}),
        }
    }
}

pub(crate) struct Lane {
    pub shared: Arc<Shared>,
    /// `discord:dm:<user>`, `discord:channel:<id>`, or `discord:operator`.
    pub target: String,
    /// "dm", "channel", or "operator".
    pub kind: &'static str,
    /// As a card names the place: `#general`, `DM @eddie`.
    pub label: String,
    pub channel: Option<u64>,
    pub dm_user: Option<u64>,
    pub last_author: Option<u64>,
    /// key → (channel, message) of every message this lane wrote.
    pub msgs: HashMap<String, (u64, u64)>,
    /// key → what Discord last got for it.
    pub sent: HashMap<String, String>,
    /// Keys a post made final: the stream's later states of them are dropped.
    pub sealed: HashSet<String>,
    /// Live ops waiting: the latest per message, in the order they first came.
    pub live: Vec<Op>,
    pub anchor: Option<u64>,
    /// Discord is away until then: nothing is tried, and live ops drop.
    pub retry_at: Option<tokio::time::Instant>,
    pub attempt: u32,
    /// Posts whose earlier sends may have landed in this process.
    pub unsure: HashSet<String>,
    /// When this process started, in unix ms: a post dispatched before it may
    /// have landed.
    pub started_ms: u64,
}

impl Lane {
    pub(crate) fn new(
        shared: Arc<Shared>,
        target: String,
        kind: &'static str,
        label: String,
        channel: Option<u64>,
        dm_user: Option<u64>,
    ) -> Self {
        Self {
            shared,
            target,
            kind,
            label,
            channel,
            dm_user,
            last_author: None,
            msgs: HashMap::new(),
            sent: HashMap::new(),
            sealed: HashSet::new(),
            live: Vec::new(),
            anchor: None,
            retry_at: None,
            attempt: 0,
            unsure: HashSet::new(),
            started_ms: theseus_protocol::now_unix_ms(),
        }
    }

    pub(crate) async fn run(mut self, mut rx: mpsc::UnboundedReceiver<LaneMsg>) {
        loop {
            let msg = match self.retry_at {
                Some(at) => tokio::select! {
                    m = rx.recv() => m.map(Some),
                    _ = tokio::time::sleep_until(at) => Some(None),
                },
                None => rx.recv().await.map(Some),
            };
            match msg {
                None => break,
                Some(Some(m)) => self.take(m),
                Some(None) => self.retry_at = None,
            }
            // Everything queued meanwhile: live ops collapse to their latest.
            while let Ok(m) = rx.try_recv() {
                self.take(m);
            }
            if self
                .retry_at
                .is_some_and(|at| tokio::time::Instant::now() < at)
            {
                continue;
            }
            self.retry_at = None;
            // The posts first, in order; then the stream.
            if self.deliver_posts().await {
                self.apply_live().await;
            }
        }
    }

    fn take(&mut self, m: LaneMsg) {
        match m {
            LaneMsg::Live(op) => {
                if self.retry_at.is_none() {
                    self.queue_live(op);
                }
            }
            LaneMsg::Wake => {}
            LaneMsg::Channel(c) => self.channel = Some(c),
            LaneMsg::Author(a) => self.last_author = Some(a),
            LaneMsg::Anchor(a) => self.anchor = Some(a),
        }
    }

    /// Keep the latest state of each message, where it first came.
    fn queue_live(&mut self, op: Op) {
        let Some(key) = op.key().map(str::to_string) else {
            if !self.live.iter().any(|o| matches!(o, Op::Typing)) {
                self.live.push(op);
            }
            return;
        };
        if self.sealed.contains(&key) {
            return;
        }
        match self.live.iter_mut().find(|o| o.key() == Some(key.as_str())) {
            Some(slot) => *slot = op,
            None => self.live.push(op),
        }
    }

    /// Discord is away: wait, and drop the stream meanwhile.
    fn away(&mut self, e: &SendErr) {
        self.attempt += 1;
        let delay = Duration::from_secs(1 << self.attempt.min(6)).min(BACKOFF_MAX);
        self.retry_at = Some(tokio::time::Instant::now() + delay);
        self.live.clear();
        self.shared.core.outbox.error("discord", e.message.clone());
        if self.attempt == 1 {
            // Once per outage in the ledger; health keeps the latest.
            self.shared
                .board
                .error("deliver", None, format!("{}: {}", self.target, e.message));
        } else {
            tracing::debug!(target = %self.target, error = %e.message, attempt = self.attempt, "discord still away");
        }
    }

    // ------------------------------------------------------------ posts

    /// Deliver this lane's posts in order. False when Discord is away.
    async fn deliver_posts(&mut self) -> bool {
        while let Some(a) = self.shared.core.outbox.next_for(&self.target) {
            match self.deliver(&a).await {
                Ok(()) => self.attempt = 0,
                Err(e) => {
                    if e.unsure {
                        self.unsure.insert(a.correlation_id.clone());
                    }
                    self.away(&e);
                    return false;
                }
            }
        }
        true
    }

    /// One post: planned into writes, dispatched, written, settled. An error
    /// back means Discord is away (or the store would not take the settle)
    /// and the post waits, dispatched; a refusal settles it as failed, and the
    /// lane goes on.
    async fn deliver(&mut self, a: &Action) -> Result<(), SendErr> {
        let t0 = std::time::Instant::now();
        let plan = match self.plan(a).await {
            Ok(p) => p,
            Err(e) if e.away => return Err(e),
            Err(e) => {
                return self.settle(a, Outcome::Failed, vec![], json!({"error": e.message}));
            }
        };
        if plan.writes.is_empty() {
            return self.settle(a, Outcome::Succeeded, vec![], plan.extra);
        }
        let before = a.dispatched_at_ms;
        let dispatched = match self.shared.core.outbox.dispatch(&a.correlation_id) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), post = %a.correlation_id, "outbox dispatch failed");
                return Err(SendErr {
                    away: true,
                    unsure: false,
                    gone: false,
                    message: format!("dispatch: {e:#}"),
                });
            }
        };
        // Sent before and maybe landed, longer ago than Discord keeps a
        // nonce: a new create says it may be a copy.
        let first = dispatched.dispatched_at_ms.unwrap_or(0);
        let maybe_landed =
            self.unsure.contains(&a.correlation_id) || before.is_some_and(|d| d < self.started_ms);
        let resend =
            maybe_landed && theseus_protocol::now_unix_ms().saturating_sub(first) > NONCE_WINDOW_MS;
        let mut messages = Vec::new();
        for mut w in plan.writes {
            if resend && w.message.is_none() && !self.msgs.contains_key(&w.key) {
                w.content = format!("{}\n{}", w.content, render::RESENT);
            }
            match self.write(&w).await {
                Ok(Some((c, m))) => messages.push((w.key.clone(), c, m)),
                Ok(None) => {}
                Err(e) if e.away => return Err(e),
                Err(e) if e.gone && w.message.is_some() => {
                    // A settle whose card was deleted: nothing left to edit.
                    tracing::info!(post = %a.correlation_id, key = %w.key, "the message to edit is gone");
                }
                Err(e) => {
                    return self.settle(a, Outcome::Failed, messages, json!({"error": e.message}));
                }
            }
        }
        let mut extra = plan.extra;
        extra["ms"] = json!(t0.elapsed().as_millis() as u64);
        if resend {
            extra["resent"] = json!(true);
        }
        self.settle(a, Outcome::Succeeded, messages, extra)
    }

    /// Record how a post went. When the store will not take it, the post is
    /// still open: the lane backs off and sends it again later, under the same
    /// nonce, rather than taking it up again at once.
    fn settle(
        &mut self,
        a: &Action,
        outcome: Outcome,
        messages: Vec<(String, u64, u64)>,
        mut detail: Value,
    ) -> Result<(), SendErr> {
        let first = messages.first().map(|(_, _, m)| m.to_string());
        detail["messages"] = messages
            .iter()
            .map(|(k, c, m)| json!({"key": k, "channel": c.to_string(), "id": m.to_string()}))
            .collect();
        match self
            .shared
            .core
            .outbox
            .settle(&a.correlation_id, outcome, first, detail, "discord")
        {
            Ok(_) => {
                self.unsure.remove(&a.correlation_id);
                Ok(())
            }
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), post = %a.correlation_id, "outbox settle failed");
                Err(SendErr {
                    away: true,
                    unsure: true,
                    gone: false,
                    message: format!("settle: {e:#}"),
                })
            }
        }
    }

    /// A post's writes, by its kind.
    async fn plan(&mut self, a: &Action) -> Result<Plan, SendErr> {
        let body = body_of(a).clone();
        let corr = &a.correlation_id;
        let text = |t: String, key: String, channel: u64, reply_to: Option<u64>| Write {
            key,
            channel,
            content: t,
            buttons: Buttons::Keep,
            reply_to,
            message: None,
        };
        let reply_to = body["reply_to"].as_str().and_then(|s| s.parse().ok());
        match kind_of(a) {
            "reply" => {
                let channel = self.place_channel().await?;
                Ok(self.reply(&body, channel))
            }
            "card" => self.card(&body).await,
            "settle" => Ok(self.settled(&body)),
            "notice" => {
                let channel = self.place_channel().await?;
                let t = body["text"].as_str().unwrap_or("").to_string();
                Ok(Plan {
                    writes: vec![text(t, format!("note:{corr}"), channel, reply_to)],
                    extra: json!({}),
                })
            }
            "failed" => {
                let channel = self.place_channel().await?;
                let key = match body["turn_id"].as_str() {
                    Some(t) => format!("{t}:failed"),
                    None => format!("failed:{corr}"),
                };
                let t = render::failed(
                    body["class"].as_str().unwrap_or("error"),
                    body["error"].as_str().unwrap_or(""),
                );
                Ok(Plan {
                    writes: vec![text(t, key, channel, None)],
                    extra: json!({}),
                })
            }
            "report" => {
                // A task's report (DD7): one message, in the place the task
                // reports to, under a key of its own, so it posts once.
                let channel = self.place_channel().await?;
                let said = body["node"]
                    .as_str()
                    .and_then(|n| self.shared.core.outbox.said(n));
                let t = render::report(&body, said.as_deref());
                let key = format!("report:{}", body["task"].as_str().unwrap_or(corr));
                Ok(Plan {
                    writes: vec![text(t, key, channel, None)],
                    extra: json!({"task": body["task"]}),
                })
            }
            "refusal" | "restarted" => {
                let t = if kind_of(a) == "refusal" {
                    render::job_refusal(&body["params"])
                } else {
                    let tables: Vec<String> = body["tables"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect();
                    render::restarted(body["at_unix_ms"].as_u64().unwrap_or(0), &tables)
                };
                let (channel, place) = self.operator_channel(&body).await?;
                Ok(Plan {
                    writes: vec![text(t, format!("note:{corr}"), channel, None)],
                    extra: json!({"place": place}),
                })
            }
            other => Err(SendErr::refused(format!(
                "a post of kind {other:?} is not one this binding knows"
            ))),
        }
    }

    /// A turn's reply: its loops' final text under the stream's keys, which
    /// edits what the stream posted and creates what it did not.
    fn reply(&mut self, body: &Value, channel: u64) -> Plan {
        let turn_id = body["turn_id"].as_str().unwrap_or("").to_string();
        let texts = self.shared.core.outbox.reply_texts(body);
        let result: Option<TurnSubmitResult> = serde_json::from_value(body["result"].clone()).ok();
        let mut parts = render::reply_parts(&turn_id, &texts, result.as_ref());
        // A wake's turn says which wake woke it (DD8).
        let wakes: Vec<String> = body["wakes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|w| w["text"].as_str().map(str::to_string))
            .collect();
        render::wake_header(&mut parts, &wakes);
        // Replying to the turn's message once: only when the stream posted
        // nothing of it.
        let prefix = format!("{turn_id}:");
        let streamed = self.msgs.keys().any(|k| k.starts_with(&prefix));
        let mut anchor = (!streamed)
            .then(|| body["reply_to"].as_str().and_then(|s| s.parse().ok()))
            .flatten();
        let writes = parts
            .into_iter()
            .map(|(key, content)| {
                self.sealed.insert(key.clone());
                Write {
                    key,
                    channel,
                    content,
                    buttons: Buttons::Keep,
                    reply_to: anchor.take(),
                    message: None,
                }
            })
            .collect();
        Plan {
            writes,
            extra: json!({"turn_id": turn_id}),
        }
    }

    /// A confirm card, where its route says: here, in a trusted DM with a
    /// note here, or only a note here. A question that closed while the card
    /// waited is posted all the same: its settle is next in line, and edits
    /// it to say how it closed.
    async fn card(&mut self, body: &Value) -> Result<Plan, SendErr> {
        let q = body["question"].as_str().unwrap_or("").to_string();
        let core = self.shared.core.clone();
        let question = match core.kernel.action(&q) {
            Ok(Some(a)) => a,
            Ok(None) => return Ok(Plan::nothing("its question is not in the store")),
            Err(e) => return Err(SendErr::refused(format!("{e:#}"))),
        };
        let req = match core.question_request(&question, body["node"].as_str()) {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(Plan::nothing("its question could not be read")),
            Err(e) => return Err(SendErr::refused(format!("{e:#}"))),
        };
        let channel = self.place_channel().await?;
        let route = self.route().await?;
        let elsewhere = core
            .approval
            .elsewhere()
            .unwrap_or_else(|| render::ELSEWHERE.to_string());
        let card = render::card(&req, &route, &elsewhere);
        let note = render::card_note(&route, &card.line, &elsewhere);
        let key = format!("confirm:{q}");
        let note_key = format!("approval:{q}");
        let buttons = Buttons::Confirm(q.clone());
        let mut writes = Vec::new();
        let (how, dm) = match &route {
            Route::Here => {
                writes.push(Write {
                    key,
                    channel,
                    content: card.content,
                    buttons,
                    reply_to: None,
                    message: None,
                });
                ("here", None)
            }
            Route::Dm { user, dm, .. } => {
                let dm_channel = self.shared.dm_channel(*user).await?;
                writes.push(Write {
                    key,
                    channel: dm_channel,
                    content: card.content,
                    buttons,
                    reply_to: None,
                    message: None,
                });
                ("dm", Some(dm.clone()))
            }
            Route::Elsewhere { .. } => ("elsewhere", None),
        };
        if let Some(n) = note {
            writes.push(Write {
                key: note_key,
                channel,
                content: n,
                buttons: Buttons::Keep,
                reply_to: None,
                message: None,
            });
        }
        Ok(Plan {
            writes,
            extra: json!({"question": q, "route": how, "dm": dm, "line": card.line, "budget": card.budget}),
        })
    }

    /// How a card's question closed: each message its card posted, edited,
    /// the card's buttons gone.
    fn settled(&mut self, body: &Value) -> Plan {
        let closed: Closed = serde_json::from_value(body["closed"].clone()).unwrap_or_default();
        let card_id = body["card"].as_str().unwrap_or("");
        let card = match self.shared.core.kernel.outbox_action(card_id) {
            Ok(Some(c)) if c.state == ActionState::Succeeded => c,
            _ => return Plan::nothing("its card was never posted"),
        };
        let d = card.detail.unwrap_or_default();
        let line = d["line"].as_str().unwrap_or("");
        let content = render::settled(&closed, line, d["budget"].as_bool().unwrap_or(false));
        let dm = (d["route"] == "dm").then(|| d["dm"].as_str().unwrap_or("the DM"));
        let mut writes = Vec::new();
        for m in d["messages"].as_array().into_iter().flatten() {
            let key = m["key"].as_str().unwrap_or("").to_string();
            let (Some(channel), Some(message)) = (
                m["channel"].as_str().and_then(|s| s.parse().ok()),
                m["id"].as_str().and_then(|s| s.parse().ok()),
            ) else {
                continue;
            };
            let (content, buttons) = if key.starts_with("confirm:") {
                (content.clone(), Buttons::Clear)
            } else {
                (render::settled_note(&content, dm), Buttons::Keep)
            };
            writes.push(Write {
                key,
                channel,
                content,
                buttons,
                reply_to: None,
                message: Some(message),
            });
        }
        if writes.is_empty() {
            return Plan::nothing("its card posted no message");
        }
        Plan {
            writes,
            extra: json!({"card": card_id, "closed": closed.how}),
        }
    }

    /// Where a card goes (theseus-sgh): here when the place is a trusted
    /// channel, or with no `[approval]`; else a trusted user's DM, with a note
    /// here; else the note alone.
    async fn route(&mut self) -> Result<Route, SendErr> {
        let ap = &self.shared.core.approval;
        if !ap.configured() {
            return Ok(Route::Here);
        }
        let channel = self.channel;
        let why = match (self.kind, channel, self.dm_user) {
            ("dm", _, Some(u)) if ap.trusts_dm(u, channel) => return Ok(Route::Here),
            ("dm", _, Some(u)) if !ap.discord_users().contains(&u) => {
                "its user is not in [approval] trusted_users".to_string()
            }
            ("dm", ..) => "[approval] channels does not list \"discord:dm\"".to_string(),
            (_, Some(c), _) if ap.lists_discord_channel(c) => {
                if self.shared.check_channel(c).await {
                    return Ok(Route::Here);
                }
                ap.checked(c).map(|k| k.detail).unwrap_or_default()
            }
            _ => "it is not listed in [approval] channels".to_string(),
        };
        self.shared.open_dm_channels().await?;
        Ok(match self.shared.approval_dm(self.last_author) {
            Some((user, dm)) => Route::Dm {
                user,
                dm,
                place: self.label.clone(),
                why,
            },
            None => Route::Elsewhere { why },
        })
    }

    /// This place's channel; a DM's opens on first use.
    async fn place_channel(&mut self) -> Result<u64, SendErr> {
        if let Some(c) = self.channel {
            return Ok(c);
        }
        match self.dm_user {
            Some(u) => {
                let c = self.shared.dm_channel(u).await?;
                self.channel = Some(c);
                Ok(c)
            }
            None => Err(SendErr::refused(format!(
                "{} is no place with a channel",
                self.target
            ))),
        }
    }

    /// Where an operator's notice goes: the DM approvals go to, else the
    /// place of the session it concerns. Returns the channel and its label.
    async fn operator_channel(&mut self, body: &Value) -> Result<(u64, String), SendErr> {
        self.shared.open_dm_channels().await?;
        if let Some((user, dm)) = self.shared.approval_dm(None) {
            return Ok((self.shared.dm_channel(user).await?, dm));
        }
        let fallback = body["fallback"].as_str().unwrap_or("");
        if let Some(id) = fallback
            .strip_prefix("discord:channel:")
            .and_then(|s| s.parse().ok())
        {
            return Ok((id, fallback.to_string()));
        }
        if let Some(u) = fallback
            .strip_prefix("discord:dm:")
            .and_then(|s| s.parse().ok())
        {
            return Ok((self.shared.dm_channel(u).await?, fallback.to_string()));
        }
        Err(SendErr::refused(
            "no DM takes approvals, and the notice names no place to fall back to",
        ))
    }

    // ------------------------------------------------------------ writing

    /// Create or edit one message. None when there is nothing to send (empty
    /// text, or what Discord already has).
    async fn write(&mut self, w: &Write) -> Result<Option<(u64, u64)>, SendErr> {
        if w.content.trim().is_empty() {
            return Ok(None);
        }
        let known = w
            .message
            .map(|m| (w.channel, m))
            .or_else(|| self.msgs.get(&w.key).copied());
        if let Some((c, m)) = known {
            if self.sent.get(&w.key) == Some(&w.content) && w.buttons == Buttons::Keep {
                return Ok(Some((c, m)));
            }
            match self.edit(c, m, &w.content, &w.buttons).await {
                Ok(()) => {
                    self.sent.insert(w.key.clone(), w.content.clone());
                    self.msgs.insert(w.key.clone(), (c, m));
                    return Ok(Some((c, m)));
                }
                // A message someone deleted: a post's own part is posted again.
                Err(e) if e.gone && w.message.is_none() => {
                    self.msgs.remove(&w.key);
                }
                Err(e) => return Err(e),
            }
        }
        let (m, landed) = self
            .create(w.channel, &w.key, &w.content, &w.buttons, w.reply_to)
            .await?;
        self.msgs.insert(w.key.clone(), (w.channel, m));
        if landed != w.content {
            // The nonce returned an earlier send of this message (the stream's,
            // or one before a restart): bring it to this state.
            self.edit(w.channel, m, &w.content, &w.buttons).await?;
        }
        self.sent.insert(w.key.clone(), w.content.clone());
        Ok(Some((w.channel, m)))
    }

    /// A new message, with its key's nonce and `enforce_nonce`: a second send
    /// of it returns the first message. Returns its id and the content Discord
    /// has for it, which is an earlier send's when the nonce matched one.
    async fn create(
        &mut self,
        channel: u64,
        key: &str,
        content: &str,
        buttons: &Buttons,
        reply_to: Option<u64>,
    ) -> Result<(u64, String), SendErr> {
        let mut body = json!({
            "content": content,
            "nonce": nonce(key),
            "enforce_nonce": true,
            "allowed_mentions": {"parse": [], "replied_user": false},
        });
        let comps = match buttons {
            Buttons::Confirm(corr) => confirm_buttons(corr),
            Buttons::ShouldHaveAsked(options) => asked_menu(options),
            Buttons::Clear | Buttons::Keep => vec![],
        };
        if !comps.is_empty() {
            body["components"] = serde_json::to_value(&comps).unwrap_or_default();
        }
        if let Some(a) = reply_to {
            body["message_reference"] =
                json!({"message_id": a.to_string(), "fail_if_not_exists": false});
        }
        let req = Request::builder(&HttpRoute::CreateMessage {
            channel_id: channel,
        })
        .json(&body)
        .build()
        .map_err(|e| SendErr::of(&e))?;
        let resp = self
            .shared
            .http
            .request::<Message>(req)
            .await
            .map_err(|e| SendErr::of(&e))?;
        let m = resp.model().await.map_err(|e| SendErr {
            away: true,
            unsure: true,
            gone: false,
            message: format!("reading the sent message: {e}"),
        })?;
        self.shared.board.update(|s| s.messages_out += 1);
        self.shared.core.binding_ledger(
            "discord.message.out",
            None,
            json!({"place": self.label, "message_id": m.id.to_string(), "part": key,
                   "chars": content.chars().count(), "buttons": matches!(buttons, Buttons::Confirm(_)),
                   "menu": matches!(buttons, Buttons::ShouldHaveAsked(_))}),
        );
        Ok((m.id.get(), m.content))
    }

    async fn edit(
        &self,
        channel: u64,
        message: u64,
        content: &str,
        buttons: &Buttons,
    ) -> Result<(), SendErr> {
        let none = AllowedMentions::default();
        let comps = match buttons {
            Buttons::Confirm(corr) => Some(confirm_buttons(corr)),
            Buttons::ShouldHaveAsked(options) => Some(asked_menu(options)),
            Buttons::Clear => Some(vec![]),
            Buttons::Keep => None,
        };
        let mut req = self
            .shared
            .http
            .update_message(Id::new(channel), Id::new(message))
            .content(Some(content))
            .allowed_mentions(Some(&none));
        if let Some(c) = &comps {
            req = req.components(Some(c));
        }
        req.await.map_err(|e| SendErr::of(&e))?;
        self.shared.board.update(|s| s.edits += 1);
        Ok(())
    }

    // ------------------------------------------------------------ live

    /// The stream's waiting ops, best-effort: the first failure drops the rest.
    async fn apply_live(&mut self) {
        let ops = std::mem::take(&mut self.live);
        for op in ops {
            if self.retry_at.is_some() {
                break;
            }
            let r = match op {
                Op::Typing => self.typing().await,
                Op::Upsert {
                    key,
                    content,
                    buttons,
                } => {
                    if self.sealed.contains(&key) {
                        continue;
                    }
                    let Ok(channel) = self.place_channel().await else {
                        continue;
                    };
                    let reply_to = (!self.msgs.contains_key(&key))
                        .then(|| self.anchor.take())
                        .flatten();
                    self.write(&Write {
                        key,
                        channel,
                        content,
                        buttons,
                        reply_to,
                        message: None,
                    })
                    .await
                    .map(|_| ())
                }
                Op::Notice { key, card } => self.notice(&key, &card).await,
            };
            match r {
                Ok(()) => {}
                Err(e) if e.away => self.away(&e),
                Err(e) => self.shared.board.error("live message", None, e.message),
            }
        }
    }

    async fn typing(&mut self) -> Result<(), SendErr> {
        let Some(c) = self.channel else {
            return Ok(());
        };
        self.shared
            .http
            .create_typing_trigger(Id::new(c))
            .await
            .map(|_| ())
            .map_err(|e| SendErr::of(&e))
    }

    /// A structured notice (a Discord embed, no mentions): live, when
    /// `[discord] notice_embeds` is on.
    async fn notice(&mut self, key: &str, card: &NoticeCard) -> Result<(), SendErr> {
        let channel = self.place_channel().await?;
        let mut b = twilight_util::builder::embed::EmbedBuilder::new()
            .title(card.title.clone())
            .color(card.color)
            .description(card.description.clone());
        for (name, value) in &card.fields {
            if !value.trim().is_empty() {
                b = b.field(twilight_util::builder::embed::EmbedFieldBuilder::new(
                    name.clone(),
                    value.clone(),
                ));
            }
        }
        let embeds = [b.build()];
        let none = AllowedMentions::default();
        // Its "Should have asked" button, or none once the tool asks first.
        let comps = asked_button(card.ask.as_ref());
        let http = &self.shared.http;
        match self.msgs.get(key).copied() {
            Some((c, m)) => {
                http.update_message(Id::new(c), Id::new(m))
                    .embeds(Some(&embeds))
                    .components(Some(&comps))
                    .allowed_mentions(Some(&none))
                    .await
                    .map_err(|e| SendErr::of(&e))?;
                self.shared.board.update(|s| s.edits += 1);
            }
            None => {
                let mut req = http
                    .create_message(Id::new(channel))
                    .embeds(&embeds)
                    .allowed_mentions(Some(&none));
                if !comps.is_empty() {
                    req = req.components(&comps);
                }
                let m = req
                    .await
                    .map_err(|e| SendErr::of(&e))?
                    .model()
                    .await
                    .map_err(|e| SendErr {
                        away: true,
                        unsure: true,
                        gone: false,
                        message: e.to_string(),
                    })?;
                self.msgs.insert(key.to_string(), (channel, m.id.get()));
                self.shared.board.update(|s| s.messages_out += 1);
                self.shared.core.binding_ledger(
                    "discord.message.out",
                    None,
                    json!({"place": self.label, "message_id": m.id.to_string(), "part": key, "notice": card.title}),
                );
            }
        }
        Ok(())
    }
}

/// Wake every lane when the outbox changes, and every half minute besides.
pub(crate) async fn courier(shared: Arc<Shared>) {
    let mut rx = shared.core.outbox.subscribe();
    let mut tick = tokio::time::interval(Duration::from_secs(30));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            r = rx.changed() => if r.is_err() { break },
            _ = tick.tick() => {}
        }
        shared.wake_lanes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nonce_is_stable_per_message_and_fits_discord() {
        let a = nonce("turn_1:L0:p0");
        assert_eq!(a, nonce("turn_1:L0:p0"));
        assert_ne!(a, nonce("turn_1:L0:p1"));
        assert!(
            a.len() <= 25 && a.chars().all(|c| c.is_ascii_digit()),
            "{a}"
        );
    }
}
