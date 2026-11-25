//! What a place's actor is told first: a task thread's message, an index
//! card's, a reply to a task's mirror, or the usual; and the quiet notices a
//! task's thread and its place post. A child of `runtime`, apart from it for
//! the shape budget's file ceiling.

use serde_json::json;

use super::{Place, PlaceMsg, Shared};

impl Shared {
    /// A quiet notice in `channel`'s place: the lane writes it, in order with
    /// the place's other posts, and it pings no one.
    pub(super) fn say_pointer(&self, channel: u64, text: String, from: &str) -> Option<String> {
        self.say_to(&format!("discord:channel:{channel}"), text, from)
    }

    /// A quiet notice in the place `target` (a channel's, a thread's, a DM's).
    /// The post's id, whose message lands under the key `note:<id>`.
    pub(super) fn say_to(&self, target: &str, text: String, from: &str) -> Option<String> {
        let body = json!({"kind": "notice", "text": text, "quiet": true});
        match self.core.outbox.post(from, "", target, body) {
            Ok(a) => Some(a.correlation_id),
            Err(e) => {
                self.board.error("post pointer", None, format!("{e:#}"));
                None
            }
        }
    }
}

impl Place {
    /// A message for the place's actor: a task thread's, or the usual.
    pub(super) async fn handle_in(&mut self, m: PlaceMsg) {
        match m {
            PlaceMsg::Inbound(i) if self.shared.task_threads.is_thread(i.channel.get()) => {
                self.task_message(i);
            }
            // An index card in the default forum, and the task home, start nothing.
            PlaceMsg::Inbound(i)
                if self.shared.task_posts.index_of(i.channel.get()).is_some()
                    || self.shared.task_posts.home.is_home(i.channel.get()) =>
            {
                self.index_pointer(&i);
            }
            // A reply to a Follow mirror of a task that has a session joins it.
            PlaceMsg::Inbound(i) if self.mirrors_a_task(&i) => self.task_reply(i),
            // A task's thread is the task's own conversation: `/new` would
            // take it off the task (theseus-i3do6).
            PlaceMsg::Control {
                cmd: super::Control::New,
                reply,
                ..
            } if self
                .channel
                .is_some_and(|c| self.shared.task_threads.is_thread(c.get())) =>
            {
                let _ = reply.send(
                    "A task's thread is the task's own: it cannot start a new conversation.".into(),
                );
            }
            PlaceMsg::Fold(ask) => self.fold_asked(*ask),
            PlaceMsg::FoldShow(ask) => self.fold_show(*ask),
            m => self.handle(m).await,
        }
    }
}
