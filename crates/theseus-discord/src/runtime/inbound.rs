//! A message for a turn, and its attachments downloading (a child of
//! `runtime`, apart from it for the shape budget's file ceiling).

use theseus_protocol::{Attachment, DiscordOrigin};
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker};
use twilight_model::id::Id;

use crate::files;

/// One message for a turn: who wrote it, what it says, its files (still
/// downloading, theseus-9g2), its Discord id, and where it was written.
pub(super) struct Inbound {
    pub(super) author: String,
    pub(super) author_id: u64,
    pub(super) text: String,
    pub(super) files: Option<Pending>,
    pub(super) message: Id<MessageMarker>,
    pub(super) channel: Id<ChannelMarker>,
    /// None in a DM.
    pub(super) guild: Option<Id<GuildMarker>>,
    /// The thread the message addresses when it is spoken in the owner's DM
    /// (a reply to a message that carries a thread's tag, or `thread:<slug>`
    /// first, theseus-sjzm): the turn runs in that thread and is answered
    /// here.
    pub(super) thread: Option<String>,
    /// The turn whose reply this message replies to (theseus-rha2): the
    /// router places the message in that turn's thread, certain.
    pub(super) reply_turn: Option<String>,
    /// The task whose Follow mirror this message replies to
    /// (theseus-esxyb): the owner's word to that task.
    pub(super) task: Option<String>,
}

impl Inbound {
    /// Who wrote it and where, as the core judges an answer (a typed
    /// `/trust`).
    pub(super) fn origin(&self) -> DiscordOrigin {
        DiscordOrigin {
            user_id: self.author_id.to_string(),
            channel_id: self.channel.to_string(),
            guild_id: self.guild.map(|g| g.to_string()),
        }
    }
}

/// A message's attachments, downloading in their own task: the gateway loop
/// awaits each message's handler, so it never waits on a download.
pub(super) struct Pending {
    pub(super) metas: Vec<files::FileMeta>,
    pub(super) task: tokio::task::JoinHandle<Vec<Attachment>>,
}

impl Pending {
    /// The entries for `turn.submit`. A task that never reported back lists
    /// its files as failed downloads; the message itself still goes.
    pub(super) async fn wait(self) -> Vec<Attachment> {
        match self.task.await {
            Ok(v) => v,
            Err(e) => files::lost(&self.metas, &e.to_string()),
        }
    }
}
