//! Who can view a guild channel (theseus-sgh, spec §3.9 "Approval"). A listed
//! guild channel is a trusted channel only while nobody outside
//! `[approval].trusted_users` can view it. Knowing who can view it takes the
//! member list, which Discord gives only to a bot whose Server Members intent
//! is on, a setting in the developer portal that the application's flags
//! report. Without it a listed channel cannot be verified, and so it is not
//! trusted.

use twilight_model::channel::permission_overwrite::PermissionOverwrite;
use twilight_model::channel::ChannelType;
use twilight_model::guild::Permissions;
use twilight_model::id::marker::{GuildMarker, RoleMarker, UserMarker};
use twilight_model::id::Id;
use twilight_model::oauth::ApplicationFlags;
use twilight_util::permission_calculator::PermissionCalculator;

/// Why a listed guild channel is not trusted when the intent is off.
pub const NO_INTENT: &str = "cannot be verified without the Server Members intent (Discord \
                             developer portal: the bot's Privileged Gateway Intents), so it is \
                             not trusted";

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

/// The members outside `trusted` who can view a channel of `kind` with
/// these overwrites. The bot itself does not count; any other bot does, like
/// anyone else.
pub fn outsiders<'m>(
    guild: &Guild<'_>,
    kind: ChannelType,
    overwrites: &[PermissionOverwrite],
    members: &'m [Member],
    trusted: &[u64],
    bot: u64,
) -> Vec<&'m Member> {
    let everyone = guild
        .roles
        .iter()
        .find(|(id, _)| id.get() == guild.id.get())
        .map_or(Permissions::empty(), |(_, p)| *p);
    members
        .iter()
        .filter(|m| m.id != bot && !trusted.contains(&m.id))
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

/// The verdict on a channel, in words: trusted when nobody outside the
/// trusted users can view it.
pub fn verdict(outside: &[&Member], checked: usize) -> (bool, String) {
    if outside.is_empty() {
        return (
            true,
            format!("only trusted users can view it ({checked} members checked)"),
        );
    }
    let names: Vec<String> = outside
        .iter()
        .take(5)
        .map(|m| format!("{} ({})", m.name, m.id))
        .collect();
    let more = outside.len().saturating_sub(names.len());
    (
        false,
        format!(
            "{} {} outside trusted_users can view it: {}{}",
            outside.len(),
            if outside.len() == 1 {
                "member"
            } else {
                "members"
            },
            names.join(", "),
            if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            }
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use twilight_model::channel::permission_overwrite::PermissionOverwriteType;

    const GUILD: u64 = 712398310421561444;
    const EDDIE: u64 = 159471966640799744;
    const MALLORY: u64 = 222222222222222222;
    const OWNER: u64 = 333333333333333333;
    const BOT: u64 = 1553557742759706625;
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
        outsiders(
            &guild,
            ChannelType::GuildText,
            overwrites,
            &members,
            &[EDDIE, OWNER],
            BOT,
        )
        .into_iter()
        .map(|m| m.id)
        .collect()
    }

    /// A channel everyone can view is not trusted; one that hides from
    /// `@everyone` and lets only Eddie in is; a role that lets someone
    /// untrusted in, or an administrator, makes it untrusted again. The owner
    /// can view everything, so the owner must be trusted, and the bot itself
    /// never counts.
    #[test]
    fn a_channel_is_trusted_only_while_nobody_untrusted_can_view_it() {
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

    #[test]
    fn the_verdict_names_who_can_view_it() {
        let m = member(MALLORY, "mallory", &[]);
        assert_eq!(
            verdict(&[&m], 4),
            (
                false,
                "1 member outside trusted_users can view it: mallory (222222222222222222)".into()
            )
        );
        let many: Vec<Member> = (0..7)
            .map(|i| member(100000000000000000 + i, &format!("m{i}"), &[]))
            .collect();
        let refs: Vec<&Member> = many.iter().collect();
        let (trusted, why) = verdict(&refs, 9);
        assert!(!trusted && why.starts_with("7 members") && why.ends_with(", and 2 more"));
        assert_eq!(
            verdict(&[], 3),
            (
                true,
                "only trusted users can view it (3 members checked)".into()
            )
        );
    }

    /// Without the intent's flag, a listed guild channel cannot be verified.
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
        assert!(NO_INTENT.starts_with("cannot be verified without the Server Members intent"));
    }
}
