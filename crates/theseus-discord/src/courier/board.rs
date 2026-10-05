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
            })
            .await?;
        if let (true, Some((c, m))) = (made, at) {
            tracing::info!(place = %self.label, message = m, "the task board: made a new one");
            self.pin(c, m).await;
        }
        Ok(())
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
    async fn pin(&mut self, channel: u64, message: u64) {
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
