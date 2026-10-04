//! Places in many guilds, each with a ceiling (step 38a, theseus-ext.3): what
//! the binding tells the core of its bindings file's guilds and places, the
//! bot's roles in each guild, and each place's spend limit. A child of
//! `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - Each guild has its own word on trust (`[[guild]]`'s `private`), told to
//!   the core with the places (`Core::trust_guilds`), so a trusted guild beside
//!   an untrusted one keeps each channel's class.
//! - A message or an interaction finds its place by its channel alone in any
//!   guild (`Routes::resolve`): a channel id is Discord's, unique across
//!   guilds, so routing needs no guild.
//! - Slash commands stay global (one list reaches every guild and the DMs).

use std::collections::HashMap;

use serde_json::{json, Value};
use theseus_protocol::PlaceCeiling;
use twilight_model::id::marker::GuildMarker;
use twilight_model::id::Id;

use super::Shared;
use crate::bindings::{snowflake, Bindings};

/// Each place's guild and ceiling, by its key (`channel:<id>`, `dm:<user>`):
/// what its health, its `discord.bound` row, and its spend limit read.
#[derive(Default)]
pub(crate) struct PlaceBits(HashMap<String, Bits>);

#[derive(Clone, Default)]
pub(crate) struct Bits {
    pub(crate) guild: Option<String>,
    pub(crate) ceiling: Option<PlaceCeiling>,
}

impl PlaceBits {
    pub(crate) fn new(b: &Bindings) -> Self {
        let channels = b.channel.iter().map(|c| {
            let bits = Bits {
                guild: Some(c.guild.clone()),
                ceiling: c.ceiling.clone(),
            };
            (format!("channel:{}", c.id), bits)
        });
        let dms = b.dm.iter().map(|d| {
            let bits = Bits {
                guild: None,
                ceiling: d.ceiling.clone(),
            };
            (format!("dm:{}", d.user), bits)
        });
        Self(channels.chain(dms).collect())
    }

    pub(crate) fn get(&self, key: &str) -> Bits {
        self.0.get(key).cloned().unwrap_or_default()
    }
}

/// The places the file binds, as the place rule takes them: a channel is
/// private by its own word, else by its guild's (theseus-rdqg), with its
/// guild and its ceiling.
pub(super) fn bound_places(b: &Bindings) -> Vec<theseus_core::places::BoundPlace> {
    let channels = b.channel.iter().map(|c| theseus_core::places::BoundPlace {
        target: format!("discord:channel:{}", c.id),
        name: c.label(),
        private: b.is_private(c),
        guild: Some(c.guild.clone()),
        ceiling: c.ceiling.clone(),
    });
    let dms = b.dm.iter().map(|d| theseus_core::places::BoundPlace {
        target: format!("discord:dm:{}", d.user),
        name: d.label(),
        private: false,
        guild: None,
        ceiling: d.ceiling.clone(),
    });
    channels.chain(dms).collect()
}

/// Every guild the file binds, as ids.
pub(super) fn guild_ids(b: &Bindings) -> anyhow::Result<Vec<Id<GuildMarker>>> {
    b.guilds
        .iter()
        .map(|g| Ok(Id::new(snowflake("guild id", &g.id)?)))
        .collect()
}

/// What the binding checks a ceiling's profile against: each one the config
/// names. `Err` names the first ceiling whose profile it lacks.
pub(super) fn check_profiles(core: &theseus_core::Core, b: &Bindings) -> anyhow::Result<()> {
    let ceilings = b
        .channel
        .iter()
        .map(|c| (c.label(), &c.ceiling))
        .chain(b.dm.iter().map(|d| (d.label(), &d.ceiling)));
    for (label, c) in ceilings {
        let Some(p) = c.as_ref().and_then(|c| c.profile.as_deref()) else {
            continue;
        };
        if core.cfg.profile(p).is_err() {
            anyhow::bail!("{label}'s ceiling names profile {p:?}, which the config does not have");
        }
    }
    Ok(())
}

impl Shared {
    /// The bot's roles in every bound guild, so an @Theseus that resolves to
    /// its managed role in any of them still counts as a mention.
    pub(super) async fn refresh_bot_roles(&self, guilds: &[Id<GuildMarker>]) {
        let mut roles = Vec::new();
        for guild in guilds {
            let asked = self.http.guild_member(*guild, Id::new(self.bot_id())).await;
            if let Ok(r) = asked {
                // Not in the guild yet: none there.
                if let Ok(m) = r.model().await {
                    roles.extend(m.roles.iter().map(|r| r.get()));
                }
            }
        }
        self.routes.lock().unwrap().bot_roles = roles;
    }

    /// The guilds of `guilds` the bot is not in, as the detail health says.
    pub(super) fn not_in(
        missing: &[Id<GuildMarker>],
        app: Id<twilight_model::id::marker::ApplicationMarker>,
    ) -> Option<String> {
        if missing.is_empty() {
            return None;
        }
        let named: Vec<String> = missing.iter().map(ToString::to_string).collect();
        Some(format!(
            "the bot is not in guild {} yet; invite it: https://discord.com/oauth2/authorize?client_id={app}&scope=bot+applications.commands&permissions=117824",
            named.join(", ")
        ))
    }

    /// The `discord.bound` row's data for the place `key`: its label, and its
    /// guild and ceiling (step 38a).
    pub(super) fn bound_row(&self, key: &str, label: &str) -> Value {
        let bits = self.place_bits.get(key);
        let mut row = json!({"place": key, "label": label});
        if let Some(g) = bits.guild {
            row["guild"] = json!(g);
        }
        if let Some(c) = bits.ceiling {
            row["ceiling"] = json!(c);
        }
        row
    }

