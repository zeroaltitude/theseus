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
    /// does. Off by default: a guild channel is a shared place, with the
    /// public tools alone. Read once at the binding's start, and health warns
    /// when anyone besides the owner can view it.
    #[serde(default)]
    pub private: bool,
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
            !b.channel[0].private,
            "a channel is shared unless bound private"
        );
        assert_eq!(b.dm[0].label(), "DM @eddie");
        assert_eq!(b.revision.len(), 12);
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
