//! One Discord thread per activity thread (theseus-sjzm, the owner's D5): the
//! binding's one helper for making, archiving, unarchiving and naming a
//! Discord thread in the owner's guild. A task's card that wants a Discord
//! thread of its own uses this and makes none itself.
//!
//! `[threads] discord_channel` names a channel (a text channel, or a forum)
//! in the owner's guild; unset, nothing here runs. Each active thread gets a
//! Discord thread there, made at the thread's first message
//! (`thread.changed { opened }`), named by its title (🔒 first when sealed),
//! recorded in the thread's `surfaces` and run as a place of its own, so the
//! thread's session posts there: its replies, its tasks' cards and the
//! pointers to what the owner said in the DM. Pause and done archive it;
//! resume and reopen unarchive it; an edit renames it. Changes are applied in
//! the order they were heard, one at a time.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use theseus_protocol::threads::{channel_surface, ThreadChanged, ThreadState};
use tokio::sync::mpsc;
use twilight_model::channel::thread::AutoArchiveDuration;
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;

use super::thread_bind::ThreadMsg;
use super::Shared;

/// Discord's longest thread name.
const NAME_MAX: usize = 100;

/// META key of the Discord thread the binding made for a thread.
fn made_key(slug: &str) -> String {
    format!("discord.guild_thread.{slug}")
}

/// A Discord thread the binding made for an activity thread.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Made {
    thread: u64,
    name: String,
}

/// A Discord thread's name: the thread's title, 🔒 first when it is sealed,
/// within Discord's limit.
pub(super) fn thread_name(title: &str, sealed: bool) -> String {
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let lock = if sealed { "🔒 " } else { "" };
    let room = NAME_MAX - lock.chars().count();
    let title: String = title.chars().take(room).collect();
    let title = if title.is_empty() {
        "thread".into()
    } else {
        title
    };
    format!("{lock}{title}")
}

/// A change of a thread, heard from the core: sent to the syncer, in order.
pub(super) fn heard(shared: &Arc<Shared>, c: &ThreadChanged) {
    if let Some(tx) = shared.threads.sync.get() {
        let _ = tx.send(c.clone());
    }
}

/// The syncer: each change in the order heard.
pub(super) async fn run(shared: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<ThreadChanged>) {
    while let Some(c) = rx.recv().await {
        if let Err(e) = shared.thread_changed(&c).await {
            shared.board.error(
                "discord thread",
                None,
                format!("{} {}: {e:#}", c.change, c.slug),
            );
        }
    }
}

impl Shared {
    /// The channel the Discord threads are made in, if there is one.
    fn guild_threads_in(&self) -> Option<u64> {
        self.core
            .cfg
            .threads
            .discord_channel
            .as_deref()?
            .parse()
            .ok()
    }

    fn made(&self, slug: &str) -> Option<Made> {
        self.core
            .store
            .get_meta::<Made>(&made_key(slug))
            .ok()
            .flatten()
    }

    /// Apply one change.
    async fn thread_changed(self: &Arc<Self>, c: &ThreadChanged) -> anyhow::Result<()> {
        // A thread the config binds to a channel, now that it exists.
        if self.threads.channel_of(&c.slug).is_none() && c.state != ThreadState::Done {
            self.apply_config(&c.slug);
        }
        let live = matches!(c.change.as_str(), "opened" | "resumed" | "reopened");
        // A channel that runs the thread takes its (new) session.
        if live {
            if let Some(channel) = self.threads.channel_of(&c.slug) {
                self.tell_place(channel, ThreadMsg::Adopt(c.slug.clone()));
            }
        }
        let Some(parent) = self.guild_threads_in() else {
            return Ok(());
        };
        let Some(made) = self.made(&c.slug) else {
            let own = c.surfaces.iter().any(|s| s.starts_with("discord:channel:"));
            if c.change == "opened" && c.state == ThreadState::Active && !own {
                self.make_guild_thread(c, parent).await?;
            }
            return Ok(());
        };
        match c.change.as_str() {
            "paused" | "done" => self.guild_thread_set(made.thread, Some(true), None).await?,
            "resumed" | "reopened" | "opened" => {
                self.guild_thread_set(made.thread, Some(false), None)
                    .await?;
            }
            _ => {}
        }
        let name = thread_name(&c.title, c.sealed);
        if name != made.name {
            self.guild_thread_set(made.thread, None, Some(&name))
                .await?;
            self.core.store.put_meta(
                &made_key(&c.slug),
                &Made {
                    thread: made.thread,
                    name,
                },
            )?;
        }
        Ok(())
    }

