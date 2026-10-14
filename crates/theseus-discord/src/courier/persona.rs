//! A persona's reply, signed (theseus-sjzm). When a thread has a persona,
//! the text of its replies in the place that runs the thread is signed with
//! the persona's name: through a channel webhook sent under that name, where
//! the bot may manage webhooks in the channel, else a bold name at the head
//! of the text. A DM has no webhooks, so its signature is always the bold
//! name. A webhook's messages are kept in `Replies` like the bot's own, so a
//! reply to one is found, and the lane edits them through the webhook, which
//! alone may. A child of `courier`.

use std::collections::HashMap;

use twilight_model::channel::message::MessageFlags;
use twilight_model::id::marker::WebhookMarker;
use twilight_model::id::Id;

use super::{Lane, SendErr, Write};
use crate::policy;

/// The webhook a lane signs through, and the thread to name when the place
/// is a Discord thread (whose webhook is its parent channel's).
#[derive(Debug, Clone)]
pub(super) struct Hook {
    id: u64,
    token: String,
    thread: Option<u64>,
}

/// What a lane remembers of signing.
#[derive(Default)]
pub(super) struct Signing {
    /// The webhook the lane found or made; `Some(None)` is none, asked once:
    /// the bot may not manage webhooks here.
    hook: Option<Option<Hook>>,
    /// The messages a webhook wrote, with the webhook.
    hooked: HashMap<u64, Hook>,
}

/// A write, signed: its text, the webhook to send it through if there is
/// one, and the name it is sent under.
pub(super) struct Signed {
    pub(super) write: Write,
    pub(super) hook: Option<Hook>,
    pub(super) name: String,
}

/// A name at the head of a text, bold.
pub(crate) fn signed_text(name: &str, text: &str) -> String {
    format!("**{name}**: {text}")
}

/// A reply's parts, each loop's first text part signed with a bold name: a
/// DM's, which has no webhooks.
pub(super) fn sign_parts(parts: &mut [(String, String)], name: &str) {
    for (key, text) in parts.iter_mut() {
        if policy::is_text_part(key) && first_of_loop(key) {
            *text = signed_text(name, text);
        }
    }
}

/// Whether a text part is the first of its loop (`…:L<n>:p0`): the one that
/// carries a bold signature.
fn first_of_loop(key: &str) -> bool {
    key.rsplit(':').next() == Some("p0")
}

impl Lane {
    /// The persona of the thread this lane's place runs, if it runs one and
    /// the thread has one.
    fn persona(&self) -> Option<String> {
        let slug = self.shared.threads.slug_of(self.channel?)?;
        self.shared.core.threads().get(&slug)?.persona.clone()
    }

    /// `w` signed with the persona's name, when it is a text part of a place
    /// that runs a thread whose persona is set.
    pub(super) async fn sign(&mut self, w: &Write) -> Option<Signed> {
        if !policy::is_text_part(&w.key) {
            return None;
        }
        let name = self.persona()?;
        if self.kind == "channel" {
            if let Some(hook) = self.hook_for(w.channel).await {
                return Some(Signed {
                    write: w.clone(),
                    hook: Some(hook),
                    name,
                });
            }
        }
        // The bold name: on the first text part of each loop; a later part
        // is the same voice.
        let content = match first_of_loop(&w.key) {
            true => signed_text(&name, &w.content),
            false => w.content.clone(),
        };
        Some(Signed {
            write: Write {
                content,
                ..w.clone()
            },
            hook: None,
            name,
        })
    }

    /// The webhook to sign through in `channel`, found among the channel's or
    /// made, once; none when the bot may not.
    async fn hook_for(&mut self, channel: u64) -> Option<Hook> {
        if let Some(h) = &self.signing.hook {
            return h.clone();
        }
        let found = self.find_hook(channel).await;
        self.signing.hook = Some(found.clone());
        found
    }

    async fn find_hook(&self, channel: u64) -> Option<Hook> {
        let http = &self.shared.http;
        // A Discord thread's webhook is its parent channel's.
        let ch = http
            .channel(Id::new(channel))
            .await
            .ok()?
            .model()
            .await
            .ok()?;
        let (home, thread) = match (ch.kind.is_thread(), ch.parent_id) {
            (true, Some(p)) => (p.get(), Some(channel)),
            _ => (channel, None),
        };
        let listed = http
            .channel_webhooks(Id::new(home))
            .await
            .ok()?
            .models()
            .await
            .ok()?;
        let ours = listed
            .into_iter()
            .find(|h| h.token.is_some() && h.name.as_deref() == Some(HOOK_NAME));
        let hook = match ours {
            Some(h) => h,
            None => http
                .create_webhook(Id::new(home), HOOK_NAME)
                .await
                .ok()?
                .model()
                .await
                .ok()?,
        };
        Some(Hook {
            id: hook.id.get(),
            token: hook.token?,
            thread,
        })
    }

    /// Create `w` through the webhook, under the persona's name; its message
    /// is remembered as the webhook's.
    pub(super) async fn create_hooked(
        &mut self,
        hook: &Hook,
        name: &str,
        w: &Write,
        ping: bool,
    ) -> Result<(u64, String), SendErr> {
        let id: Id<WebhookMarker> = Id::new(hook.id);
        let none = twilight_model::channel::message::AllowedMentions::default();
        let mut req = self
            .shared
            .http
            .execute_webhook(id, &hook.token)
            .content(&w.content)
            .username(name)
            .allowed_mentions(Some(&none));
        if let Some(t) = hook.thread {
            req = req.thread_id(Id::new(t));
        }
        if !ping {
            req = req.flags(MessageFlags::SUPPRESS_NOTIFICATIONS);
        }
        let m = req
            .wait()
            .await
            .map_err(|e| SendErr::of(&e))?
            .model()
            .await
            .map_err(|e| SendErr {
                away: true,
                unsure: true,
                gone: false,
                message: format!("reading the sent message: {e}"),
            })?;
        self.signing.hooked.insert(m.id.get(), hook.clone());
        self.shared.board.update(|s| s.messages_out += 1);
        Ok((m.id.get(), m.content))
    }

    /// Edit a message the webhook wrote, through it.
    pub(super) async fn edit_hooked(
        &self,
        message: u64,
        content: &str,
    ) -> Option<Result<(), SendErr>> {
        let hook = self.signing.hooked.get(&message)?;
        let none = twilight_model::channel::message::AllowedMentions::default();
        let mut req = self
            .shared
            .http
            .update_webhook_message(Id::new(hook.id), &hook.token, Id::new(message))
            .content(Some(content))
            .allowed_mentions(Some(&none));
        if let Some(t) = hook.thread {
            req = req.thread_id(Id::new(t));
        }
        let r = req.await.map(|_| ()).map_err(|e| SendErr::of(&e));
        if r.is_ok() {
            self.shared.board.update(|s| s.edits += 1);
        }
        Some(r)
    }
}

/// The name of the webhook the binding makes in a channel.
const HOOK_NAME: &str = "Theseus";
