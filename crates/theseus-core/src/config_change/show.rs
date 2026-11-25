//! `config.show`: the owner's own turn reads Theseus's settings by name (issue 22).
//!
//! Asked to change a setting, a turn used to go to the shell to find out what it
//! was now, and the file is held by the floor, so the owner was asked about the
//! look before the card. This is the look the daemon does itself: for each
//! setting named, the value now, whether the file sets it or it is the default,
//! whether a change to it is sensitive and why, and whether it restarts.
//!
//! **What it gives and what it does not.** It reads the file the daemon would
//! start on, parsed and checked, with every default filled in, so it never
//! shows text, comments or order, only settings. It never gives:
//!
//! - anything in `[secrets]` or `[broker]`: the names of vault items and where
//!   each secret is read from. The card, which the daemon builds for the owner,
//!   shows them; a turn has no need to have seen them;
//! - a value under a key that names where a secret is read from (`env`,
//!   `token`, `*_secret`, `credentials`, and the rest of
//!   `sensitivity::key_reason`'s first class);
//! - any value that is a vault reference (`op://`) or a URL that carries a
//!   user, a password or a query;
//! - a value longer than a card shows whole: it says how long, not its start.
//!
//! For each of these it still says whether the setting is set, whether a change
//! is sensitive and whether it restarts. The wall is `config.history`'s: the
//! owner, alone in a private place, in their own session. A turn that has read
//! text from outside since the owner's last message is asked, not free: the
//! gate puts the call to the owner (`ask_if_outside`), so a turn that was
//! tricked cannot map the owner's permissions for nothing.

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_tools::{parse, Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::{diff, edit, history, wall};
use crate::config::sensitivity;
use crate::toolrun::{ToolRuntime, TurnCtx};

pub const SHOW: &str = "config.show";
/// The most paths one call names.
pub const MAX_PATHS: usize = 20;
/// The most settings one call lists, a table's settings included.
pub const MAX_LISTED: usize = 60;
/// What a withheld value is said to be.
const WITHHELD: &str = "its value is not shown to Theseus (it is where a secret is read from)";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    paths: Vec<String>,
}

fn paths_of(input: &Value) -> Result<Vec<Vec<String>>, String> {
    let i: Input = parse(input)?;
    if i.paths.is_empty() || i.paths.len() > MAX_PATHS {
        return Err(format!("name between 1 and {MAX_PATHS} settings to read"));
    }
    i.paths.iter().map(|p| edit::parse_path(p)).collect()
}

/// The settings as Theseus holds them: the file's text, loaded and checked as
/// a start loads it, as a tree with the defaults filled in. A table the loader
/// leaves out of what it writes when it is all defaults is put back from the
/// parsed config, so a default is always a value here.
pub(crate) fn effective(text: &str) -> Result<Value, String> {
    let (cfg, _) = crate::Config::parse(text)
        .map_err(|e| format!("the settings file as it is now does not load: {e:#}"))?;
    let mut v =
        serde_json::to_value(&cfg).map_err(|e| format!("the settings cannot be read: {e}"))?;
    let tables: Vec<(&str, serde_json::Result<Value>)> = vec![
        ("context", serde_json::to_value(&cfg.context)),
        ("broker", serde_json::to_value(&cfg.broker)),
        ("voice", serde_json::to_value(&cfg.voice)),
        ("aws", serde_json::to_value(&cfg.aws)),
        ("google", serde_json::to_value(&cfg.google)),
        ("memory", serde_json::to_value(&cfg.memory)),
        ("routing", serde_json::to_value(&cfg.routing)),
        ("privacy", serde_json::to_value(&cfg.privacy)),
        (
            "content_based_security",
            serde_json::to_value(&cfg.content_based_security),
        ),
        ("mcp", serde_json::to_value(&cfg.mcp)),
        ("mcp_server", serde_json::to_value(&cfg.mcp_server)),
        ("lsp", serde_json::to_value(&cfg.lsp)),
        ("sessions", serde_json::to_value(&cfg.sessions)),
        ("routines", serde_json::to_value(&cfg.routines)),
        ("route_bench", serde_json::to_value(&cfg.route_bench)),
        ("persona_import", serde_json::to_value(&cfg.persona_import)),
        ("project", serde_json::to_value(&cfg.project)),
        ("projects", serde_json::to_value(&cfg.projects)),
        ("standing", serde_json::to_value(&cfg.standing)),
        ("threads", serde_json::to_value(&cfg.threads)),
        ("initiative", serde_json::to_value(&cfg.initiative)),
        ("watch_for", serde_json::to_value(&cfg.watch_for)),
        ("cockpit", serde_json::to_value(&cfg.cockpit)),
        ("notify", serde_json::to_value(&cfg.notify)),
        ("cache", serde_json::to_value(&cfg.cache)),
        ("people", serde_json::to_value(&cfg.people)),
        ("card", serde_json::to_value(&cfg.card)),
        ("ontology", serde_json::to_value(&cfg.ontology)),
        ("import", serde_json::to_value(&cfg.import)),
        ("self", serde_json::to_value(&cfg.self_improve)),
        ("self_issues", serde_json::to_value(&cfg.self_issues)),
        ("places", serde_json::to_value(&cfg.places)),
    ];
    for (k, x) in tables {
        if let (None, Ok(x)) = (v.get(k), x) {
            v[k] = x;
        }
    }
    Ok(v)
}

