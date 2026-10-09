//! The bindings file (spec P5): which Discord places Theseus lives in and who
//! may drive it there. Its SHA-256 prefix is the binding revision.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use theseus_protocol::{GuildInfo, PlaceCeiling};
use toml::Spanned;

/// Printed by `theseusd example-bindings`; parsed by a test so it cannot drift.
pub const EXAMPLE_BINDINGS: &str = include_str!("../bindings.example.toml");

/// The bindings file, read. Two formats load (step 38a, theseus-ext.3):
///
/// - **Format 1**: one guild, as `guild_id` at the top, with the operator's
///   word on it as `private` beside it (theseus-rdqg). Its channels are in
///   that guild.
/// - **Format 2**: each guild a `[[guild]]` of its own, with its `id` and its
///   own `private`, and each `[[channel]]` naming its `guild`. A DM-only file
///   needs no guild.
///
/// A file that mixes the two is refused, naming the line. A `[channel.ceiling]`
/// or `[dm.ceiling]` narrows what its place gets (`PlaceCeiling`).
#[derive(Debug, Clone, PartialEq)]
pub struct Bindings {
    /// 1 or 2, as above.
    pub format: u8,
    /// Every guild, in the file's order: format 1's one, or format 2's.
    pub guilds: Vec<GuildBinding>,
    pub channel: Vec<ChannelBinding>,
    pub dm: Vec<DmBinding>,
    /// First 12 hex of the file's SHA-256.
    pub revision: String,
}

/// A guild Theseus binds places in, and the operator's word on it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuildBinding {
    pub id: String,
    /// A label for health and the web UI.
    #[serde(default)]
    pub name: Option<String>,
    /// The operator's word that the whole guild is theirs (theseus-rdqg):
    /// every `[[channel]]` in it is private unless it says `private = false`,
    /// and who can view one is never read, since the word covers the guild.
    /// Off by default: a channel is private only when it says so.
    #[serde(default)]
    pub private: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChannelBinding {
    /// Its guild's id: its own `guild` (format 2), or `guild_id` (format 1).
    pub guild: String,
    pub id: String,
    pub name: Option<String>,
    pub users: Vec<String>,
    /// Answer only messages that @mention Theseus or reply to one of its
    /// messages. On by default: a channel shared with people or other bots
    /// should not get a turn per message.
    pub mention_only: bool,
    /// The operator's word that only the owner can view it (the place rule,
    /// theseus-nbsh): its session gets everything, as a DM with the owner
    /// does. Absent, its guild's word decides (theseus-rdqg), and that is off
    /// by default: a guild channel is a shared place, with the public tools
    /// alone. Outside a trusted guild a channel bound private is read once at
    /// the binding's start, and health warns when anyone besides the owner
    /// can view it. Read it through `Bindings::is_private`.
    pub private: Option<bool>,
    /// A voice channel (rows 77 and 78), in any bound guild: `/join`, from a
    /// private place, brings Theseus into it when `[voice]` is on, and only
    /// its users are heard. Its text chat is the place's text.
    pub voice: bool,
    /// What the place gets beneath the place rule (step 38a).
    pub ceiling: Option<PlaceCeiling>,
    /// It shows each loop's tool line, and a notified call's embed
    /// (theseus-l1y1). On by default.
    pub show_tools: bool,
    /// It shows each loop's thinking (theseus-l1y1). On by default.
    pub show_thinking: bool,
}

/// A `[[channel]]` as the file writes it, with where its keys are.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChannel {
    #[serde(default)]
    guild: Option<Spanned<String>>,
    id: Spanned<String>,
    #[serde(default)]
    name: Option<String>,
    users: Vec<String>,
    #[serde(default = "yes")]
    mention_only: bool,
    #[serde(default)]
    private: Option<bool>,
    #[serde(default)]
    voice: bool,
    #[serde(default)]
    ceiling: Option<Ceiling>,
    #[serde(default = "yes")]
    show_tools: bool,
    #[serde(default = "yes")]
    show_thinking: bool,
}

/// The file as written, with where its format's keys are.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(default)]
    guild_id: Option<Spanned<String>>,
    #[serde(default)]
    private: Option<Spanned<bool>>,
    #[serde(default)]
    guild: Vec<Spanned<GuildBinding>>,
    #[serde(default)]
    channel: Vec<RawChannel>,
    #[serde(default)]
    dm: Vec<RawDm>,
}

