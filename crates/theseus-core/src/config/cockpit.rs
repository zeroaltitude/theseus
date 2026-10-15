//! `[cockpit]` (theseus-x9swu): where a client sends a person to look at a
//! turn. Every key has a default, so an owner's config names none of them
//! unless it differs.
//!
//! - `public_url`: the address a browser reaches the cockpit at
//!   (`http://localhost:7433`), the base of the link in a reply's footer
//!   (`<public_url>/session/<session>/turn/<turn>`). It differs from the
//!   daemon's own bind where a relay forwards another port to it. It carries
//!   no key or token, and the cockpit asks for its own sign-in.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CockpitConfig {
    #[serde(default = "default_public_url")]
    pub public_url: String,
}

fn default_public_url() -> String {
    "http://localhost:7433".into()
}

impl Default for CockpitConfig {
    fn default() -> Self {
        Self {
            public_url: default_public_url(),
        }
    }
}

impl CockpitConfig {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<()> {
        let u = &self.public_url;
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            bail!("cockpit.public_url {u:?} must start with http:// or https://");
        }
        if u.contains('@') || u.contains('?') || u.contains('#') || u.contains(char::is_whitespace)
        {
            bail!("cockpit.public_url {u:?} must be a bare address: no login, query, fragment or space");
        }
        Ok(())
    }

    /// The link to one turn: the base, `/session/<session>/turn/<turn>`.
    pub fn turn_url(&self, session_id: &str, turn_id: &str) -> String {
        theseus_protocol::inspect::turn_url(&self.public_url, session_id, turn_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[cockpit]` is the default, a note may change it, and
    /// the link is built from the configured base.
    #[test]
    fn the_templates_cockpit_section_is_the_default_and_the_link_follows_it() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.cockpit, CockpitConfig::default());
        assert!(crate::Config::EXAMPLE_TOML.contains("# [cockpit]\n# public_url = "));
        assert_eq!(
            cfg.cockpit.turn_url("ses_a", "turn_b"),
            "http://localhost:7433/session/ses_a/turn/turn_b"
        );
        let relay =
            crate::Config::from_toml("[cockpit]\npublic_url = \"http://localhost:17433/\"\n")
                .unwrap();
        assert_eq!(
            relay.cockpit.turn_url("ses_a", "turn_b"),
            "http://localhost:17433/session/ses_a/turn/turn_b"
        );
        let secrets = "[secrets]\nanthropic_api_key = \"op://v/i/f\"\n";
        for bad in [
            "public_url = \"localhost:7433\"",
            "public_url = \"http://u:p@localhost:7433\"",
            "public_url = \"http://localhost:7433/?key=1\"",
        ] {
            assert!(crate::Config::parse(&format!("{secrets}\n[cockpit]\n{bad}\n")).is_err());
        }
        assert!(crate::Config::from_toml("[cockpit]\nbogus = 1\n").is_err());
    }
}
