//! Components V2 on a lane (theseus-2j2ka): the live board and the needs-you
//! card. A child of `courier`.
//!
//! - **A V2 write's `content` is its components, as JSON.** `Write` and the
//!   lane's `sent` map keep a string, so a V2 message is written with
//!   `Buttons::V2` and its components serialized into `content`: two writes of
//!   the same state compare equal and the second is not sent, and the create
//!   and the edit send the flag and the components, no `content`.
//! - **The board is edited, never posted anew, and at most once in
//!   `BOARD_EVERY`.** A state that comes sooner waits for the rest of the
//!   interval, and the latest one replaces it (`queue_live`'s rule). The lane
//!   wakes for it (`State::due`) without a message.
//! - **At most one ping per `CARD_EVERY` per place for a needs-you card.** A
//!   card that comes inside the window is created silent, and when the window
//!   ends with any such card still open, one message says how many need you
//!   (`State::summary`): a burst is one buzz. The message is edited to say the
//!   questions were answered once none is open.
//! - **A bot that cannot send V2** (Discord refuses the create) falls back to
//!   the text board and the classic card, for the process: `Tasks::v2_refused`.

use std::collections::HashSet;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::time::Instant;
use twilight_http::request::Request;
use twilight_http::routing::Route as HttpRoute;

use super::{Lane, SendErr};
use crate::render::{self, board_v2, v2, Buttons, Op, BOARD_KEY};

/// The most often the board is edited.
pub(crate) const BOARD_EVERY: Duration = Duration::from_secs(2);
/// The most often a place's needs-you cards ping.
pub(crate) const CARD_EVERY: Duration = Duration::from_secs(30);