    /// Make the thread's Discord thread, run it as a place, and record it.
    async fn make_guild_thread(
        self: &Arc<Self>,
        c: &ThreadChanged,
        parent: u64,
    ) -> anyhow::Result<()> {
        let name = thread_name(&c.title, c.sealed);
        let starter = format!("🧵 {} · `thread:{}`", c.title, c.slug);
        let (thread, guild) = self.guild_thread_create(parent, &name, &starter).await?;
        self.core.store.put_meta(
            &made_key(&c.slug),
            &Made {
                thread,
                name: name.clone(),
            },
        )?;
        // The thread's place says no bind notice: it is not a fresh place.
        self.core
            .store
            .put_meta(&format!("discord.bound.channel:{thread}"), &true)?;
        let b = self.dynamic_lane(guild, thread, &format!("#{}", c.title))?;
        self.start_channel(&b).await?;
        self.threads.bind(thread, &c.slug);
        self.core
            .thread_surfaces(&c.slug, Some(&channel_surface(thread)), None, "discord")?;
        self.tell_place(thread, ThreadMsg::Adopt(c.slug.clone()));
        Ok(())
    }

    /// A Discord thread in `parent`, a text channel's or a forum's, named
    /// `name`; the forum's first post says `starter`. Its id and its guild.
    pub(super) async fn guild_thread_create(
        &self,
        parent: u64,
        name: &str,
        starter: &str,
    ) -> anyhow::Result<(u64, Option<u64>)> {
        let channel = self.http.channel(Id::new(parent)).await?.model().await?;
        let guild = channel.guild_id.map(Id::get);
        let week = AutoArchiveDuration::Week;
        let made = if channel.kind == ChannelType::GuildForum {
            let post = self
                .http
                .create_forum_thread(Id::new(parent), name)
                .auto_archive_duration(week)
                .message()
                .content(starter)
                .await?
                .model()
                .await?;
            post.channel.id
        } else {
            self.http
                .create_thread(Id::new(parent), name, ChannelType::PublicThread)
                .auto_archive_duration(week)
                .await?
                .model()
                .await?
                .id
        };
        Ok((made.get(), guild))
    }

    /// Archive or unarchive a Discord thread, and rename it.
    pub(super) async fn guild_thread_set(
        &self,
        thread: u64,
        archived: Option<bool>,
        name: Option<&str>,
    ) -> anyhow::Result<()> {
        let mut req = self.http.update_thread(Id::new(thread));
        if let Some(a) = archived {
            req = req.archived(a);
        }
        if let Some(n) = name {
            req = req.name(n);
        }
        req.await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name is the title, 🔒 first when sealed, within Discord's limit.
    #[test]
    fn a_name_is_the_title_with_a_lock_when_sealed_within_the_limit() {
        assert_eq!(thread_name("Tide  gauges", false), "Tide gauges");
        assert_eq!(thread_name("Tide gauges", true), "🔒 Tide gauges");
        let long = "x".repeat(300);
        assert_eq!(thread_name(&long, false).chars().count(), 100);
        assert_eq!(thread_name(&long, true).chars().count(), 100);
        assert!(thread_name(&long, true).starts_with("🔒 x"));
        assert_eq!(thread_name("  ", false), "thread");
    }
}
