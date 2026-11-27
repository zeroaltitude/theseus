//! The owner's own press of a control in a private place on Discord (issue 59): the one judge a
//! sign-in's methods ask beside the CLI. The presser is the one the Discord binding read off the
//! press, judged by `places::owner_in_private` as every press is (an owner, in a direct message or
//! a channel bound private). A turn never presses: nothing a turn calls reaches these methods.

use theseus_protocol::DiscordOrigin;

use super::server::Conn;
use crate::approval::Surface;

impl crate::Core {
    /// `None` when this is not a Discord press that names its presser (the caller's own rule
    /// answers then); else whether it is the owner's own, in a private place, and why not.
    pub(super) fn owner_press(
        &self,
        conn: &Conn<'_>,
        author: Option<String>,
        discord: Option<DiscordOrigin>,
    ) -> Option<Result<(), String>> {
        if conn.surface != Surface::Discord || discord.is_none() {
            return None;
        }
        let who = conn.answerer(author, discord);
        Some(crate::places::owner_in_private(
            &who,
            &self.runner.place_rule,
            &self.cfg,
        ))
    }
}
