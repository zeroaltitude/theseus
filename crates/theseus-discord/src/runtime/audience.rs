//! Who can view a guild channel: one walk of its viewers serves the approval
//! check (`check_channel`, theseus-sgh) and the place rule's one read of a
//! channel bound `private = true` (theseus-nbsh). A child of `runtime`, apart
//! from it for the shape budget's file ceiling.

use twilight_model::guild::Permissions;
use twilight_model::id::marker::RoleMarker;
use twilight_model::id::Id;

use super::Shared;
use crate::viewers;

/// A guild channel as a read of its viewers finds it (theseus-sgh).
pub(crate) struct View {
    /// Every member who can view it, the bot aside.
    pub(super) viewers: Vec<viewers::Member>,
    /// The guild's members checked.
    pub(super) checked: usize,
}

impl Shared {
    /// Who can view a channel bound `private = true`, read once at the
    /// binding's start (the place rule, theseus-nbsh), for health. Never
    /// before a turn: the operator's word is trusted, and this checks it.
    pub(crate) async fn check_private(&self, channel: u64, name: &str) {
        let viewers = match self.members_intent().await {
            false => Err(viewers::NO_INTENT_AUDIENCE.to_string()),
            true => self
                .view(channel)
                .await
                .map(|v| v.viewers.into_iter().map(|m| (m.id, m.name)).collect())
                .map_err(|e| format!("{e:#}")),
        };
        self.core.private_place_viewed(channel, name, viewers);
    }

    /// Everyone who can view a guild channel, the bot aside: the guild's
    /// roles and owner, the channel's overwrites, and every member, through
    /// twilight's permission calculation. The approval check takes those
    /// outside `[approval].trusted_users`; the place rule's check, those who
    /// are not the owner.
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
            viewers,
            checked: members.len(),
        })
    }
}
