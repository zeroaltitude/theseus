//! `look.theseus`: Theseus tells the owner's own turn what it is running as
//! (issue 26): the profiles and the one that is live, the providers and
//! whether each one's key resolved, the daemon's health and the build.
//!
//! A turn used to run Theseus's own command line for this, and the shell is
//! the wrong way to look: a command that names a program the floor protects is
//! held at every setting, and a card for a look is a question the owner should
//! not be asked. The daemon knows all of it already; this words it.
//!
//! **What it never gives.** A key's value, the vault item it comes from, and
//! the text of a failure: a provider's key is `ready`, `still resolving` or
//! `not resolved`, and where it is read from is `vault`, `environment` or
//! `file`, as health says it. Paths, references and the settings file's text
//! are not here; `config.show` reads settings.

use std::fmt::Write;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::secrets::{SecretState, Source};
use crate::turn::Target;
use crate::Core;

/// The parts of the look, in the order they are given.
const PARTS: [&str; 4] = ["profiles", "providers", "health", "build"];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    #[serde(default)]
    what: Option<Vec<String>>,
}

/// The parts asked for: all of them when none is named.
pub(super) fn parts_of(input: &Value) -> Result<Vec<&'static str>, String> {
    let i: Input = serde_json::from_value(input.clone()).map_err(|e| format!("bad input: {e}"))?;
    let Some(what) = i.what.filter(|w| !w.is_empty()) else {
        return Ok(PARTS.to_vec());
    };
    let mut out = Vec::new();
    for w in &what {
        let part = PARTS.iter().find(|p| **p == w.trim()).ok_or_else(|| {
            format!(
                "`{w}` is not a part of the look: choose from {}",
                PARTS.join(", ")
            )
        })?;
        if !out.contains(part) {
            out.push(*part);
        }
    }
    out.sort_by_key(|p| PARTS.iter().position(|x| x == p));
    Ok(out)
}

/// The words for each part asked for.
pub(super) fn words(core: &Core, target: Option<&Target>, parts: &[&str]) -> String {
    let mut out = String::new();
    for p in parts {
        let part = match *p {
            "profiles" => profiles(core, target),
            "providers" => providers(core),
            "health" => health(core),
            _ => build(core),
        };
        out.push_str(&part);
        out.push('\n');
    }
    out
}

fn profiles(core: &Core, target: Option<&Target>) -> String {
    let list = core.profile_list();
    let mut out = format!(
        "Profiles (the live one is {}: {}):\n",
        list.live, list.live_source
    );
    for p in &list.profiles {
        let _ = write!(
            out,
            "- {}{} · {} · {}",
            p.name,
            if p.live { " (live)" } else { "" },
            p.provider,
            p.model
        );
        if let Some(e) = &p.effort {
            let _ = write!(out, " · effort {e}");
        }
        if let Some(o) = &p.origin {
            let _ = write!(out, " · origin {o}");
        }
        out.push('\n');
    }
    if let Some(t) = target {
        let _ = writeln!(
            out,
            "This conversation runs on {} ({}).",
            t.profile, t.model
        );
    }
    out
}

/// How a provider's key stands, from the board: never the value or why.
fn key_state(core: &Core, secret: &str) -> &'static str {
    match core.secrets.states().get(secret) {
        Some(SecretState::Ready(_)) => "ready",
        Some(SecretState::Resolving) => "still resolving",
        Some(SecretState::Failed(_)) => "not resolved",
        None => "not configured",
    }
}

fn source_of(core: &Core, secret: &str) -> &'static str {
    match core.cfg.secrets.get(secret).map(|r| Source::of(r)) {
        Some(Source::Env) => "environment",
        Some(Source::File) => "file",
        Some(Source::Vault) => "vault",
        None => "nowhere",
    }
}

fn providers(core: &Core) -> String {
    let mut out =
        String::from("Providers (each one's key: its state and where it is read from):\n");
    for (name, p) in &core.cfg.providers {
        let _ = writeln!(
            out,
            "- {name}: key {} · from the {}",
            key_state(core, &p.api_key_secret),
            source_of(core, &p.api_key_secret)
        );
    }
    out
}

fn health(core: &Core) -> String {
    let h = core.health();
    let mut out = String::from("Health:\n");
    let _ = writeln!(
        out,
        "- up {} s · {} sessions · {} turns · {} provider errors · {} ledger rows",
        h.uptime_secs, h.sessions, h.turns, h.provider_errors, h.ledger_rows
    );
    let _ = writeln!(
        out,
        "- kernel {} · turns held {}/{} · spend limit ${:.2} a session",
        if h.kernel.accepting {
            "accepting"
        } else {
            "not accepting"
        },
        h.kernel.turns_held,
        h.kernel.admission_ceiling,
        h.kernel.spend_limit_usd
    );
    let failed: Vec<&str> = h.secrets.failed.iter().map(|f| f.name.as_str()).collect();
    let _ = writeln!(
        out,
        "- secrets {} ({} ready, {} resolving{})",
        h.secrets.state,
        h.secrets.ready.len(),
        h.secrets.resolving.len(),
        match failed.is_empty() {
            true => String::new(),
            false => format!(", not resolved: {}", failed.join(", ")),
        }
    );
    let _ = writeln!(out, "- settings: {} ({})", h.config.source, h.config.state);
    out
}

fn build(core: &Core) -> String {
    let h = core.health();
    let mut out = format!("Build: Theseus {} · protocol {}", h.version, h.protocol);
    if let Some(c) = &h.build.commit {
        let _ = write!(out, " · commit {c}");
    }
    out.push('\n');
    if let Some(j) = &h.joined_not_running {
        let _ = writeln!(out, "{j}");
    }
    out
}

/// The tool's input schema.
pub(super) fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "what": {
                "type": "array",
                "description": "Which parts: profiles, providers, health, build. Omit for all.",
                "items": {"type": "string", "enum": PARTS}
            }
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests;
