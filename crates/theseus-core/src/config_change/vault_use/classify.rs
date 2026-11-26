//! Which of two paths a `vault.use` takes (issue 54, the conductor's rule of round 4), decided by
//! a POSITIVE list and nothing else. There is no use without a card.
//!
//! 1. [`Case::Pure`]: the secret's own entry (new, or re-pointed) and settings that are, by the
//!    list in [`is_reader`], real readers of a secret name, each set to exactly that name: ONE
//!    settings card with no second proof (the owner: using a secret takes no authenticator code).
//!    A new name with nothing else is pure too: a setting may read that name by default (the
//!    voice's key, with its section left out of the file, is read by an audition), so it is never
//!    written unasked.
//! 2. [`Case::Other`]: anything else, which takes the settings card with the proof
//!    `config.propose` would ask for the whole change.
//!
//! The list is decided by the shape of a setting's path, never by a freely named map component:
//! a provider or a server is named by its key, so a key such as `key` or `x_secret` changes
//! nothing here. A reader is listed only where what it reads goes to a place that is already
//! configured and does not change: a provider that already exists (its address is not touched,
//! since nothing else may ride a pure use), the main model's key, and an MCP server's
//! `auth_secret` for a server that already exists with a URL. An MCP server's `env`, anything of
//! a new server or provider, and every other setting are not on it.

use serde_json::Value;

use super::super::diff::Setting;
use crate::Config;

/// The path a use takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Case {
    Pure,
    Other,
}

/// Which path a use of the secret `name` takes when it changes `settings` of the file `base`.
pub fn case_of(base: &Config, settings: &[Setting], name: &str) -> Case {
    let pure = !settings.is_empty()
        && settings
            .iter()
            .all(|s| own(s, name) || is_reader(base, s, name));
    match pure {
        true => Case::Pure,
        false => Case::Other,
    }
}

/// Whether `s` is the secret's own entry, `secrets.<name>`, set to a reference.
fn own(s: &Setting, name: &str) -> bool {
    s.path.len() == 2
        && s.path[0] == "secrets"
        && s.path[1] == name
        && matches!(s.after, Some(Value::String(_)))
}

/// Whether `s` sets a setting on the list of readers of a secret name to exactly `name`.
pub fn is_reader(base: &Config, s: &Setting, name: &str) -> bool {
    if s.after != Some(Value::String(name.to_string())) {
        return false;
    }
    let p: Vec<&str> = s.path.iter().map(String::as_str).collect();
    match p.as_slice() {
        ["model", "api_key_secret"] => true,
        ["providers", provider, "api_key_secret"] => base.providers.contains_key(*provider),
        ["mcp", "servers", server, "auth_secret"] => base
            .mcp
            .servers
            .get(*server)
            .is_some_and(|c| c.url.is_some()),
        _ => false,
    }
}