fn yes() -> bool {
    true
}

/// A place's ceiling as the file writes it (`[channel.ceiling]`,
/// `[dm.ceiling]`): each key absent is no narrowing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ceiling {
    #[serde(default)]
    posture_floor: Option<String>,
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    spend_limit_usd: Option<f64>,
    #[serde(default)]
    profile: Option<String>,
}

impl Ceiling {
    /// The ceiling, checked; none when it narrows nothing. `place` names it.
    fn check(self, place: &str) -> Result<Option<PlaceCeiling>> {
        if let Some(f) = &self.posture_floor {
            if theseus_core::policy::Posture::parse(f).is_none() {
                bail!("{place}'s ceiling: posture_floor {f:?} is not open, notify, or approve");
            }
        }
        for t in self.tools.iter().flatten() {
            theseus_core::ceiling::check_entry(t)
                .map_err(|e| anyhow::anyhow!("{place}'s ceiling: tools entry {e}"))?;
        }
        if let Some(usd) = self.spend_limit_usd {
            if !(usd.is_finite() && usd > 0.0) {
                bail!("{place}'s ceiling: spend_limit_usd {usd} must be more than 0");
            }
        }
        if self.profile.as_deref().is_some_and(|p| p.trim().is_empty()) {
            bail!("{place}'s ceiling: profile is empty");
        }
        let c = PlaceCeiling {
            posture_floor: self.posture_floor,
            tools: self.tools,
            spend_limit_usd: self.spend_limit_usd,
            profile: self.profile,
        };
        Ok((!c.is_empty()).then_some(c))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDm {
    user: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    ceiling: Option<Ceiling>,
    #[serde(default = "yes")]
    show_tools: bool,
    #[serde(default = "yes")]
    show_thinking: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DmBinding {
    pub user: String,
    pub name: Option<String>,
    /// What the place gets beneath the place rule (step 38a).
    pub ceiling: Option<PlaceCeiling>,
    /// As a channel's (theseus-l1y1).
    pub show_tools: bool,
    pub show_thinking: bool,
}

impl ChannelBinding {
    pub fn label(&self) -> String {
        format!("#{}", self.name.as_deref().unwrap_or(&self.id))
    }
}

impl DmBinding {
    pub fn label(&self) -> String {
        format!("DM @{}", self.name.as_deref().unwrap_or(&self.user))
    }
}

/// A Discord id: 15 to 21 decimal digits.
pub fn snowflake(what: &str, s: &str) -> Result<u64> {
    let ok = (15..=21).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !ok {
        bail!("{what} {s:?} is not a Discord id (15-21 digits, as a string)");
    }
    s.parse().with_context(|| format!("{what} {s:?}"))
}

/// The line of `text` a key's span starts on, from 1.
fn line(text: &str, span: std::ops::Range<usize>) -> usize {
    text[..span.start.min(text.len())].matches('\n').count() + 1
}

impl Bindings {
    /// The guild `id`, if the file binds it.
    pub fn guild(&self, id: &str) -> Option<&GuildBinding> {
        self.guilds.iter().find(|g| g.id == id)
    }

    /// Whether the channel `c` is private: its own word, else its guild's
    /// (theseus-rdqg).
    pub fn is_private(&self, c: &ChannelBinding) -> bool {
        c.private
            .unwrap_or_else(|| self.guild(&c.guild).is_some_and(|g| g.private))
    }

    /// The guilds the operator trusts whole, by id.
    pub fn trusted(&self) -> std::collections::BTreeSet<String> {
        let trusted = self.guilds.iter().filter(|g| g.private);
        trusted.map(|g| g.id.clone()).collect()
    }

    /// Every guild, as health shows it.
    pub fn guild_infos(&self) -> Vec<GuildInfo> {
        self.guilds
            .iter()
            .map(|g| GuildInfo {
                id: g.id.clone(),
                name: g.name.clone(),
                trusted: g.private,
            })
            .collect()
    }

    /// The channels whose viewers the binding reads at its start (the place
    /// rule's one check): each one bound private, outside a trusted guild.
    /// In one, none: the operator's word covers the guild (theseus-rdqg).
    pub fn read_at_start(&self) -> impl Iterator<Item = &ChannelBinding> {
        self.channel.iter().filter(|c| {
            let trusted = self.guild(&c.guild).is_some_and(|g| g.private);
            !trusted && self.is_private(c)
        })
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading bindings file {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("bindings file {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let raw: Raw = toml::from_str(text)?;
        let (format, guilds) = Self::format_of(&raw, text)?;
        if raw.channel.is_empty() && raw.dm.is_empty() {
            bail!("no [[channel]] and no [[dm]]: nothing to bind");
        }
        let channel = raw
            .channel
            .into_iter()
            .map(|c| Self::channel_of(c, format, &guilds, text))
            .collect::<Result<Vec<_>>>()?;
        let mut dm = Vec::new();
        for d in raw.dm {
            snowflake("dm user", &d.user)?;
            let label = format!("DM @{}", d.name.as_deref().unwrap_or(&d.user));
            let ceiling = d.ceiling.map(|x| x.check(&label)).transpose()?.flatten();
            dm.push(DmBinding {
                user: d.user,
                name: d.name,
                ceiling,
                show_tools: d.show_tools,
                show_thinking: d.show_thinking,
            });
        }
        let digest = Sha256::digest(text.as_bytes());
        Ok(Bindings {
            format,
            guilds,
            channel,
            dm,
            revision: hex::encode(digest)[..12].to_string(),
        })
    }

    /// The file's format and its guilds: format 1's one, from `guild_id` and
    /// its `private`, or format 2's `[[guild]]`s. A file that mixes them is
    /// refused, naming the lines.
    fn format_of(raw: &Raw, text: &str) -> Result<(u8, Vec<GuildBinding>)> {
        let at = |span| line(text, span);
        let named_guild = raw.channel.iter().find_map(|c| c.guild.as_ref());
        let format2 = raw.guild.first().map(|g| ("[[guild]]", g.span()));
        let format2 = format2.or(named_guild.map(|g| ("a channel's guild", g.span())));
        match (&raw.guild_id, format2) {
            (Some(id), Some((what, span))) => bail!(
                "line {}: guild_id is format 1's, and {what} (line {}) is format 2's: write one or the \
                 other (format 2: a [[guild]] each, and each [[channel]] naming its guild)",
                at(id.span()),
                at(span)
            ),
            (Some(id), None) => {
                snowflake("guild_id", id.get_ref())?;
                let private = raw.private.as_ref().is_some_and(|p| *p.get_ref());
                let one = GuildBinding {
                    id: id.get_ref().clone(),
                    name: None,
                    private,
                };
                Ok((1, vec![one]))
            }
            (None, _) => {
                if let Some(p) = &raw.private {
                    bail!(
                        "line {}: private at the top is format 1's word, beside guild_id; in \
                         format 2 a guild's private is in its [[guild]]",
                        at(p.span())
                    );
                }
                let mut guilds: Vec<GuildBinding> = Vec::new();
                for g in &raw.guild {
                    snowflake("guild id", &g.get_ref().id)?;
                    if guilds.iter().any(|h| h.id == g.get_ref().id) {
                        bail!("line {}: guild {} is bound twice", at(g.span()), g.get_ref().id);
                    }
                    guilds.push(g.get_ref().clone());
                }
                Ok((2, guilds))
            }
        }
    }

    /// A `[[channel]]`, checked, in its guild: its own `guild` (format 2),
    /// one a `[[guild]]` binds, or format 1's one.
    fn channel_of(
        c: RawChannel,
        format: u8,
        guilds: &[GuildBinding],
        text: &str,
    ) -> Result<ChannelBinding> {
        let id = c.id.get_ref().clone();
        snowflake("channel id", &id)?;
        let guild = match (&c.guild, format) {
            (Some(g), _) => {
                if !guilds.iter().any(|h| &h.id == g.get_ref()) {
                    bail!(
                        "line {}: channel {id} names guild {}, which no [[guild]] binds",
                        line(text, g.span()),
                        g.get_ref()
                    );
                }
                g.get_ref().clone()
            }
            (None, 1) => guilds[0].id.clone(),
            (None, _) => bail!(
                "line {}: channel {id} names no guild: in format 2 each [[channel]] says \
                 guild = \"<guild id>\", one a [[guild]] binds",
                line(text, c.id.span())
            ),
        };
        if c.users.is_empty() {
            bail!("channel {id} lists no users: nobody could drive it");
        }
        for u in &c.users {
            snowflake("user id", u)?;
        }
        let label = format!("#{}", c.name.as_deref().unwrap_or(&id));
        let ceiling = c.ceiling.map(|x| x.check(&label)).transpose()?.flatten();
        Ok(ChannelBinding {
            guild,
            id,
            name: c.name,
            users: c.users,
            mention_only: c.mention_only,
            private: c.private,
            voice: c.voice,
            ceiling,
            show_tools: c.show_tools,
            show_thinking: c.show_thinking,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_parses_and_has_a_revision() {
        let b = Bindings::parse(EXAMPLE_BINDINGS).unwrap();
        assert_eq!(b.format, 2, "the example shows format 2");
        assert_eq!(b.guilds.len(), 1);
        assert_eq!(b.channel.len(), 1);
        assert_eq!(b.dm.len(), 1);
        assert_eq!(b.channel[0].label(), "#theseus");
        assert_eq!(b.channel[0].guild, b.guilds[0].id);
        assert!(b.channel[0].mention_only, "mention_only defaults on");
        assert!(
            !b.guilds[0].private && !b.is_private(&b.channel[0]),
            "a channel is shared unless bound private"
        );
        assert_eq!(b.channel[0].ceiling, None, "the ceiling is commented");
        assert_eq!(b.dm[0].label(), "DM @zeroaltitude");
        assert_eq!(b.revision.len(), 12);
    }

    /// The example's commented ceiling, uncommented, is a real one.
    #[test]
    fn the_examples_ceiling_lines_are_real() {
        let example = EXAMPLE_BINDINGS
            .replacen("# [channel.ceiling]", "[channel.ceiling]  ", 1)
            .replacen("# posture_floor", "posture_floor  ", 1)
            .replacen("# tools", "tools  ", 1)
            .replacen("# spend_limit_usd", "spend_limit_usd  ", 1)
            .replacen("# profile", "profile  ", 1);
        let b = Bindings::parse(&example).unwrap();
        let c = b.channel[0].ceiling.clone().unwrap();
        assert_eq!(c.posture_floor.as_deref(), Some("approve"));
        assert_eq!(c.tools.unwrap(), ["fs", "git", "web"]);
        assert_eq!(c.spend_limit_usd, Some(5.0));
        assert_eq!(c.profile.as_deref(), Some("default"));
    }

    /// A place shows its tool lines and its thinking unless it says not to
    /// (theseus-l1y1): both on by default, each its own word, a channel's
    /// and a DM's; the example's lines, uncommented, are real.
    #[test]
    fn a_place_shows_tools_and_thinking_unless_it_says_not_to() {
        let b = Bindings::parse(EXAMPLE_BINDINGS).unwrap();
        let (c, d) = (&b.channel[0], &b.dm[0]);
        assert!(c.show_tools && c.show_thinking && d.show_tools && d.show_thinking);
        let example = EXAMPLE_BINDINGS
            .replacen("# show_tools = true ", "show_tools = false ", 1)
            .replacen("# show_thinking = true ", "show_thinking = true ", 2)
            .replacen("# show_tools = true ", "show_tools = true ", 1);
        let b = Bindings::parse(&example).unwrap();
        assert!(!b.channel[0].show_tools && b.channel[0].show_thinking);
        assert!(b.dm[0].show_tools && b.dm[0].show_thinking);
        let b = Bindings::parse("[[dm]]\nuser = \"323456789012345678\"\nshow_thinking = false\n")
            .unwrap();
        assert!(b.dm[0].show_tools && !b.dm[0].show_thinking);
    }

    /// A voice channel is a `[[channel]]` bound `voice = true` (rows 77 and
    /// 78); a channel is not one unless it says so.
    #[test]
    fn a_voice_channel_is_a_channel_bound_voice() {
        let b = Bindings::parse(
            "guild_id = \"123456789012345678\"\n\
             [[channel]]\nid = \"223456789012345678\"\nname = \"lounge\"\n\
             users = [\"323456789012345678\"]\nprivate = true\nvoice = true\n",
        )
        .unwrap();
        assert!(b.channel[0].voice && b.is_private(&b.channel[0]));
        assert_eq!(b.channel[0].label(), "#lounge");
        assert!(!Bindings::parse(EXAMPLE_BINDINGS).unwrap().channel[0].voice);
    }

    /// Three channels in one guild: `#lab` says nothing of `private`,
    /// `#hall` says `private = false`, and `#den` says `private = true`.
    /// `guild` is what the file says beside `guild_id`, if anything.
    fn three(guild: &str) -> String {
        format!(
            "guild_id = \"123456789012345678\"\n{guild}\n\
             [[channel]]\nid = \"223456789012345678\"\nname = \"lab\"\nusers = [\"323456789012345678\"]\n\
             [[channel]]\nid = \"223456789012345679\"\nname = \"hall\"\nusers = [\"323456789012345678\"]\nprivate = false\n\
             [[channel]]\nid = \"223456789012345680\"\nname = \"den\"\nusers = [\"323456789012345678\"]\nprivate = true\n"
        )
    }

    /// Each channel's class, and the channels read at the start, by label.
    fn classes(b: &Bindings) -> (Vec<(String, bool)>, Vec<String>) {
        let class = b.channel.iter().map(|c| (c.label(), b.is_private(c)));
        let read = b.read_at_start().map(ChannelBinding::label);
        (class.collect(), read.collect())
    }

    fn labelled(want: &[(&str, bool)]) -> Vec<(String, bool)> {
        want.iter().map(|(l, p)| (l.to_string(), *p)).collect()
    }

    /// theseus-rdqg: `private = true` beside `guild_id` is the operator's word
    /// that the whole guild is theirs. Every channel in it is private unless it
    /// says `private = false`, and none is read at the start. The example's
    /// commented line, uncommented, is that setting, in its `[[guild]]`.
    #[test]
    fn a_trusted_guilds_channels_are_private_unless_one_says_not() {
        let b = Bindings::parse(&three("private = true")).unwrap();
        assert_eq!((b.format, b.guilds[0].private), (1, true));
        let (class, read) = classes(&b);
        assert_eq!(
            class,
            labelled(&[("#lab", true), ("#hall", false), ("#den", true)])
        );
        assert!(
            read.is_empty(),
            "nothing is read in a trusted guild: {read:?}"
        );

        let example = EXAMPLE_BINDINGS.replacen("# private = true ", "private = true   ", 1);
        assert_ne!(example, EXAMPLE_BINDINGS, "the example shows the setting");
        let b = Bindings::parse(&example).unwrap();
        assert!(
            b.guilds[0].private && b.is_private(&b.channel[0]),
            "{example}"
        );
        assert_eq!(b.read_at_start().count(), 0);
    }

    /// An old file, without the guild's word, loads with the old meaning: a
    /// channel is private only when it says so, and each one that does is
    /// read at the start. Written below a `[[channel]]`, the guild's line is
    /// that channel's own (TOML's rule), which fails safe: the guild stays
    /// untrusted, so its other channels stay shared and that one is read.
    /// Below a `[[dm]]` the file is refused.
    #[test]
    fn an_old_file_keeps_its_meaning_and_a_misplaced_line_fails_safe() {
        let old = Bindings::parse(&three("")).unwrap();
        assert!(!old.guilds[0].private);
        let (class, read) = classes(&old);
        assert_eq!(
            class,
            labelled(&[("#lab", false), ("#hall", false), ("#den", true)])
        );
        assert_eq!(read, ["#den"]);

        let below = three("").replacen("name = \"lab\"\n", "name = \"lab\"\nprivate = true\n", 1);
        let b = Bindings::parse(&below).unwrap();
        assert!(!b.guilds[0].private, "the guild is not trusted");
        assert_eq!(
            classes(&b).1,
            ["#lab", "#den"],
            "each one read, and warned of"
        );
        let after_dm = format!(
            "{}[[dm]]\nuser = \"323456789012345678\"\nprivate = true\n",
            three("")
        );
        assert!(
            Bindings::parse(&after_dm).is_err(),
            "a [[dm]] has no private"
        );
    }

    /// Format 2: guild A trusted, with `#lab` (saying nothing) and `#hall`
    /// (`private = false`); guild B untrusted, with `#den` (`private = true`)
    /// and `#pier` (saying nothing); and a DM.
    const TWO: &str = "[[guild]]\nid = \"100000000000000001\"\nname = \"home\"\nprivate = true\n\
         [[guild]]\nid = \"100000000000000002\"\n\
         [[channel]]\nguild = \"100000000000000001\"\nid = \"223456789012345678\"\nname = \"lab\"\nusers = [\"323456789012345678\"]\n\
         [channel.ceiling]\nposture_floor = \"approve\"\n\
         [[channel]]\nguild = \"100000000000000001\"\nid = \"223456789012345679\"\nname = \"hall\"\nusers = [\"323456789012345678\"]\nprivate = false\n\
         [[channel]]\nguild = \"100000000000000002\"\nid = \"223456789012345680\"\nname = \"den\"\nusers = [\"323456789012345678\"]\nprivate = true\nvoice = true\n\
         [[channel]]\nguild = \"100000000000000002\"\nid = \"223456789012345681\"\nname = \"pier\"\nusers = [\"323456789012345678\"]\n\
         [channel.ceiling]\ntools = [\"web\"]\nspend_limit_usd = 1\n\
         [[dm]]\nuser = \"323456789012345678\"\nname = \"ana\"\n\
         [dm.ceiling]\nprofile = \"deep\"\n";

    /// Format 1 and format 2 both parse. In format 2 each guild has its own
    /// word: a trusted guild beside an untrusted one keeps each channel's
    /// class, and only the private channel outside the trusted guild is read
    /// at the start. Each place carries its guild and its ceiling.
    #[test]
    fn format_two_binds_many_guilds_each_with_its_own_word_and_ceilings() {
        let b = Bindings::parse(TWO).unwrap();
        assert_eq!(b.format, 2);
        let guilds: Vec<(&str, bool)> = b
            .guilds
            .iter()
            .map(|g| (g.id.as_str(), g.private))
            .collect();
        assert_eq!(
            guilds,
            [("100000000000000001", true), ("100000000000000002", false)]
        );
        assert_eq!(
            b.trusted().into_iter().collect::<Vec<_>>(),
            ["100000000000000001"]
        );
        let (class, read) = classes(&b);
        assert_eq!(
            class,
            labelled(&[
                ("#lab", true),
                ("#hall", false),
                ("#den", true),
                ("#pier", false)
            ])
        );
        assert_eq!(read, ["#den"], "the trusted guild's are never read");
        assert!(b.channel[2].voice, "a voice channel in the second guild");
        let ceiling = |i: usize| b.channel[i].ceiling.clone();
        assert_eq!(
            ceiling(0).unwrap().posture_floor.as_deref(),
            Some("approve")
        );
        assert_eq!(ceiling(1), None);
        let pier = ceiling(3).unwrap();
        assert_eq!(
            (pier.tools.unwrap(), pier.spend_limit_usd),
            (vec!["web".to_string()], Some(1.0))
        );
        assert_eq!(
            b.dm[0].ceiling.clone().unwrap().profile.as_deref(),
            Some("deep")
        );
        let infos = b.guild_infos();
        assert_eq!(
            (infos[0].name.as_deref(), infos[0].trusted),
            (Some("home"), true)
        );

        let one = Bindings::parse(&three("")).unwrap();
        assert_eq!((one.format, one.guilds.len()), (1, 1));
        assert!(one.channel.iter().all(|c| c.guild == "123456789012345678"));
        let dms = Bindings::parse("[[dm]]\nuser = \"323456789012345678\"\n").unwrap();
        assert_eq!(
            (dms.format, dms.guilds.len()),
            (2, 0),
            "a DM-only file needs no guild"
        );
    }

    /// A file that mixes the formats is refused, naming the line; so is a
    /// format-2 channel that names no guild, or one no `[[guild]]` binds.
    #[test]
    fn a_file_that_mixes_the_formats_is_refused_naming_the_line() {
        let refused = |text: &str| format!("{:#}", Bindings::parse(text).unwrap_err());
        let mixed = format!("guild_id = \"100000000000000009\"\n{TWO}");
        let e = refused(&mixed);
        assert!(
            e.contains("line 1: guild_id is format 1's, and [[guild]] (line "),
            "{e}"
        );
        let named = three("").replacen(
            "[[channel]]\n",
            "[[channel]]\nguild = \"123456789012345678\"\n",
            1,
        );
        let e = refused(&named);
        assert!(
            e.contains("line 1: guild_id is format 1's, and a channel's guild (line 4)"),
            "{e}"
        );
        let top = format!("private = true\n{TWO}");
        let e = refused(&top);
        assert!(
            e.contains("line 1: private at the top is format 1's word"),
            "{e}"
        );
        let unnamed = TWO.replacen("guild = \"100000000000000001\"\n", "", 1);
        let e = refused(&unnamed);
        assert!(
            e.contains("line 8: channel 223456789012345678 names no guild"),
            "{e}"
        );
        let stray = TWO.replacen(
            "guild = \"100000000000000002\"\nid = \"223456789012345681\"",
            "guild = \"100000000000000003\"\nid = \"223456789012345681\"",
            1,
        );
        let e = refused(&stray);
        assert!(
            e.contains("names guild 100000000000000003, which no [[guild]] binds"),
            "{e}"
        );
        let twice = TWO.replacen(
            "id = \"100000000000000002\"",
            "id = \"100000000000000001\"",
            1,
        );
        assert!(refused(&twice).contains("bound twice"));
    }

    /// A key in the wrong table fails safe: a ceiling's key written on its
    /// channel, a channel's key written in its ceiling, an unknown key, and a
    /// floor, a family, or a limit that is not one are refused; `private`
    /// below a `[[channel]]` is that channel's own, so its guild stays as it
    /// said; `private` in a `[channel.ceiling]` is refused.
    #[test]
    fn a_misplaced_or_bad_ceiling_key_is_refused_or_fails_safe() {
        let refused = |text: &str| format!("{:#}", Bindings::parse(text).unwrap_err());
        let floor_on_channel = TWO.replacen("[channel.ceiling]\nposture_floor", "posture_floor", 1);
        assert!(refused(&floor_on_channel).contains("posture_floor"));
        for (key, says) in [
            ("guild = \"100000000000000002\"\n", "guild"),
            ("private = true\n", "private"),
            ("bogus = 1\n", "bogus"),
            ("posture_floor = \"loose\"\n", "posture_floor \"loose\""),
            ("tools = [\"fs.read\"]\n", "tools entry \"fs.read\""),
            ("spend_limit_usd = 0\n", "spend_limit_usd 0"),
        ] {
            let text = TWO.replacen("tools = [\"web\"]\nspend_limit_usd = 1\n", key, 1);
            let e = refused(&text);
            assert!(e.contains(says), "{key}: {e}");
        }
        // `private = true` below #pier's channel line, before its ceiling:
        // #pier's own, and guild B stays untrusted.
        let own = TWO.replacen(
            "name = \"pier\"\nusers = [\"323456789012345678\"]\n",
            "name = \"pier\"\nusers = [\"323456789012345678\"]\nprivate = true\n",
            1,
        );
        let b = Bindings::parse(&own).unwrap();
        assert!(!b.guilds[1].private);
        assert_eq!(classes(&b).1, ["#den", "#pier"]);
        // An empty ceiling narrows nothing.
        let empty = TWO.replacen("[dm.ceiling]\nprofile = \"deep\"\n", "[dm.ceiling]\n", 1);
        assert_eq!(Bindings::parse(&empty).unwrap().dm[0].ceiling, None);
    }

    #[test]
    fn bad_files_are_refused_with_the_reason() {
        let e = Bindings::parse("guild_id = \"12\"\n[[dm]]\nuser = \"123456789012345678\"\n")
            .unwrap_err();
        assert!(e.to_string().contains("not a Discord id"), "{e}");
        let e = Bindings::parse("guild_id = \"123456789012345678\"\n").unwrap_err();
        assert!(e.to_string().contains("nothing to bind"), "{e}");
        let e = Bindings::parse(
            "guild_id = \"123456789012345678\"\n[[channel]]\nid = \"123456789012345678\"\nusers = []\n",
        )
        .unwrap_err();
        assert!(e.to_string().contains("nobody could drive it"), "{e}");
        assert!(Bindings::parse("guild_id = \"123456789012345678\"\nbogus = 1\n").is_err());
        let e = Bindings::parse("[[guild]]\nid = \"12\"\n[[dm]]\nuser = \"123456789012345678\"\n")
            .unwrap_err();
        assert!(e.to_string().contains("not a Discord id"), "{e}");
    }
}
