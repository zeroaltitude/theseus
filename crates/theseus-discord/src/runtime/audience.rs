//! The audience (M4 19a): who can view each bound guild channel, read and told
//! to the core (`Core::place_viewers`), which compiles the session there for
//! them. One walk of a channel's viewers also serves the approval check
//! (`check_channel`, theseus-sgh). A child of `runtime`, apart from it for
//! the shape budget's file ceiling.

use std::time::Duration;

use twilight_model::guild::Permissions;
use twilight_model::id::marker::RoleMarker;
use twilight_model::id::Id;

use super::Shared;
use crate::viewers;

/// A guild channel as a read of its viewers finds it (theseus-sgh, M4 19a).
pub(crate) struct View {
    pub(super) name: Option<String>,
    /// Every member who can view it, the bot aside.
    pub(super) viewers: Vec<viewers::Member>,
    /// The guild's members checked.
    pub(super) checked: usize,
}

/// A turn in a guild channel reads its viewers again when the last read is
/// older than this (M4 19a).
const AUDIENCE_FRESH: Duration = Duration::from_secs(60);

impl Shared {
    /// Read who can view a bound guild channel, and tell the core: the
    /// audience of the session there (M4 19a). Without the Server Members
    /// intent nobody's view can be read, and the channel counts as public.
    pub(crate) async fn read_audience(&self, channel: u64) {
        let view = match self.members_intent().await {
            true => Some(self.view(channel).await),
            false => None,
        };
        self.tell_audience(channel, view);
    }

    /// `read_audience`, unless it was read in the last minute: a turn in a
    /// guild channel compiles for whoever can view it now.
    pub(crate) async fn read_audience_if_stale(&self, channel: u64) {
        let now = std::time::Instant::now();
        let stale = {
            let mut read = self.audience_read.lock().unwrap();
            let stale = read
                .get(&channel)
                .is_none_or(|at| now.duration_since(*at) >= AUDIENCE_FRESH);
            if stale {
                read.insert(channel, now);
            }
            stale
        };
        if stale {
            self.read_audience(channel).await;
        }
    }

    /// Every bound guild channel's audience, read again: a role or a channel
    /// changed (M4 19a).
    pub(crate) async fn read_audiences(&self) {
        let channels = self.routes.lock().unwrap().guild_channels.clone();
        for c in channels {
            self.read_audience(c).await;
        }
    }

    /// The channel is a guild channel the bindings file binds.
    pub(super) fn bound_channel(&self, channel: u64) -> bool {
        self.routes
            .lock()
            .unwrap()
            .guild_channels
            .contains(&channel)
    }

    /// Tell the core what a read found. A read that failed keeps the last
    /// one: its error is on the board.
    pub(super) fn tell_audience(&self, channel: u64, view: Option<anyhow::Result<View>>) {
        match view {
            Some(Ok(v)) => {
                let ids = v.viewers.iter().map(|m| m.id).collect();
                self.core.place_viewers(channel, v.name, Some(ids), None);
            }
            Some(Err(e)) => self.board.error(
                "read who can view a channel",
                None,
                format!("{channel}: {e}"),
            ),
            None => {
                // Named as the bindings file names it: no request needs it.
                let name = self
                    .routes
                    .lock()
                    .unwrap()
                    .channel_names
                    .get(&channel)
                    .cloned();
                self.core
                    .place_viewers(channel, name, None, Some(viewers::NO_INTENT_AUDIENCE));
            }
        }
    }

    /// Everyone who can view a guild channel, the bot aside: the guild's
    /// roles and owner, the channel's overwrites, and every member, through
    /// twilight's permission calculation. The approval check takes those
    /// outside `[approval].trusted_users`; the audience takes them all.
    pub(super) async fn view(&self, channel: u64) -> anyhow::Result<View> {
        let ch = self.http.channel(Id::new(channel)).await?.model().await?;
        let guild_id = ch
            .guild_id
            .ok_or_else(|| anyhow::anyhow!("it is not a guild channel"))?;
        let guild = self.http.guild(guild_id).await?.model().await?;
        let roles: Vec<(Id<RoleMarker>, Permissions)> =
            guild.roles.iter().map(|r| (r.id, r.permissions)).collect();
        let mut members = Vec::new();
        let mut after = None;
        loop {
            let mut req = self.http.guild_members(guild_id).limit(1000);
            if let Some(a) = after {
                req = req.after(a);
            }
            let page = req.await?.models().await?;
            let full = page.len() == 1000;
            after = page.last().map(|m| m.user.id);
            members.extend(page.into_iter().map(|m| viewers::Member {
                id: m.user.id.get(),
                name: m.user.name,
                roles: m.roles,
            }));
            if !full {
                break;
            }
        }
        let g = viewers::Guild {
            id: guild_id,
            owner: guild.owner_id,
            roles: &roles,
        };
        let overwrites = ch.permission_overwrites.unwrap_or_default();
        let viewers = viewers::can_view(&g, ch.kind, &overwrites, &members, self.bot_id())
            .into_iter()
            .cloned()
            .collect();
        Ok(View {
            name: ch.name,
            viewers,
            checked: members.len(),
        })
    }
}
