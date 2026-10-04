//! Who can view a guild channel: the place rule's one read, at the binding's
//! start, of a channel bound `private = true` (theseus-nbsh), which health
//! shows. Knowing who can view it takes the member list, which Discord gives
//! only to a bot whose Server Members intent is on, a setting in the
//! developer portal that the application's flags report. Without it health
//! says the channel's viewers could not be read.

use twilight_model::channel::permission_overwrite::PermissionOverwrite;
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::id::marker::{GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;
use twilight_model::oauth::ApplicationFlags;
use twilight_util::permission_calculator::PermissionCalculator;

/// The developer portal has the Server Members intent on for this
/// application: a verified bot's flag, or the one for a bot in fewer than a
/// hundred servers.
pub fn members_intent(flags: Option<ApplicationFlags>) -> bool {
    flags.is_some_and(|f| {
        f.intersects(
            ApplicationFlags::GATEWAY_GUILD_MEMBERS
                | ApplicationFlags::GATEWAY_GUILD_MEMBERS_LIMITED,
        )
    })
}

/// A guild member, as the check needs one.
#[derive(Debug, Clone)]
pub struct Member {
    pub id: u64,
    pub name: String,
    pub roles: Vec<Id<RoleMarker>>,
}

/// A guild's permissions: its roles (with `@everyone`, whose id is the
/// guild's), and its owner, who can view everything.
pub struct Guild<'a> {
    pub id: Id<GuildMarker>,
    pub owner: Id<UserMarker>,
    pub roles: &'a [(Id<RoleMarker>, Permissions)],
}

/// Why a guild channel's audience cannot be read without the intent (M4
/// 19a): it then counts as public, and owner-only material is withheld there.
pub const NO_INTENT_AUDIENCE: &str = "the bot's Server Members intent is off";

/// Every member who can view a channel of `kind` with these overwrites, the
/// bot aside (M4 19a): the audience of the session there.
pub fn can_view<'m>(
    guild: &Guild<'_>,
    kind: ChannelType,
    overwrites: &[PermissionOverwrite],
    members: &'m [Member],
    bot: u64,
) -> Vec<&'m Member> {
    let everyone = guild
        .roles
        .iter()
        .find(|(id, _)| id.get() == guild.id.get())
        .map_or(Permissions::empty(), |(_, p)| *p);
    members
        .iter()
        .filter(|m| m.id != bot)
        .filter(|m| {
            // Every role the member holds, so an overwrite for any of them
            // applies; one the guild's list lacks grants nothing guild-wide.
            let roles: Vec<(Id<RoleMarker>, Permissions)> = m
                .roles
                .iter()
                .filter(|r| r.get() != guild.id.get())
                .map(|r| {
                    let perms = guild.roles.iter().find(|(id, _)| id == r);
                    (*r, perms.map_or(Permissions::empty(), |(_, p)| *p))
                })
                .collect();
            PermissionCalculator::new(guild.id, Id::new(m.id), everyone, &roles)
                .owner_id(guild.owner)
                .in_channel(kind, overwrites)
                .contains(Permissions::VIEW_CHANNEL)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::channel::permission_overwrite::PermissionOverwriteType;

    const GUILD: u64 = 314159265358979323;
    const EDDIE: u64 = 271828182845904523;
    const MALLORY: u64 = 222222222222222222;
    const OWNER: u64 = 333333333333333333;
    const BOT: u64 = 1618033988749894848;
    const HELPERS: u64 = 444444444444444444;

    fn member(id: u64, name: &str, roles: &[u64]) -> Member {
        Member {
            id,
            name: name.into(),
            roles: roles.iter().map(|r| Id::new(*r)).collect(),
        }
    }

    fn overwrite(id: u64, kind: PermissionOverwriteType, allow: bool) -> PermissionOverwrite {
        let (a, d) = if allow {
            (Permissions::VIEW_CHANNEL, Permissions::empty())
        } else {
            (Permissions::empty(), Permissions::VIEW_CHANNEL)
        };
        PermissionOverwrite {
            allow: a,
            deny: d,
            id: Id::new(id),
            kind,
        }
    }

    fn check(roles: &[(u64, Permissions)], overwrites: &[PermissionOverwrite]) -> Vec<u64> {
        let roles: Vec<(Id<RoleMarker>, Permissions)> =
            roles.iter().map(|(id, p)| (Id::new(*id), *p)).collect();
        let guild = Guild {
            id: Id::new(GUILD),
            owner: Id::new(OWNER),
            roles: &roles,
        };
        let members = [
            member(EDDIE, "eddie", &[]),
            member(MALLORY, "mallory", &[HELPERS]),
            member(OWNER, "owner", &[]),
            member(BOT, "Theseus", &[]),
        ];
        can_view(&guild, ChannelType::GuildText, overwrites, &members, BOT)
            .into_iter()
            .map(|m| m.id)
            .filter(|id| ![EDDIE, OWNER].contains(id))
            .collect()
    }

    /// Who can view a channel, besides Eddie and the guild's owner: everyone,
    /// for a channel everyone can view; nobody, for one that hides from
    /// `@everyone` and lets only Eddie in; a role that lets someone in, or an
    /// administrator, shows them again. The owner can view everything, and
    /// the bot itself never counts.
    #[test]
    fn who_can_view_a_channel_follows_its_roles_and_overwrites() {
        use PermissionOverwriteType::{Member as M, Role as R};
        let open = [(
            GUILD,
            Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES,
        )];
        assert_eq!(check(&open, &[]), [MALLORY]);
        let private = [overwrite(GUILD, R, false), overwrite(EDDIE, M, true)];
        assert!(check(&open, &private).is_empty());
        let helpers_in = [
            overwrite(GUILD, R, false),
            overwrite(EDDIE, M, true),
            overwrite(HELPERS, R, true),
        ];
        assert_eq!(check(&open, &helpers_in), [MALLORY]);
        let admins = [open[0], (HELPERS, Permissions::ADMINISTRATOR)];
        assert_eq!(check(&admins, &private), [MALLORY]);
        // Mallory denied by name, though her role would let her in.
        let named_out = [
            overwrite(GUILD, R, false),
            overwrite(HELPERS, R, true),
            overwrite(MALLORY, M, false),
        ];
        assert!(check(&open, &named_out).is_empty());
    }

    /// Without the intent's flag, a channel's viewers cannot be read.
    #[test]
    fn the_intent_comes_from_the_application_flags() {
        assert!(!members_intent(None));
        assert!(!members_intent(Some(ApplicationFlags::GATEWAY_PRESENCE)));
        assert!(members_intent(Some(
            ApplicationFlags::GATEWAY_GUILD_MEMBERS_LIMITED
        )));
        assert!(members_intent(Some(
            ApplicationFlags::GATEWAY_GUILD_MEMBERS | ApplicationFlags::EMBEDDED
        )));
    }
}
