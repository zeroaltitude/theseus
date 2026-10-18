//! The war room (theseus-bh3t.4): the owner brings a guest into a private
//! task's place to watch and comment, and only the owner steers.
//!
//! - **Share.** `/task name:<task> share:<@user>` (or `unshare:`) is the
//!   owner's alone, from a private place. A task with its own text channel
//!   gives the guest a member overwrite (view, send, read history); a task
//!   thread adds the guest as a thread member, when they can already see the
//!   thread's parent (a thread is as visible as its channel, and an add cannot
//!   widen that). The guests are kept per task in META `discord.task_guests.<task>`.
//! - **At the end** the place is the owner's alone: the channel's close replaces
//!   every overwrite (`task_channels.rs`), a thread's guests are removed while
//!   it is still open (`end_guests`; Discord keeps an archived thread's members
//!   as they are), the list is cleared, and a `share:` is refused.
//! - **A comment.** A non-owner's message in a private task's place is never a
//!   `turn.submit`, a steer or a redirect, shared explicitly or not: it is kept
//!   (META `discord.task_comments.<task>`, the last 50, with a running total),
//!   relayed once to the owner as a quiet line in the parent place, listed in
//!   the task's Details, and counted on its report. Guest text is untrusted: it
//!   reaches no session, no board line and no ledger row (a row has `chars`,
//!   not text). A shared place's thread keeps the guest rule of
//!   `Place::join_task`: its guests take part in a shared conversation.

use serde::{Deserialize, Serialize};
use serde_json::json;
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::http::permission_overwrite::{PermissionOverwrite, PermissionOverwriteType};
use twilight_model::id::marker::UserMarker;
use twilight_model::id::Id;

use super::inbound::Inbound;
use super::threads::{Made, PlaceKind};
use super::{Place, Shared};
use crate::render::clip;
use theseus_protocol::LedgerKind;

/// How many comments a task keeps (the total goes on counting).
const KEPT: usize = 50;
/// A kept comment's longest text, in characters.
const TEXT_MAX: usize = 500;
/// The relay line quotes this many characters.
const QUOTE: usize = 80;

/// What a guest may do in a task's channel.
const GUEST: Permissions = Permissions::VIEW_CHANNEL
    .union(Permissions::SEND_MESSAGES)
    .union(Permissions::READ_MESSAGE_HISTORY);

/// What `/task share:` or `unshare:` asks.
#[derive(Clone, Copy)]
pub(crate) enum Guest {
    Add(Id<UserMarker>),
    Remove(Id<UserMarker>),
}

/// A guest's message, kept for the owner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Comment {
    pub author: u64,
    pub name: String,
    pub at_ms: u64,
    pub text: String,
    pub message: u64,
}

/// A task's comments: how many there have been, and the latest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Comments {
    pub total: usize,
    pub last: Vec<Comment>,
}

fn comments_key(task: &str) -> String {
    format!("discord.task_comments.{task}")
}

fn guests_key(task: &str) -> String {
    format!("discord.task_guests.{task}")
}

