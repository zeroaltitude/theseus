//! The bindings file (spec P5): which Discord places Theseus lives in and who
//! may drive it there. Its SHA-256 prefix is the binding revision.

use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Printed by `theseusd example-bindings`; parsed by a test so it cannot drift.
pub const EXAMPLE_BINDINGS: &str = include_str!("../bindings.example.toml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub guild_id: String,
    /// The operator's word that the whole guild is theirs (theseus-rdqg): every
    /// `[[channel]]` in it is private unless it says `private = false`, and
    /// who can view one is never read, since the word covers the guild. Off
    /// by default: a channel is private only when it says so.
    #[serde(default)]
    pub private: bool,
    #[serde(default)]
    pub channel: Vec<ChannelBinding>,
    #[serde(default)]
    pub dm: Vec<DmBinding>,
    /// First 12 hex of the file's SHA-256.
    #[serde(skip)]
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChannelBinding {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub users: Vec<String>,
    /// Answer only messages that @mention Theseus or reply to one of its
    /// messages. On by default: a channel shared with people or other bots
    /// should not get a turn per message.
    #[serde(default = "yes")]
    pub mention_only: bool,
    /// The operator's word that only the owner can view it (the place rule,
    /// theseus-nbsh): its session gets everything, as a DM with the owner
    /// does. Absent, the guild's word decides (`private` beside `guild_id`,
    /// theseus-rdqg), and that is off by default: a guild channel is a shared
    /// place, with the public tools alone. Outside a trusted guild a channel
    /// bound private is read once at the binding's start, and health warns
    /// when anyone besides the owner can view it. Read it through
    /// `Bindings::is_private`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private: Option<bool>,
    /// A voice channel (rows 77 and 78): `/join`, from a private place,
    /// brings Theseus into it when `[voice]` is on, and only its users are
    /// heard. Its text chat is the place's text, as a text channel's is.
    #[serde(default)]
    pub voice: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DmBinding {
    pub user: String,
    #[serde(default)]
    pub name: Option<String>,
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

impl Bindings {
    /// Whether the channel `c` is private: its own word, else its guild's
    /// (theseus-rdqg).
    pub fn is_private(&self, c: &ChannelBinding) -> bool {
        c.private.unwrap_or(self.private)
    }

    /// The channels whose viewers the binding reads at its start (the place
    /// rule's one check): each one bound private, outside a trusted guild.
    /// In one, none: the operator's word covers the guild (theseus-rdqg).
    pub fn read_at_start(&self) -> impl Iterator<Item = &ChannelBinding> {
        self.channel
            .iter()
            .filter(|c| !self.private && self.is_private(c))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading bindings file {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("bindings file {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut b: Bindings = toml::from_str(text)?;
        snowflake("guild_id", &b.guild_id)?;
        if b.channel.is_empty() && b.dm.is_empty() {
            bail!("no [[channel]] and no [[dm]]: nothing to bind");
        }
        for c in &b.channel {
            snowflake("channel id", &c.id)?;
            if c.users.is_empty() {
                bail!("channel {} lists no users: nobody could drive it", c.id);
            }
            for u in &c.users {
                snowflake("user id", u)?;
            }
        }
        for d in &b.dm {
            snowflake("dm user", &d.user)?;
        }
        let digest = Sha256::digest(text.as_bytes());
        b.revision = hex::encode(digest)[..12].to_string();
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_parses_and_has_a_revision() {
        let b = Bindings::parse(EXAMPLE_BINDINGS).unwrap();
        assert_eq!(b.channel.len(), 1);
        assert_eq!(b.dm.len(), 1);
        assert_eq!(b.channel[0].label(), "#theseus");
        assert!(b.channel[0].mention_only, "mention_only defaults on");
        assert!(
            !b.private && !b.is_private(&b.channel[0]),
            "a channel is shared unless bound private"
        );
        assert_eq!(b.dm[0].label(), "DM @eddie");
        assert_eq!(b.revision.len(), 12);
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

    /// theseus-rdqg: `private = true` beside `guild_id` is the operator's word
    /// that the whole guild is theirs. Every channel in it is private unless it
    /// says `private = false`, and none is read at the start. The example's
    /// commented line, uncommented, is that setting.
    #[test]
    fn a_trusted_guilds_channels_are_private_unless_one_says_not() {
        let b = Bindings::parse(&three("private = true")).unwrap();
        assert!(b.private);
        let (class, read) = classes(&b);
        let want = [("#lab", true), ("#hall", false), ("#den", true)];
        let want: Vec<(String, bool)> = want.iter().map(|(l, p)| (l.to_string(), *p)).collect();
        assert_eq!(class, want);
        assert!(
            read.is_empty(),
            "nothing is read in a trusted guild: {read:?}"
        );

        let example = EXAMPLE_BINDINGS.replacen("# private = true ", "private = true   ", 1);
        assert_ne!(example, EXAMPLE_BINDINGS, "the example shows the setting");
        let b = Bindings::parse(&example).unwrap();
        assert!(b.private && b.is_private(&b.channel[0]), "{example}");
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
        assert!(!old.private);
        let (class, read) = classes(&old);
        let want = [("#lab", false), ("#hall", false), ("#den", true)];
        let want: Vec<(String, bool)> = want.iter().map(|(l, p)| (l.to_string(), *p)).collect();
        assert_eq!(class, want);
        assert_eq!(read, ["#den"]);

        let below = three("").replacen("name = \"lab\"\n", "name = \"lab\"\nprivate = true\n", 1);
        let b = Bindings::parse(&below).unwrap();
        assert!(!b.private, "the guild is not trusted");
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
    }
}
