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

use std::future::Future;
use std::time::Duration;

use theseus_core::exits::Exits;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};

use super::Shared;
use crate::render::clip;

/// Discord's limit on a message's text.
const LIMIT: usize = 2000;

/// `text` as a reply may leave: the vault's values replaced first, then the cut,
/// so a value that straddles the limit leaves no piece of itself behind.
pub(super) fn leaving(exits: &Exits, text: &str) -> String {
    clip(&exits.text(text).0, LIMIT)
}

/// How long a handler waits for the daemon after it has acknowledged an interaction: Discord lets the
/// response be completed for fifteen minutes, so the wait ends a minute inside that.
pub(super) const FOLLOW_UP_WINDOW: Duration = Duration::from_secs(14 * 60);

/// How long a handler that must answer with a modal (which cannot follow an acknowledgement) or an autocomplete
/// waits for the daemon: inside the service's three seconds, with room to send the response.
pub(super) const ACK_DEADLINE: Duration = Duration::from_millis(2_500);

/// What such a handler says when the daemon did not answer in time: nothing was done, and trying again is safe.
pub(super) const TOO_SLOW: &str = "⏳ Theseus was slow to answer. Nothing was done: try again.";

/// `work`, or `None` when [`ACK_DEADLINE`] ends first.
pub(super) async fn within_ack<T>(work: impl Future<Output = T>) -> Option<T> {
    tokio::time::timeout(ACK_DEADLINE, work).await.ok()
}

/// `work`, or `None` when the follow-up window ends first. The daemon's request has been sent and is not
/// cancelled: only the wait for its answer ends.
pub(super) async fn within_follow_up<T>(work: impl Future<Output = T>) -> Option<T> {
    tokio::time::timeout(FOLLOW_UP_WINDOW, work).await.ok()
}

impl Shared {
    /// Acknowledge an interaction at once, for a response that is completed later (Discord's first response is
    /// due within three seconds: a handler that waits for the daemon first can miss it, and the person sees
    /// "Something went wrong" whatever it then says). The acknowledgement carries nothing: the presser alone
    /// sees "thinking", and the response is then completed with `finish_command`.
    pub(super) async fn ack_ephemeral(&self, i: &Interaction) {
        let resp = InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(InteractionResponseData {
                flags: Some(MessageFlags::EPHEMERAL),
                ..Default::default()
            }),
        };
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("interaction response", None, e);
        }
    }

    /// Acknowledge a press on a message at once, for an edit of that message (or a follow-up to the presser)
    /// that is made later.
    pub(super) async fn ack_update(&self, i: &Interaction) {
        self.respond(
            i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
    }

    /// A message of its own, public, after an acknowledgement that was private: a follow-up with no flag is seen by
    /// everyone in the place, whatever the acknowledgement was. For the one line a place is meant to see (a prompt
    /// that runs there), never for an answer that was private.
    pub(super) async fn followup_public(&self, i: &Interaction, text: &str) {
        let content = leaving(&self.core.exits, text);
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .create_followup(&i.token)
            .content(&content)
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error("interaction follow-up", None, e);
        }
    }

    /// Complete an acknowledged reply (`ack_ephemeral`) with words and buttons.
    pub(super) async fn finish_data(
        &self,
        i: &Interaction,
        data: InteractionResponseData,
        what: &str,
    ) {
        let content = leaving(&self.core.exits, data.content.as_deref().unwrap_or(""));
        let components = data.components.unwrap_or_default();
        if let Err(e) = self
            .interact
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&content))
            .components(Some(&components))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error(what, None, e);
        }
    }

    /// Complete an acknowledged press on a message (`ack_update`): its words become `content` and its buttons
    /// go, when it was answered; `what` names the handler in the board's error when the service refuses.
    ///
    /// One writer per card: this is for a card **only the press writes**. A card the daemon settles itself
    /// (`settled_by_daemon`) is written by its settle alone, and its press writes nothing.
    pub(super) async fn edit_pressed(&self, i: &Interaction, content: &str, what: &str) {
        let content = leaving(&self.core.exits, content);
        let nobody = AllowedMentions::default();
        let client = self.interact.interaction(i.application_id);
        let update = client
            .update_response(&i.token)
            .content(Some(&content))
            .components(Some(&[]))
            .allowed_mentions(Some(&nobody));
        if let Err(e) = update.await {
            self.board.error(what, None, e);
        }
    }

    /// Whether a counted answer to `question` leaves its card to the daemon: a card or a needs-you post of the
    /// outbox is settled by the settle post the answer writes (its words are the card's last), so the press that
    /// answered it has acknowledged and writes nothing to the card, not even its buttons: a V2 card keeps its
    /// words in its components, and clearing them re-sends the words the press read. A card from before the outbox
    /// has no settle, and its press is the one writer.
    pub(super) fn settled_by_daemon(&self, question: &str) -> bool {
        self.core.outbox.has_post(question)
    }

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