/// A comment's words on one line, for a notice.
fn one_line(text: &str, max: usize) -> String {
    clip(&text.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

impl Shared {
    /// `user` is an owner (the places' `owners`).
    pub(super) fn is_owner(&self, user: u64) -> bool {
        self.core
            .runner
            .place_rule
            .owners(&self.core.cfg)
            .contains(&format!("discord:{user}"))
    }

    /// The comments kept for a task.
    pub(crate) fn comments_of(&self, task: &str) -> Comments {
        let got = self.core.store.get_meta(&comments_key(task));
        got.ok().flatten().unwrap_or_default()
    }

    /// How many comments the task of `session` has (a report names its session).
    pub(crate) fn comments_on_session(&self, session: &str) -> usize {
        let made = self.task_threads.all();
        let made = made.iter().find(|m| m.session == session);
        made.map_or(0, |m| self.comments_of(&m.task).total)
    }

    /// The people the owner shared the task with.
    fn guests_of(&self, task: &str) -> Vec<u64> {
        let got = self.core.store.get_meta(&guests_key(task));
        got.ok().flatten().unwrap_or_default()
    }

    fn keep<T: Serialize>(&self, key: &str, value: &T) {
        if let Err(e) = self.core.store.put_meta(key, value) {
            self.board.error("war room", None, e);
        }
    }
}

impl Shared {
    /// The task ended: its guests go. A channel's close has replaced every
    /// overwrite (the caller says so by calling this), so the list is
    /// cleared; a thread's guests are removed from it, which Discord allows
    /// only while it is open, and one it refuses stays listed for `unshare:`.
    pub(super) async fn end_guests(&self, made: &Made) {
        let guests = self.guests_of(&made.task);
        if guests.is_empty() {
            return;
        }
        let mut left = Vec::new();
        for g in guests {
            if made.kind == PlaceKind::Thread {
                let gone = self
                    .http
                    .remove_thread_member(Id::new(made.thread), Id::new(g))
                    .await;
                if let Err(e) = gone {
                    self.board.error("war room", Some(&made.session), e);
                    left.push(g);
                }
            }
        }
        self.keep(&guests_key(&made.task), &left);
    }
}

impl Place {
    /// A non-owner's message in a private task's place: a comment, which no
    /// session ever reads. True when it was one.
    pub(super) fn comment_on(&mut self, m: &mut Inbound, made: &Made, title: &str) -> bool {
        let s = self.shared.clone();
        if !made.private || s.is_owner(m.author_id) {
            return false;
        }
        self.last_activity_ms = theseus_protocol::now_unix_ms();
        // Its files are not downloaded for anyone.
        if let Some(p) = m.files.take() {
            p.task.abort();
        }
        s.core.binding_ledger(
            LedgerKind::DiscordMessageIn,
            Some(&self.session_id),
            json!({"place": self.label, "author": m.author, "chars": m.text.chars().count(),
                   "message_id": m.message.to_string(), "task_thread": true, "comment": true}),
        );
        let mut kept = s.comments_of(&made.task);
        let first = !kept.last.iter().any(|c| c.author == m.author_id);
        kept.total += 1;
        kept.last.push(Comment {
            author: m.author_id,
            name: m.author.clone(),
            at_ms: theseus_protocol::now_unix_ms(),
            text: clip(&m.text, TEXT_MAX),
            message: m.message.get(),
        });
        let over = kept.last.len().saturating_sub(KEPT);
        kept.last.drain(..over);
        s.keep(&comments_key(&made.task), &kept);
        let link = match made.guild {
            Some(g) => format!(
                " · https://discord.com/channels/{g}/{}/{}",
                m.channel, m.message
            ),
            None => String::new(),
        };
        s.say_to(
            &format!("discord:channel:{}", made.parent),
            format!(
                "-# 💬 {} on “{}”: {}{link}",
                one_line(&m.author, 40),
                one_line(title, 40),
                one_line(&m.text, QUOTE),
            ),
            &made.parent_session,
        );
        if first {
            s.say_to(
                &self.target,
                "-# The task doesn't read guests' messages; the owner sees them.".into(),
                &made.session,
            );
        }
        true
    }

    /// `/task name: share:<@user>` or `unshare:`, an ephemeral line back.
    pub(super) async fn share_task(
        &self,
        i: &twilight_model::application::interaction::Interaction,
        name: &str,
        guest: Option<Guest>,
    ) {
        let s = &self.shared;
        let line = match guest {
            None => "Give `share` or `unshare`, not both.".to_string(),
            Some(g) => self.share(i.author_id().map(Id::get), name, g).await,
        };
        s.tell(i, line).await;
    }

    async fn share(&self, by: Option<u64>, name: &str, guest: Guest) -> String {
        let s = &self.shared;
        if !by.is_some_and(|u| s.is_owner(u)) {
            return "🔐 Only the owner shares a task.".into();
        }
        if !s.is_private(&self.target) {
            return "🔐 A task is shared from a private place: your DM with me, or a channel \
                    bound private."
                .into();
        }
        let Some(v) = s.tasks.find_task(&self.session_id, name) else {
            return format!("No single task of this place matches `{name}`: pick one as you type.");
        };
        let Some(made) = s.task_threads.all().into_iter().find(|m| m.task == v.id) else {
            return "That task has no thread or channel to share yet.".into();
        };
        if made.ended_ms.is_some() && matches!(guest, Guest::Add(_)) {
            return "That task has ended; its place is yours alone.".into();
        }
        match s.set_guest(&made, guest).await {
            Ok(line) | Err(line) => line,
        }
    }
}

impl Shared {
    /// Give or take a guest's access to the task's place, whatever the place
    /// is (a channel's overwrite, a thread's member).
    pub(crate) async fn set_guest(&self, made: &Made, guest: Guest) -> Result<String, String> {
        let hurt = |e: &dyn std::fmt::Display| format!("⚠️ Discord refused: {e}");
        let place = Id::new(made.thread);
        let ch = self.http.channel(place).await.map_err(|e| hurt(&e))?;
        let ch = ch.model().await.map_err(|e| hurt(&e))?;
        let (user, add) = match guest {
            Guest::Add(u) => (u, true),
            Guest::Remove(u) => (u, false),
        };
        let said = match ch.kind {
            ChannelType::GuildText => {
                if add {
                    let o = PermissionOverwrite {
                        allow: Some(GUEST),
                        deny: None,
                        id: user.cast(),
                        kind: PermissionOverwriteType::Member,
                    };
                    self.http.update_channel_permission(place, &o).await
                } else {
                    self.http
                        .delete_channel_permission(place)
                        .member(user)
                        .await
                }
                .map_err(|e| hurt(&e))?;
                String::new()
            }
            ChannelType::PublicThread | ChannelType::PrivateThread => {
                let parent = made.parent;
                if add {
                    let seen = self.view(made.parent).await.map_err(|e| {
                        format!("⚠️ I can't read who sees <#{parent}>, so I can't share its thread: {e:#}")
                    })?;
                    if !seen.viewers.iter().any(|v| v.id == user.get()) {
                        return Err(format!(
                            "<@{user}> can't see <#{parent}>, so they can't see its threads; a \
                             task with its own channel can be shared"
                        ));
                    }
                    self.http.add_thread_member(place, user).await
                } else {
                    self.http.remove_thread_member(place, user).await
                }
                .map_err(|e| hurt(&e))?;
                if add {
                    String::new()
                } else {
                    " A thread stays visible to everyone who sees its channel.".into()
                }
            }
            _ => return Err("⚠️ That task's place is neither a text channel nor a thread.".into()),
        };
        let mut guests = self.guests_of(&made.task);
        guests.retain(|g| *g != user.get());
        if add {
            guests.push(user.get());
        }
        self.keep(&guests_key(&made.task), &guests);
        Ok(if add {
            format!(
                "✅ <@{user}> can watch and comment in <#{}>. Their messages are comments: the \
                 task never reads them, and you see them in <#{}>.",
                made.thread, made.parent
            )
        } else {
            format!("✅ <@{user}> is out of <#{}>.{said}", made.thread)
        })
    }
}
