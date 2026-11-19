//! The task board on its place's lane (M7 step 39b, theseus-ext.14): a live
//! upsert under `render::BOARD_KEY`, never replayed, its message id in
//! memory. A child of `courier`.
//!
//! - **Made once, pinned best-effort.** Its first write creates it, and the
//!   lane pins it when the bot may; a refused pin is logged once a lane.
//! - **After a restart** the id is gone with the process. The lane's first
//!   board write looks for the board and edits it, or makes a new one, and
//!   says which and why. Never on the start path: only at the place's first
//!   change after it. It looks (`seek_board`) first for the message id it
//!   recorded when it made the board (`discord.board:<channel>`, one read of
//!   that message: found whether pinned or not, however far up the channel
//!   it is), then among the channel's pins (the bot's message that begins
//!   with `render::BOARD_HEAD`, or the V2 board's Clear button). A read that
//!   fails for a reason that may pass (Discord away) is not an answer: the
//!   write waits and looks again, so a board that is there is never made a
//!   second time because Discord was busy. Cost: one read of one message and,
//!   when that finds nothing, the pins' one read, once a lane a run.

use serde::{Deserialize, Serialize};
use twilight_model::id::Id;

use super::{Lane, SendErr, Write};
use crate::render::{Buttons, BOARD_HEAD, BOARD_KEY};

/// The board's message in a channel, as the lane that made it recorded it.
#[derive(Serialize, Deserialize)]
struct Recorded {
    message: u64,
    /// A Components V2 board (else the text one).
    v2: bool,
}

/// META key of a channel's board.
fn record_key(channel: u64) -> String {
    format!("discord.board:{channel}")
}

impl Lane {
    /// Write the board's latest state.
    pub(super) async fn board(&mut self, content: String) -> Result<(), SendErr> {
        let channel = self.place_channel().await?;
        self.take_up_board(channel, false).await?;
        let made = !self.msgs.contains_key(BOARD_KEY);
        let at = self
            .write(&Write {
                key: BOARD_KEY.into(),
                channel,
                content,
                buttons: Buttons::Keep,
                reply_to: None,
                message: None,
                mentions: vec![],
                ping: self.pings(crate::policy::Event::Board),
            })
            .await?;
        if let (true, Some((c, m))) = (made, at) {
            tracing::info!(place = %self.label, message = m, "the task board: made a new one");
            self.record_board(c, m, false);
            self.pin(c, m).await;
        }
        Ok(())
    }

    /// After a restart, find this channel's board once a run and keep it in
    /// `msgs`; Err when Discord could not say yet (the write waits).
    pub(super) async fn take_up_board(&mut self, channel: u64, v2: bool) -> Result<(), SendErr> {
        if self.msgs.contains_key(BOARD_KEY) || self.board_sought {
            return Ok(());
        }
        let found = match self.recorded_board(channel, Some(v2)).await? {
            Some(m) => {
                tracing::info!(place = %self.label, message = m, "the task board: found the one it made, edited in place");
                Some(m)
            }
            None => {
                let pinned = if v2 {
                    self.find_board_v2(channel).await
                } else {
                    self.find_board(channel).await
                };
                if let Some(m) = pinned {
                    tracing::info!(place = %self.label, message = m, "the task board: found the pinned one, edited in place");
                }
                pinned
            }
        };
        self.board_sought = true;
        if let Some(m) = found {
            self.msgs.insert(BOARD_KEY.into(), (channel, m));
        }
        Ok(())
    }