/// Whether a string is a vault reference, or a URL that carries a credential.
fn credential_like(s: &str) -> bool {
    if s.starts_with("op://") {
        return true;
    }
    match reqwest::Url::parse(s) {
        Ok(u) => !u.username().is_empty() || u.password().is_some() || u.query().is_some(),
        Err(_) => false,
    }
}

/// Why the value at `path` is not given, or `None` when it is. Judged by where
/// the setting is, by what its key says and by what the value is, so a secret
/// kept somewhere nobody classified is still not read out.
fn withheld(path: &[String], v: &Value) -> Option<&'static str> {
    if matches!(path.first().map(String::as_str), Some("secrets" | "broker")) {
        return Some(WITHHELD);
    }
    if path
        .iter()
        .any(|k| sensitivity::key_reason(k) == Some("where a secret is read from"))
    {
        return Some(WITHHELD);
    }
    let carries = match v {
        Value::String(s) => credential_like(s),
        Value::Array(a) => a.iter().any(|x| x.as_str().is_some_and(credential_like)),
        _ => false,
    };
    carries.then_some(WITHHELD)
}

/// A value as a line says it: whole, in words, or how long it is.
fn shown(v: &Value) -> String {
    let w = diff::words(Some(v));
    match w.chars().count() > diff::SHOWN {
        true => format!(
            "a long value ({} characters; a change to it shows the owner all of it)",
            w.chars().count()
        ),
        false => w,
    }
}

/// The settings under `v` at `path`, as leaves (a leaf is a value, an empty
/// table or a list), at most `room` of them; whether it stopped short.
fn leaves(
    v: &Value,
    path: &mut Vec<String>,
    out: &mut Vec<(Vec<String>, Value)>,
    room: usize,
) -> bool {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, x) in m {
                if out.len() >= room {
                    return true;
                }
                path.push(k.clone());
                let stopped = leaves(x, path, out, room);
                path.pop();
                if stopped {
                    return true;
                }
            }
            false
        }
        _ => {
            out.push((path.clone(), v.clone()));
            false
        }
    }
}

/// One setting's line: its value now, where it comes from, whether a change
/// is sensitive, and whether it restarts.
pub(crate) fn line(path: &[String], now: Option<&Value>, file: &Value) -> String {
    let value = match now {
        None => format!(
            "not set (it has no default value), or no setting has this name: ask for the table `{}` to list what it holds",
            path.first().map_or("", String::as_str)
        ),
        Some(v) => match withheld(path, v) {
            Some(why) => why.to_string(),
            None => shown(v),
        },
    };
    let from = match history::value_at(file, path) {
        Some(_) => "set in the settings file",
        None => "the default (the file does not set it)",
    };
    let change = match (sensitivity::reason(path), sensitivity::needs_fresh_proof(path, None)) {
        (Some(why), true) => format!(
            "a sensitive change ({why}): it asks for a fresh code from the authenticator app every time"
        ),
        (Some(why), false) => format!(
            "a sensitive change ({why}): it needs the second proof as well as the owner's yes"
        ),
        (None, _) => "an ordinary change: the owner's yes on the card".to_string(),
    };
    let restart = match sensitivity::needs_restart(path) {
        true => "takes effect after a restart",
        false => "takes effect at once",
    };
    format!(
        "- {}: {value} · {from} · {change} · {restart}",
        edit::dotted(path)
    )
}

/// Whether `t` is one of the settings' tables.
fn is_table(t: &str) -> bool {
    sensitivity::SENSITIVE_TABLES.iter().any(|(x, _)| *x == t)
        || sensitivity::ORDINARY_TABLES.contains(&t)
        || sensitivity::ORDINARY_TABLES_MORE.contains(&t)
}

/// What a name that is no setting says: the tables there are, so a typo finds
/// its table.
fn unknown(path: &[String]) -> String {
    let tables: Vec<&str> = sensitivity::SENSITIVE_TABLES
        .iter()
        .map(|(t, _)| *t)
        .chain(sensitivity::ORDINARY_TABLES)
        .chain(sensitivity::ORDINARY_TABLES_MORE)
        .collect();
    format!(
        "- {}: no such setting. The settings are in these tables: {}",
        edit::dotted(path),
        tables.join(", ")
    )
}