#[cfg(test)]
thread_local! {
    /// A test's own window, so a burst's summary is seen in seconds.
    static CARD_WINDOW: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

/// A test shortens the window for its thread.
#[cfg(test)]
pub(crate) fn card_window_for_tests(d: Duration) {
    CARD_WINDOW.with(|w| w.set(Some(d)));
}

/// The window now: `CARD_EVERY`, or a test's.
fn card_every() -> Duration {
    #[cfg(test)]
    if let Some(d) = CARD_WINDOW.with(std::cell::Cell::get) {
        return d;
    }
    CARD_EVERY
}

/// What a lane keeps for V2.
#[derive(Default)]
pub(crate) struct State {
    /// When the board was last written.
    board_at: Option<Instant>,
    /// A board state is waiting for this.
    board_due: Option<Instant>,
    /// When a needs-you card last pinged here, and for which question.
    card_ping: Option<(Instant, String)>,
    /// Cards created silent inside the window, by question id.
    silent: Vec<String>,
    /// The board to write is an idle one: only where a board is pinned.
    pub only_if_pinned: bool,
    /// The burst's summary is due then.
    summary_due: Option<Instant>,
    /// The summary message, once posted: (channel, message).
    summary: Option<(u64, u64)>,
}

impl State {
    /// The next time the lane should wake with nothing new from its place.
    pub fn due(&self) -> Option<Instant> {
        [self.board_due, self.summary_due]
            .into_iter()
            .flatten()
            .min()
    }

    /// Whether a needs-you card created now may ping, and if not, that it was
    /// created silent. A card pings unless another that pinged less than
    /// `CARD_EVERY` ago is still waiting: one question answered and a next
    /// asked buzzes again, a burst of questions buzzes once. `open` says whether
    /// a question is still waiting.
    pub fn card_may_ping(
        &mut self,
        question: &str,
        now: Instant,
        open: impl Fn(&str) -> bool,
    ) -> bool {
        match &self.card_ping {
            Some((at, last)) if now < *at + card_every() && last != question && open(last) => {
                let due = *at + card_every();
                if !self.silent.iter().any(|q| q == question) {
                    self.silent.push(question.to_string());
                }
                self.summary_due.get_or_insert(due);
                false
            }
            _ => true,
        }
    }

    /// A needs-you card pinged.
    pub fn card_pinged(&mut self, question: &str, now: Instant) {
        self.card_ping = Some((now, question.to_string()));
    }
}

/// A question id out of a card's key (`confirm:<question>`).
pub(super) fn question_of(key: &str) -> Option<&str> {
    key.strip_prefix("confirm:")
}

impl Lane {
    /// A failed V2 call. A 400 means this bot cannot send Components V2 (or a
    /// message of ours is malformed, which `v2::validate` and the tests are
    /// for): the process falls back to the text board and the classic card, and
    /// the call is tried again as one of those.
    fn failure(&self, e: &twilight_http::Error) -> SendErr {
        let err = SendErr::of(e);
        if err.away || err.gone || !err.message.contains("400") || self.shared.tasks.v2_refused() {
            return err;
        }
        tracing::warn!(place = %self.label, error = %err.message, "discord refused a Components V2 message: the text board and the classic card take over");
        self.shared.tasks.refuse_v2();
        self.shared.wake_places_for_board();
        SendErr {
            away: true,
            unsure: false,
            gone: false,
            message: format!("Components V2 refused, falling back: {}", err.message),
        }
    }

    /// A V2 message made: its id, and the content it was made from.
    pub(super) async fn create_v2(
        &mut self,
        (channel, key, content): (u64, &str, &str),
        mentions: &[u64],
        (wants, ping): (bool, bool),
    ) -> Result<(u64, String), SendErr> {
        let comps: Vec<Value> =
            serde_json::from_str(content).map_err(|e| SendErr::refused(e.to_string()))?;
        let mut body = v2::body(&comps, !ping);
        body["nonce"] = json!(super::nonce(key));
        body["enforce_nonce"] = json!(true);
        let mut allowed = json!({"parse": [], "replied_user": false});
        if !mentions.is_empty() {
            allowed["users"] = json!(mentions.iter().map(u64::to_string).collect::<Vec<_>>());
        }
        body["allowed_mentions"] = allowed;
        let req = Request::builder(&HttpRoute::CreateMessage {
            channel_id: channel,
        })
        .json(&body)
        .build()
        .map_err(|e| SendErr::of(&e))?;
        let resp = self
            .shared
            .http
            .request::<Value>(req)
            .await
            .map_err(|e| self.failure(&e))?;
        let m = resp.model().await.map_err(|e| SendErr {
            away: true,
            unsure: true,
            gone: false,
            message: format!("reading the sent message: {e}"),
        })?;
        let id: u64 = m["id"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| SendErr {
                away: true,
                unsure: true,
                gone: false,
                message: "the sent message has no id".into(),
            })?;
        self.shared.board.update(|s| s.messages_out += 1);
        self.shared.core.binding_ledger_soon(
            theseus_protocol::LedgerKind::DiscordMessageOut,
            None,
            json!({"place": self.label, "message_id": id.to_string(), "part": key, "ping": ping,
                   "held": wants && !ping, "chars": v2::text_len(&comps), "buttons": true,
                   "menu": false, "mentions": mentions.iter().map(u64::to_string).collect::<Vec<_>>(),
                   "components_v2": true}),
        );
        Ok((id, content.to_string()))
    }

    /// A V2 message edited to `content`'s components.
    pub(super) async fn edit_v2(
        &self,
        channel: u64,
        message: u64,
        content: &str,
    ) -> Result<(), SendErr> {
        let comps: Vec<Value> =
            serde_json::from_str(content).map_err(|e| SendErr::refused(e.to_string()))?;
        // The flag goes with every edit: a V2 message keeps it, and an edit
        // that left it out would read as one that removes it.
        let body = json!({"flags": v2::IS_COMPONENTS_V2, "components": comps,
                          "allowed_mentions": {"parse": []}});
        let req = Request::builder(&HttpRoute::UpdateMessage {
            channel_id: channel,
            message_id: message,
        })
        .json(&body)
        .build()
        .map_err(|e| SendErr::of(&e))?;
        self.shared
            .http
            .request::<Value>(req)
            .await
            .map_err(|e| self.failure(&e))?;
        self.shared.board.update(|s| s.edits += 1);
        Ok(())
    }

    /// The board's latest state: a V2 one is held to `BOARD_EVERY`.
    pub(super) async fn board_op(
        &mut self,
        content: String,
        buttons: Buttons,
    ) -> Result<(), SendErr> {
        if buttons != Buttons::V2 {
            return self.board(content).await;
        }
        // The state Discord has already: nothing to wait for or to send.
        if self.sent.get(BOARD_KEY) == Some(&content) && self.msgs.contains_key(BOARD_KEY) {
            return Ok(());
        }
        let now = Instant::now();
        if let Some(at) = self.v2.board_at.filter(|at| now < *at + BOARD_EVERY) {
            // Too soon: the latest state waits for the interval, in the lane's
            // live ops, where a newer state replaces it.
            self.v2.board_due = Some(at + BOARD_EVERY);
            self.live.push(Op::Upsert {
                key: BOARD_KEY.into(),
                content,
                buttons,
            });
            return Ok(());
        }
        self.v2.board_due = None;
        if self.board_v2(content).await? {
            self.v2.board_at = Some(Instant::now());
        }
        Ok(())
    }

    /// Write the V2 board: the pinned one found after a restart, else made.
    async fn board_v2(&mut self, content: String) -> Result<bool, SendErr> {
        let channel = self.place_channel().await?;
        if !self.msgs.contains_key(BOARD_KEY) && !self.board_sought {
            self.board_sought = true;
            if let Some(m) = self.find_board_v2(channel).await {
                tracing::info!(place = %self.label, message = m, "the task board: found the pinned one, edited in place");
                self.msgs.insert(BOARD_KEY.into(), (channel, m));
            }
        }
        if self.v2.only_if_pinned && !self.msgs.contains_key(BOARD_KEY) {
            return Ok(false);
        }
        let made = !self.msgs.contains_key(BOARD_KEY);
        let at = self
            .write(&super::Write {
                key: BOARD_KEY.into(),
                channel,
                content,
                buttons: Buttons::V2,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: false,
            })
            .await?;
        if let (true, Some((c, m))) = (made, at) {
            tracing::info!(place = %self.label, message = m, "the task board: made a new one");
            self.pin(c, m).await;
        }
        Ok(at.is_some())
    }

    /// The bot's pinned V2 board in `channel`: its message that carries the
    /// board's Clear button.
    async fn find_board_v2(&self, channel: u64) -> Option<u64> {
        let req = Request::builder(&HttpRoute::GetPins {
            channel_id: channel,
            limit: None,
            before: None,
        })
        .build()
        .ok()?;
        let pins = match self.shared.http.request::<Value>(req).await {
            Ok(r) => r.model().await.ok()?,
            Err(e) => {
                tracing::info!(place = %self.label, error = %e, "the task board: the pins could not be read");
                return None;
            }
        };
        pins["items"].as_array()?.iter().find_map(|p| {
            let m = &p["message"];
            let ours = m["author"]["bot"].as_bool() == Some(true)
                && m["flags"]
                    .as_u64()
                    .is_some_and(|f| f & v2::IS_COMPONENTS_V2 != 0)
                && m["components"].to_string().contains(board_v2::CLEAR);
            ours.then(|| m["id"].as_str().and_then(|s| s.parse().ok()))
                .flatten()
        })
    }

    /// The burst's summary, when its window ends with silent cards still open.
    pub(super) async fn v2_tick(&mut self) {
        let now = Instant::now();
        if self.v2.summary_due.is_none_or(|at| now < at) {
            return;
        }
        self.v2.summary_due = None;
        let open = self.open_total();
        if open == 0 {
            return;
        }
        let Ok(channel) = self.place_channel().await else {
            return;
        };
        let key = format!("needs-you:{}", self.clock);
        let key_for_summary = key.clone();
        let content = serde_json::to_string(&render::card_v2::summary(open)).unwrap_or_default();
        let ping = self.pings(crate::policy::Event::Card)
            && self.shared.tasks.quiet_until(&self.target).is_none();
        match self
            .write(&super::Write {
                key,
                channel,
                content,
                buttons: Buttons::V2,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping,
            })
            .await
        {
            Ok(Some((c, m))) => {
                self.v2.summary = Some((c, m));
                if ping {
                    self.v2.card_pinged(&key_for_summary, Instant::now());
                }
            }
            Ok(None) => {}
            Err(e) if e.away => self.away(&e),
            Err(_) => {}
        }
    }

    /// Whether the question `q` is still waiting for an answer.
    pub(super) fn question_open(&self, q: &str) -> bool {
        self.shared
            .core
            .kernel
            .action(q)
            .ok()
            .flatten()
            .is_some_and(|a| theseus_core::outbox::Closed::of(&a).is_none())
    }

    /// How many needs-you cards of the burst wait: the silent ones, and the
    /// one that pinged.
    fn open_total(&mut self) -> usize {
        let silent = self.open_silent();
        let pinged = self
            .v2
            .card_ping
            .as_ref()
            .map(|(_, q)| q.clone())
            .filter(|q| !self.v2.silent.contains(q) && self.question_open(q));
        silent + usize::from(pinged.is_some())
    }

    /// How many of the cards created silent are still waiting for an answer.
    fn open_silent(&mut self) -> usize {
        let open: HashSet<String> = self
            .v2
            .silent
            .iter()
            .filter(|q| self.question_open(q))
            .cloned()
            .collect();
        self.v2.silent.retain(|q| open.contains(q));
        open.len()
    }

    /// A card's question closed: when none of the silent ones is open any
    /// more, the summary says so and stops asking.
    pub(super) async fn v2_settled(&mut self, question: &str) {
        self.v2.silent.retain(|q| q != question);
        if self.open_total() > 0 {
            return;
        }
        self.v2.summary_due = None;
        if let Some((c, m)) = self.v2.summary.take() {
            let content = serde_json::to_string(&render::card_v2::summary(0)).unwrap_or_default();
            let _ = self.edit_v2(c, m, &content).await;
        }
    }
}
