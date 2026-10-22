//! `[cockpit]` (theseus-x9swu): where a client sends a person to look at a
//! turn. Every key has a default, so an owner's config names none of them
//! unless it differs.
//!
//! - `public_url`: the address a browser reaches the cockpit at, the base of
//!   the link in a reply's footer (`<public_url>/session/<session>/turn/<turn>`).
//!   Left out, it is `http://localhost:<[web] port>`, resolved once at load
//!   (`resolve`); set it where a relay forwards another port to the daemon.
//!   It carries no key or token, and the cockpit asks for its own sign-in.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CockpitConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_url: Option<String>,
    /// The link's base, settled by `resolve`: never read from a note. None while the web UI is
    /// off: there is nothing to link to.
    #[serde(skip, default = "localhost_base")]
    base: Option<String>,
}

fn localhost_base() -> Option<String> {
    Some(local_base(super::default_web_port()))
}

fn local_base(port: u16) -> String {
    format!("http://localhost:{port}")
}

impl Default for CockpitConfig {
    fn default() -> Self {
        Self {
            public_url: None,
            base: localhost_base(),
        }
    }
}

impl CockpitConfig {
    pub fn is_default(&self) -> bool {
        self.public_url.is_none()
    }

    /// Settle the link's base once the note is read: `public_url`, else
    /// localhost on the web UI's port, and none when the web UI is off.
    pub fn resolve(&mut self, web_port: u16, web_enabled: bool) {
        self.base = web_enabled.then(|| local_base(web_port));
    }

    pub fn validate(&self) -> Result<()> {
        let Some(u) = &self.public_url else {
            return Ok(());
        };
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            bail!("cockpit.public_url {u:?} must start with http:// or https://");
        }
        if u.contains('@') || u.contains('?') || u.contains('#') || u.contains(char::is_whitespace)
        {
            bail!("cockpit.public_url {u:?} must be a bare address: no login, query, fragment or space");
        }
        Ok(())
    }

    /// The link a reply's footer carries: none when no web UI runs and no `public_url` names one.
    pub fn turn_link(&self, session_id: &str, turn_id: &str) -> Option<String> {
        let base = self.public_url.as_deref().or(self.base.as_deref())?;
        Some(theseus_protocol::inspect::turn_url(
            base, session_id, turn_id,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The template's `[cockpit]` is the default, a note may change it, and
    /// the link is built from the configured base: `public_url` when set,
    /// else localhost on `[web] port`.
    #[test]
    fn the_templates_cockpit_section_is_the_default_and_the_link_follows_it() {
        let cfg = crate::Config::from_toml(crate::Config::EXAMPLE_TOML).unwrap();
        assert_eq!(cfg.cockpit, CockpitConfig::default());
        assert!(crate::Config::EXAMPLE_TOML.contains("# [cockpit]\n# public_url = "));
        assert_eq!(
            cfg.cockpit.turn_link("ses_a", "turn_b").as_deref(),
            Some("http://localhost:7433/session/ses_a/turn/turn_b")
        );
        let relay =
            crate::Config::from_toml("[cockpit]\npublic_url = \"http://localhost:17433/\"\n")
                .unwrap();
        assert_eq!(
            relay.cockpit.turn_link("ses_a", "turn_b").as_deref(),
            Some("http://localhost:17433/session/ses_a/turn/turn_b")
        );
        let ported = crate::Config::from_toml("[web]\nport = 17433\n").unwrap();
        assert!(ported.cockpit.is_default());
        assert_eq!(
            ported.cockpit.turn_link("ses_a", "turn_b").as_deref(),
            Some("http://localhost:17433/session/ses_a/turn/turn_b")
        );
        let both = crate::Config::from_toml(
            "[web]\nport = 17433\n[cockpit]\npublic_url = \"http://localhost:9000\"\n",
        )
        .unwrap();
        assert_eq!(
            both.cockpit.turn_link("ses_a", "turn_b").as_deref(),
            Some("http://localhost:9000/session/ses_a/turn/turn_b")
        );
        let no_web = crate::Config::from_toml("[web]\nenabled = false\nport = 17433\n").unwrap();
        assert_eq!(no_web.cockpit.turn_link("ses_a", "turn_b"), None);
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

    /// A reply links the cockpit only where one runs: web off and no public URL gives no link;
    /// web on gives the local one; a public URL gives it whatever web says.
    #[test]
    fn no_inspect_link_without_a_web_ui() {
        let off = crate::Config::from_toml("[web]\nenabled = false\n").unwrap();
        assert_eq!(off.cockpit.turn_link("ses_a", "turn_b"), None);
        let on = crate::Config::from_toml("[web]\nport = 17433\n").unwrap();
        assert_eq!(
            on.cockpit.turn_link("ses_a", "turn_b").as_deref(),
            Some("http://localhost:17433/session/ses_a/turn/turn_b")
        );
        let public = crate::Config::from_toml(
            "[web]\nenabled = false\n[cockpit]\npublic_url = \"http://localhost:9000\"\n",
        )
        .unwrap();
        assert_eq!(
            public.cockpit.turn_link("ses_a", "turn_b").as_deref(),
            Some("http://localhost:9000/session/ses_a/turn/turn_b")
        );
        assert!(CockpitConfig::default()
            .turn_link("ses_a", "turn_b")
            .is_some());
    }
}
