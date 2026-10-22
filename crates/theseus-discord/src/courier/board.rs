//! The task board on its place's lane (M7 step 39b, theseus-ext.14): a live
//! upsert under `render::BOARD_KEY`, never replayed, its message id in
//! memory. A child of `courier`.
//!
//! - **Made once, pinned best-effort.** Its first write creates it, and the
//!   lane pins it when the bot may; a refused pin is logged once a lane.
//! - **After a restart** the id is gone with the process. The lane's first
//!   board write looks for it among the channel's pins (the bot's message that
//!   begins with `render::BOARD_HEAD`) and edits that one, or makes a new one,
//!   and says which. Never on the start path: only at the place's first
//!   change after it.

use twilight_model::id::Id;

use super::{Lane, SendErr, Write};
use crate::render::{Buttons, BOARD_HEAD, BOARD_KEY};

impl Lane {
    /// Write the board's latest state.
    pub(super) async fn board(&mut self, content: String) -> Result<(), SendErr> {
        let channel = self.place_channel().await?;
        if !self.msgs.contains_key(BOARD_KEY) && !self.board_sought {
            self.board_sought = true;
            if let Some(m) = self.find_board(channel).await {
                tracing::info!(place = %self.label, message = m, "the task board: found the pinned one, edited in place");
                self.msgs.insert(BOARD_KEY.into(), (channel, m));
            }
        }
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
            self.pin(c, m).await;
        }
        Ok(())
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
        let found = match self.find_board_v2(channel).await {
            Some(m) => Some(m),
            None => self.find_board(channel).await,
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
