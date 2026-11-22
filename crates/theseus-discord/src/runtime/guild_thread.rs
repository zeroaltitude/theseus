//! The binding's one helper for making, archiving, unarchiving and naming a
//! Discord thread in the owner's guild: a task's card that wants a Discord
//! thread of its own uses this and makes none itself.

use twilight_model::channel::thread::AutoArchiveDuration;
use twilight_model::channel::ChannelType;
use twilight_model::id::Id;

use super::Shared;

impl Shared {
    /// A Discord thread in `parent`, a text channel's or a forum's, named
    /// `name`, archiving itself after `archive` of quiet (a task's is a day);
    /// the forum's first post says `starter`. Its id and its guild.
    pub(super) async fn guild_thread_create(
        &self,
        parent: u64,
        name: &str,
        starter: &str,
        archive: AutoArchiveDuration,
    ) -> anyhow::Result<(u64, Option<u64>)> {
        let channel = self.http.channel(Id::new(parent)).await?.model().await?;
        let guild = channel.guild_id.map(Id::get);
        let made = if channel.kind == ChannelType::GuildForum {
            let post = self
                .http
                .create_forum_thread(Id::new(parent), name)
                .auto_archive_duration(archive)
                .message()
                .content(starter)
                .await?
                .model()
                .await?;
            post.channel.id
        } else {
            self.http
                .create_thread(Id::new(parent), name, ChannelType::PublicThread)
                .auto_archive_duration(archive)
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