    /// The place `key`'s session takes its ceiling's spend limit, the lower
    /// of it and the config's, or the config's again without one (step 38a).
    pub(super) fn place_limit(&self, key: &str, label: &str, session_id: &str) {
        let cap = self
            .place_bits
            .get(key)
            .ceiling
            .and_then(|c| c.spend_limit_usd);
        self.core.place_spend(session_id, label, cap);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::super::tests::{core_with, slash, EDDIE, MALLORY};
    use super::super::{shared_for_tests, start_places};
    use super::*;

    const HOME: &str = "100000000000000001";
    const AWAY: &str = "100000000000000002";
    const LAB: u64 = 223_456_789_012_345_678;
    const PIER: u64 = 223_456_789_012_345_681;
    const ELSEWHERE: u64 = 223_456_789_012_345_699;

    /// Guild A (`home`) trusted, with `#lab` and a floor; guild B untrusted,
    /// with a shared `#pier`, its tools `web` and its limit $1; and a DM.
    fn two() -> Bindings {
        Bindings::parse(&format!(
            "[[guild]]\nid = \"{HOME}\"\nname = \"home\"\nprivate = true\n\
             [[guild]]\nid = \"{AWAY}\"\n\
             [[channel]]\nguild = \"{HOME}\"\nid = \"{LAB}\"\nname = \"lab\"\nusers = [\"{EDDIE}\"]\nmention_only = false\n\
             [channel.ceiling]\nposture_floor = \"approve\"\n\
             [[channel]]\nguild = \"{AWAY}\"\nid = \"{PIER}\"\nname = \"pier\"\nusers = [\"{EDDIE}\", \"{MALLORY}\"]\n\
             [channel.ceiling]\ntools = [\"web\"]\nspend_limit_usd = 1\n\
             [[dm]]\nuser = \"{EDDIE}\"\nname = \"eddie\"\n"
        ))
        .unwrap()
    }

    /// The binding's start over two guilds, against the stand-in's REST: each
    /// place is bound with its guild and ceiling (its `discord.bound` row,
    /// health, the core's place rule), `#pier`'s session takes its $1 limit,
    /// and a slash command reaches the place of its channel in either guild,
    /// while one in a channel neither binds gets no answer.
    #[tokio::test]
    async fn interactions_route_by_channel_across_two_guilds() {
        let fake = theseus_sim::fake_discord::FakeDiscord::start();
        let d = tempfile::tempdir().unwrap();
        let addr = fake.addr.clone();
        let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
            c.discord.rest_proxy = Some(addr)
        });
        let b = two();
        core.trust_guilds(b.trusted());
        core.bind_places(bound_places(&b));
        let mut shared = shared_for_tests(&core);
        Arc::get_mut(&mut shared).unwrap().place_bits = PlaceBits::new(&b);
        shared.clone().start_lanes(&b).unwrap();
        let (_notes_tx, notes) = tokio::sync::mpsc::unbounded_channel();
        start_places(&shared, &b, &shared.board, notes)
            .await
            .unwrap();

        // Each place, with its guild and ceiling, as health and the ledger say.
        let st = core.bindings.all().pop().unwrap();
        let places: Vec<(String, Option<String>, bool)> = st
            .places
            .iter()
            .map(|p| (p.label.clone(), p.guild.clone(), p.ceiling.is_some()))
            .collect();
        assert_eq!(
            places,
            [
                ("#lab".into(), Some(HOME.into()), true),
                ("#pier".into(), Some(AWAY.into()), true),
                ("DM @eddie".into(), None, false),
            ]
        );
        let bound: Vec<serde_json::Value> = core
            .store
            .ledger_tail::<theseus_core::ledger::LedgerRow>(10_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "discord.bound")
            .map(|(_, r)| r.data)
            .collect();
        let pier = bound.iter().find(|r| r["label"] == "#pier").unwrap();
        assert_eq!(pier["guild"], AWAY);
        assert_eq!(pier["ceiling"]["tools"], serde_json::json!(["web"]));
        assert_eq!(pier["ceiling"]["spend_limit_usd"], 1.0);
        let class = |c: u64| {
            let rule = &core.runner.place_rule;
            rule.class(&core.cfg, Some(&format!("discord:channel:{c}")))
        };
        assert_eq!(
            class(LAB),
            theseus_protocol::PlaceClass::Private,
            "its guild is trusted"
        );
        assert_eq!(class(PIER), theseus_protocol::PlaceClass::Shared);
        let sid = core
            .outbox
            .place_session(&format!("channel:{PIER}"))
            .unwrap()
            .unwrap();
        let rec: theseus_core::session::SessionRecord =
            core.store.get_session(&sid).unwrap().unwrap();
        let e = core
            .kernel
            .execution(&rec.execution_id.unwrap())
            .unwrap()
            .unwrap();
        assert_eq!((e.budget.limit_micros, e.budget.pinned), (1_000_000, true));

        // A slash command in each guild reaches its channel's place.
        let said = |what: &str| {
            fake.replies()
                .iter()
                .any(|r| r.content.as_deref().is_some_and(|c| c.contains(what)))
        };
        for (guild, channel, label) in [(HOME, LAB, "(#lab)"), (AWAY, PIER, "(#pier)")] {
            shared
                .clone()
                .on_interaction(slash(Some(guild), channel, EDDIE, "status"))
                .await;
            let t0 = Instant::now();
            while !said(label) {
                assert!(
                    t0.elapsed() < Duration::from_secs(10),
                    "{label}: {:?}",
                    fake.replies()
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        // A channel neither guild's places bind: no answer.
        let before = fake.replies().len();
        shared
            .clone()
            .on_interaction(slash(Some(AWAY), ELSEWHERE, EDDIE, "status"))
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(fake.replies().len(), before);
    }
}
