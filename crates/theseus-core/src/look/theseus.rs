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
/// What may be asked for: the four, and the ontology, which only a call that names it gets (it lists
/// the owner's topics and people, so it is not part of "everything"); and this turn's view (issue 44,
/// `look/view.rs`), which the look reads with the turn's own presence.
const ASKABLE: [&str; 6] = [
    "profiles",
    "providers",
    "health",
    "build",
    "ontology",
    "view",
];

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
        let part = ASKABLE.iter().find(|p| **p == w.trim()).ok_or_else(|| {
            format!(
                "`{w}` is not a part of the look: choose from {}",
                ASKABLE.join(", ")
            )
        })?;
        if !out.contains(part) {
            out.push(*part);
        }
    }
    out.sort_by_key(|p| ASKABLE.iter().position(|x| x == p));
    Ok(out)
}

/// The words for each part asked for.
pub(super) fn words(core: &Core, target: Option<&Target>, parts: &[&str]) -> String {
    let mut out = String::new();
    for p in parts {
        let part = match *p {
            // Read with the turn's presence, by the look itself (`look/view.rs`).
            "view" => continue,
            "profiles" => profiles(core, target),
            "providers" => providers(core),
            "health" => health(core),
            "ontology" => ontology(core),
            _ => build(core),
        };
        out.push_str(&part);
        out.push('\n');
    }
    out
}

/// The most of each list the ontology part gives.
const KINDS_MAX: usize = 12;
const CATEGORIES_MAX: usize = 40;
const PEOPLE_MAX: usize = 30;
const PROPOSALS_MAX: u32 = 10;

/// The ontology (issue 28, step 5), as `theseus ontology kinds|categories|proposals` read it: the kinds
/// table, the category tree, the people the ontology holds, and Jev's unanswered proposals.
///
/// **What it never gives**: a person's handles (their Discord ids, addresses and names in other
/// places), a session's memberships (which conversation is in which category), a proposal's session
/// or its title, and the guidance text. A category is its name, kind, depth and how many sessions it
/// holds; a person is their name and how many; a proposal is the topic and Jev's confidence and band.
/// Every list is bounded and says what it left out.
fn ontology(core: &Core) -> String {
    use theseus_protocol::{OntologyListParams, OntologyProposalsParams};
    let list = match core.ontology_list(&OntologyListParams {
        session_id: None,
        memberships: Some(false),
    }) {
        Ok(l) => l,
        Err(_) => return "Ontology: it could not be read just now.\n".into(),
    };
    let mut out = format!("Ontology: {} kinds.\n", list.kinds.len());
    for k in list.kinds.iter().take(KINDS_MAX) {
        let _ = writeln!(
            out,
            "- kind {} · {} · {} per session · precedence {} · rule {}",
            clean(&k.name),
            k.basis,
            k.per_session,
            k.precedence,
            k.rule
        );
    }
    let (people, topics): (Vec<_>, Vec<_>) =
        list.categories.iter().partition(|c| c.kind == "person");
    let _ = writeln!(out, "Categories ({}, in their tree):", topics.len());
    for c in topics.iter().take(CATEGORIES_MAX) {
        let _ = writeln!(
            out,
            "{}- {} {} · {} session{}{}",
            "  ".repeat(c.depth.saturating_sub(1) as usize),
            clean(&c.kind),
            clean(&c.name),
            c.members,
            if c.members == 1 { "" } else { "s" },
            if c.guidance.is_some() {
                " · has guidance"
            } else {
                ""
            }
        );
    }
    if topics.len() > CATEGORIES_MAX {
        let _ = writeln!(out, "… and {} more", topics.len() - CATEGORIES_MAX);
    }
    let _ = writeln!(
        out,
        "People held ({}; handles are not shown):",
        people.len()
    );
    for c in people.iter().take(PEOPLE_MAX) {
        let _ = writeln!(
            out,
            "- {} · {} session{}",
            clean(&c.name),
            c.members,
            if c.members == 1 { "" } else { "s" }
        );
    }
    if people.len() > PEOPLE_MAX {
        let _ = writeln!(out, "… and {} more", people.len() - PEOPLE_MAX);
    }
    match core.ontology_proposals(&OntologyProposalsParams {
        limit: Some(PROPOSALS_MAX),
        ..Default::default()
    }) {
        Err(_) => out.push_str("Proposals: they could not be read just now.\n"),
        Ok(r) if r.proposals.is_empty() => out.push_str("Proposals: none wait.\n"),
        Ok(r) => {
            let _ = writeln!(
                out,
                "Proposals of Jev's that wait for the owner ({} shown, {} more, {} hidden); only they accept or reject:",
                r.proposals.len(),
                r.more,
                r.hidden
            );
            for p in &r.proposals {
                let what = match (&p.person, &p.topic_name, p.new_topic) {
                    (Some(_), _, _) => "a person".to_string(),
                    (_, Some(t), _) => clean(t),
                    (_, _, true) => "a new topic".to_string(),
                    _ => "a topic".to_string(),
                };
                let _ = writeln!(out, "- {what} · {:.2} · {}", p.confidence, p.band);
            }
        }
    }
    out
}

/// A name as one short line (a title or a name is the owner's own words, and may hold newlines).
fn clean(s: &str) -> String {
    crate::secrets::look::clean(s)
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
                "description": "Parts: profiles, providers, health, build, ontology, view (this turn's context). Omit: all but ontology, view.",
                "items": {"type": "string", "enum": ASKABLE}
            }
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests;