    /// The board this channel has, by the id recorded when it was made: that
    /// message, if it is still there and the bot's (of kind `v2` when given).
    /// Gone is no board; Discord away is an Err, not an answer.
    pub(super) async fn recorded_board(
        &self,
        channel: u64,
        v2: Option<bool>,
    ) -> Result<Option<u64>, SendErr> {
        let store = &self.shared.core.store;
        let Ok(Some(rec)) = store.get_meta::<Recorded>(&record_key(channel)) else {
            return Ok(None);
        };
        if v2.is_some_and(|v| v != rec.v2) {
            return Ok(None);
        }
        let got = self
            .shared
            .http
            .message(Id::new(channel), Id::new(rec.message))
            .await;
        match got {
            Ok(r) => match r.model().await {
                Ok(m) if m.author.bot => Ok(Some(rec.message)),
                _ => Ok(None),
            },
            Err(e) => {
                let err = SendErr::of(&e);
                if err.gone {
                    tracing::info!(place = %self.label, message = rec.message, "the task board: the one it made is gone");
                    return Ok(None);
                }
                tracing::info!(place = %self.label, error = %e, "the task board: its message could not be read");
                if err.away {
                    return Err(err);
                }
                Ok(None)
            }
        }
    }

    /// Keep where the board is, so a restart finds it pinned or not.
    pub(super) fn record_board(&self, channel: u64, message: u64, v2: bool) {
        let put = self
            .shared
            .core
            .store
            .put_meta(&record_key(channel), &Recorded { message, v2 });
        if let Err(e) = put {
            tracing::info!(place = %self.label, error = %e, "the task board: its place was not recorded");
        }
    }

    /// The board is drawn elsewhere now: forget this lane's, and any state
    /// of it waiting, and remove the message at the next turn of the loop. A
    /// lane with no board of its own looks among the pins for one an earlier
    /// run drew (its id went with that process).
    pub(super) fn board_moved(&mut self) {
        self.live.retain(|o| o.key() != Some(BOARD_KEY));
        self.sent.remove(BOARD_KEY);
        self.stale_board = self.msgs.remove(BOARD_KEY);
        self.seek_stale = self.stale_board.is_none() && !self.board_sought;
    }

    /// Delete the board `board_moved` set aside, or the pinned one of an
    /// earlier run; a message already gone is done.
    pub(super) async fn drop_stale_board(&mut self) {
        let stale = match self.stale_board.take() {
            Some(b) => Some(b),
            None if std::mem::take(&mut self.seek_stale) => self.earlier_board().await,
            None => None,
        };
        let Some((channel, message)) = stale else {
            return;
        };
        let sent = self
            .shared
            .http
            .delete_message(Id::new(channel), Id::new(message))
            .await;
        if let Err(e) = sent {
            if SendErr::of(&e).gone {
                return;
            }
            tracing::info!(place = %self.label, error = %e, "the old task board was not removed");
        }
    }

    /// The bot's pinned board, V2 or text, that this lane did not make.
    async fn earlier_board(&mut self) -> Option<(u64, u64)> {
        let channel = self.place_channel().await.ok()?;
        let found = match self.recorded_board(channel, None).await {
            Ok(Some(m)) => Some(m),
            _ => match self.find_board_v2(channel).await {
                Some(m) => Some(m),
                None => self.find_board(channel).await,
            },
        };
        found.map(|m| (channel, m))
    }

    /// The bot's pinned board in `channel`, if it has one.
    async fn find_board(&self, channel: u64) -> Option<u64> {
        let pins = match self.shared.http.pins(Id::new(channel)).await {
            Ok(r) => r.model().await.ok()?,
            Err(e) => {
                tracing::info!(place = %self.label, error = %e, "the task board: the pins could not be read");
                return None;
            }
        };
        pins.items
            .iter()
            .find(|p| p.message.author.bot && p.message.content.starts_with(BOARD_HEAD))
            .map(|p| p.message.id.get())
    }

    /// Pin the board, best-effort: a refusal is logged once a lane.
    pub(super) async fn pin(&mut self, channel: u64, message: u64) {
        match self
            .shared
            .http
            .create_pin(Id::new(channel), Id::new(message))
            .await
        {
            Ok(_) => tracing::debug!(place = %self.label, message, "the task board pinned"),
            Err(e) if !self.pin_refused => {
                self.pin_refused = true;
                tracing::info!(place = %self.label, error = %e, "the task board was not pinned (the bot may not pin here); it is still edited in place");
            }
            Err(_) => {}
        }
    }
}