/// The words for each path named, built from the settings text `base`.
pub(crate) fn words(base: &str, paths: &[Vec<String>]) -> Result<String, String> {
    let now = effective(base)?;
    let file = history::doc_of(base).ok_or("the settings file does not parse")?;
    let mut out = String::new();
    let mut listed = 0usize;
    let mut stopped = false;
    for p in paths {
        if listed >= MAX_LISTED {
            stopped = true;
            break;
        }
        let Some(v) = history::value_at(&now, p) else {
            // Set in the file but unknown to the loader cannot happen (the file
            // loads); a name nobody has is said so.
            let known = p.first().is_some_and(|t| is_table(t));
            match history::value_at(&file, p).is_some() || known {
                true => out.push_str(&format!("{}\n", line(p, None, &file))),
                false => out.push_str(&format!("{}\n", unknown(p))),
            }
            listed += 1;
            continue;
        };
        let mut found = Vec::new();
        let mut at = p.clone();
        let room = MAX_LISTED.saturating_sub(listed).max(1);
        stopped |= match v.is_object() {
            true => leaves(&v, &mut at, &mut found, room),
            false => {
                found.push((p.clone(), v.clone()));
                false
            }
        };
        for (lp, lv) in found {
            out.push_str(&format!("{}\n", line(&lp, Some(&lv), &file)));
            listed += 1;
        }
    }
    if stopped {
        out.push_str(&format!(
            "(stopped at {MAX_LISTED} settings: name fewer, or a narrower table)\n"
        ));
    }
    Ok(out)
}

/// What a proposal's result says each setting was before it, as the daemon read
/// the file: its value, set or default. Empty when the file cannot be read
/// back (the proposal's own checks have said so).
pub(crate) fn before(base: &str, settings: &[diff::Setting]) -> String {
    let (Ok(now), Some(file)) = (effective(base), history::doc_of(base)) else {
        return String::new();
    };
    let mut out = String::from("Before this change, as Theseus read the settings:\n");
    for s in settings {
        out.push_str(&line(
            &s.path,
            history::value_at(&now, &s.path).as_ref(),
            &file,
        ));
        out.push('\n');
    }
    out
}

/// Run `config.show` in the turn `tc`: the owner's own turn gets the words,
/// anything else is refused in its own words.
pub fn run(rt: &ToolRuntime, tc: &TurnCtx<'_>, input: &Value) -> Result<(String, Value), String> {
    if let Some(why) = refusal(tc) {
        return Err(why);
    }
    let paths = paths_of(input)?;
    let base = super::read_base(rt)?;
    let said = words(&base, &paths)?;
    Ok((said, json!({"settings": paths.len()})))
}

/// The gate's hold on a read that is not free: when the turn has read text
/// from outside since the owner's last message, `config.show` is put to the
/// owner as any call that asks is, whatever its posture.
pub(crate) fn ask_if_outside(tc: &TurnCtx<'_>, name: &str, plan: Plan) -> Plan {
    if name != SHOW {
        return plan;
    }
    let (outside, steered) = super::outside_text(tc);
    held(name, outside, steered, &super::flag_way(tc), plan)
}

/// [`ask_if_outside`], on the facts it reads: each cause in its own words, with `way` to clear
/// the session's flag.
fn held(name: &str, outside: bool, steered: bool, way: &str, mut plan: Plan) -> Plan {
    if name == SHOW {
        if let Some(said) = super::hold_words("read Theseus's own settings", outside, steered, way)
        {
            plan.authority = Some(said);
        }
    }
    plan
}

/// `config.show`.
pub struct Show;

impl Tool for Show {
    fn name(&self) -> &str {
        SHOW
    }

    fn description(&self) -> &str {
        "Read a Theseus setting by name: its value now, default or set, and whether it restarts; never open the settings \
         file. The file is never opened, listed or searched, and the shell is not the way. Name each setting by its \
         path (`discord.task_channel_cap`, or a table such as `kernel` to list its settings); the answer also says \
         whether a change needs the second proof. A secret's value, and the names of vault items, are never given. To \
         change a setting, call `config.propose`; its result says each value before, so a plain change needs no read \
         first. Changes nothing. Offered only to the owner, in a private place."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "description": "The settings to read, at most 20, each by its path with keys joined by dots; a table lists its settings.",
                    "items": {"type": "string"}
                }
            },
            "required": ["paths"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Read
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::SafeToRepeat
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let paths = paths_of(input)?;
        let names: Vec<String> = paths.iter().map(|p| edit::dotted(p)).collect();
        Ok(Plan {
            summary: format!("read Theseus's settings: {}", names.join(", ")),
            ..Default::default()
        })
    }
}

/// The wall, before anything is read: `config.history`'s.
pub(crate) fn refusal(tc: &TurnCtx<'_>) -> Option<String> {
    wall::refusal_for(tc, SHOW)
}

#[cfg(test)]
mod tests;
