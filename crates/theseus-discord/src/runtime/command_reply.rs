//! The one way a slash command's reply leaves (theseus-qtofh): the place's
//! words carry the vault's values replaced by their names (`Exits::text`, as the
//! courier's do), ping nobody they name (`allowed_mentions` empty), and are cut
//! to Discord's limit. `/why`, `/digest`, `/prompt` and every command
//! the general arm answers come through `Shared::finish_command`; a new command
//! that edits in its reply does the same, so the three rules are kept in one
//! place and not once per command.
//!
//! The three paths that answer without an edit (theseus-emqbm) share the first
//! and last rules through `leaving`, since `finish_command` is an edit of the
//! original reply and none of them is: `Shared::followup` makes a new ephemeral
//! message, and `Shared::respond` and `Shared::say_ephemeral` are the
//! interaction's first answer, before there is a reply to edit. Their
//! `allowed_mentions` were always empty.

use theseus_core::exits::Exits;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::AllowedMentions;

use super::Shared;
use crate::render::clip;

/// Discord's limit on a message's text.
const LIMIT: usize = 2000;

/// `text` as a reply may leave: the vault's values replaced first, then the cut,
/// so a value that straddles the limit leaves no piece of itself behind.
pub(super) fn leaving(exits: &Exits, text: &str) -> String {
    clip(&exits.text(text).0, LIMIT)
}

impl Shared {
    /// Edit `text` into the reply to `i` (already acknowledged). `what` names
    /// the command in the board's error when Discord refuses it.
    pub(super) async fn finish_command(&self, i: &Interaction, text: &str, what: &str) {
        let content = leaving(&self.core.exits, text);
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&content))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error(what, None, e);
        }
    }
}
