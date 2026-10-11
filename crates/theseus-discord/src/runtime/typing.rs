//! A person's typing (theseus-tnky): Discord's typing event, told to the core
//! as `session.typing` so it warms what the message will wait for (the
//! provider's connection, the index's model). A child of `runtime`, apart
//! from it for the shape budget's file ceiling.
//!
//! The gateway loop hands each event to [`Shared::on_typing`], which does
//! almost nothing: it drops what is no place of ours, a place that wants an
//! @mention first, a person the place does not let drive it, and the bot
//! itself, and tells each person in each channel once per idle spell
//! ([`theseus_protocol::warm::Typist`]). The place's own actor sends the
//! notice with its session's id, since only it knows it. The core refuses
//! anyone but an owner (`Core::typist_refused`), so another person's typing in
//! a shared place warms nothing even from a binding that told it.

use std::sync::Arc;

use theseus_protocol::warm::{SessionTypingParams, SessionTypingResult};
use theseus_protocol::{method, DiscordOrigin};
use twilight_model::gateway::payload::incoming::TypingStart;

use super::{PlaceMsg, Shared};
use crate::rpc_client::RpcClient;

impl Shared {
    /// One typing event: a notice to the place's actor, or nothing.
    pub(super) fn on_typing(self: &Arc<Self>, t: &TypingStart) {
        if t.user_id.get() == self.bot_id() {
            return;
        }
        let tx = {
            let mut r = self.routes.lock().unwrap();
            let Some(found) = r.resolve(
                Some(t.channel_id.get()),
                t.guild_id.is_some(),
                Some(t.user_id.get()),
            ) else {
                return;
            };
            // A shared channel that wants an @mention first: the typing is
            // not known to be for Theseus.
            if !found.allowed || found.mention_only {
                return;
            }
            // Per person as well as per channel: another person's typing in
            // a shared channel must not use up the owner's spell.
            let key = format!("{}:{}", t.channel_id, t.user_id);
            if !r.typist.keystroke(&key, theseus_protocol::now_unix_ms()) {
                return;
            }
            found.place
        };
        let _ = tx.send(PlaceMsg::Typing(DiscordOrigin {
            user_id: t.user_id.to_string(),
            channel_id: t.channel_id.to_string(),
            guild_id: t.guild_id.map(|g| g.to_string()),
        }));
    }
}

/// The notice, on the place's behalf, never waited for: its answer says only
/// whether the core took it.
pub(super) fn notice(rpc: &Arc<RpcClient>, session_id: &str, origin: DiscordOrigin) {
    let rpc = Arc::clone(rpc);
    let session_id = Some(session_id.to_string()).filter(|s| !s.is_empty());
    tokio::spawn(async move {
        let params = SessionTypingParams {
            session_id,
            author: None,
            discord: Some(origin),
        };
        let _ = rpc
            .call::<_, SessionTypingResult>(method::SESSION_TYPING, params)
            .await;
    });
}
