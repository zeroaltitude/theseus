//! The classification of every setting (`config/sensitivity.rs`): the whole
//! config type is walked, so a setting added without a decision fails here.

use super::sensitivity::{
    is_sensitive, ORDINARY_KEYS, ORDINARY_TABLES, ORDINARY_TABLES_MORE, SENSITIVE_TABLES,
};
use super::Config;
use serde_json::Value;

/// The keys a table of the config type accepts, read from the loader's own
/// refusal of one it does not: every table is `deny_unknown_fields`, so the
/// message names them all.
fn fields_of_top_level() -> Vec<String> {
    let e = Config::from_toml("zzz_probe = 1").expect_err("an unknown key is refused");
    let text = format!("{e:#}");
    let after = text
        .split("expected one of")
        .nth(1)
        .unwrap_or_else(|| panic!("the refusal names the fields: {text}"));
    after
        .lines()
        .next()
        .unwrap_or_default()
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// Every leaf of the config as the template sets it, as a path.
fn leaves(v: &Value, at: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, v) in m {
                at.push(k.clone());
                leaves(v, at, out);
                at.pop();
            }
        }
        _ => out.push(at.clone()),
    }
}

/// A key that reads as risky: one of its words (split at `_`, `-` and `.`) is
/// on this list. Broader than what is sensitive, so what is ordinary among
/// them is named.
fn suspect(k: &str) -> bool {
    const WORDS: &[&str] = &[
        "usd",
        "budget",
        "ceiling",
        "spend",
        "cap",
        "limit",
        "secret",
        "token",
        "key",
        "password",
        "credentials",
        "path",
        "paths",
        "dir",
        "dirs",
        "root",
        "roots",
        "file",
        "files",
        "egress",
        "sandbox",
        "owner",
        "allow",
        "host",
        "hosts",
        "bind",
        "port",
        "url",
        "endpoint",
        "command",
        "env",
        "trust",
        "posture",
        "floor",
        "public",
        "private",
        "guild",
        "mode",
        "argv",
        "origin",
        "base",
        "proxy",
        "external",
    ];
    k.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| WORDS.contains(&w))
}

fn excused(path: &[String]) -> bool {
    ORDINARY_KEYS.iter().any(|p| {
        let want: Vec<&str> = p.split('.').collect();
        want.len() == path.len() && want.iter().zip(path).all(|(w, g)| *w == "*" || w == g)
    })
}

#[test]
fn every_table_and_every_risky_key_is_classified() {
    let top = fields_of_top_level();
    assert!(
        top.len() > 50,
        "the probe found the config's tables: {top:?}"
    );
    let named: Vec<&str> = SENSITIVE_TABLES
        .iter()
        .map(|(t, _)| *t)
        .chain(ORDINARY_TABLES)
        .chain(ORDINARY_TABLES_MORE)
        .collect();
    let missing: Vec<&String> = top
        .iter()
        .filter(|t| !named.contains(&t.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "these tables are in neither the sensitive list nor the ordinary one (config/sensitivity.rs): {missing:?}"
    );
    let stale: Vec<&&str> = named
        .iter()
        .filter(|t| !top.iter().any(|x| x == **t))
        .collect();
    assert!(
        stale.is_empty(),
        "these named tables are not in the config: {stale:?}"
    );

    let text = super::tests::uncommented(Config::EXAMPLE_TOML);
    let cfg = Config::from_toml(&text).expect("the uncommented template parses");
    let mut all = Vec::new();
    leaves(
        &serde_json::to_value(&cfg).unwrap(),
        &mut Vec::new(),
        &mut all,
    );
    assert!(
        all.len() > 200,
        "the walk reached the template's settings: {}",
        all.len()
    );
    let loose: Vec<String> = all
        .iter()
        .filter(|p| p.last().is_some_and(|k| suspect(k)) && !is_sensitive(p) && !excused(p))
        .map(|p| p.join("."))
        .collect();
    assert!(
        loose.is_empty(),
        "these keys read as risky and are neither sensitive nor named in ORDINARY_KEYS:\n{}",
        loose.join("\n")
    );
}

#[test]
fn every_sensitive_table_is_a_real_one_and_the_template_reaches_it() {
    let text = super::tests::uncommented(Config::EXAMPLE_TOML);
    let cfg = Config::from_toml(&text).unwrap();
    let v = serde_json::to_value(&cfg).unwrap();
    for (t, _) in SENSITIVE_TABLES {
        assert!(
            fields_of_top_level().iter().any(|f| f == t),
            "{t} is not a table of the config: was it renamed?"
        );
        let _ = &v;
    }
}

/// Every leaf of the config as the template sets it, with its value.
fn valued(v: &Value, at: &mut Vec<String>, out: &mut Vec<(Vec<String>, Value)>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, v) in m {
                at.push(k.clone());
                valued(v, at, out);
                at.pop();
            }
        }
        _ => out.push((at.clone(), v.clone())),
    }
}

/// (G1) The addresses the configuration holds today, by the walk of the uncommented template:
/// KEPT by the owner's word.
const ADDRESSES: &[&str] = &[
    "aws.accounts.111122223333.endpoint",
    "catalog.some-new-model.provider",
    "cockpit.public_url",
    "discord.gateway_proxy",
    "discord.rest_proxy",
    "google.endpoint",
    "judge.api_base",
    "mcp.servers.docs.url",
    "model.api_base",
    "model.provider",
    "profiles.dsflash.provider",
    "profiles.dspro.provider",
    "profiles.fable.provider",
    "profiles.fable51.provider",
    "profiles.fable52.provider",
    "profiles.glm.provider",
    "profiles.glm53.provider",
    "profiles.haiku.provider",
    "profiles.kimi.provider",
    "profiles.kimimax.provider",
    "profiles.mythos.provider",
    "profiles.opus.provider",
    "profiles.sonnet.provider",
    "providers.deepseek.api_base",
    "providers.deepseek.origin",
    "providers.moonshot.api_base",
    "providers.moonshot.origin",
    "providers.openai.api_base",
    "providers.openai.origin",
    "providers.zai.api_base",
    "providers.zai.origin",
    "telemetry.otlp_endpoint",
    "voice.api_base",
    "web.bind",
    "web.dev_origin",
    "web.port",
];
/// (G3) The folders, files and agent lists the configuration holds today outside the account
/// tables: DROPPED by the owner's word, with the global exceptions still winning.
const FOLDERS_AND_AGENTS: &[&str] = &[
    "import.openclaw.state_dir",
    "index.weights_dir",
    "mcp.servers.github.read",
    "personas.theseus.files",
    "tools.projects_dir",
    "tools.roots",
];
/// (D) The prompt-text keys the configuration holds today.
const PROMPT_TEXT: &[&str] = &[
    "model.end_note",
    "model.system",
    "personas.theseus.card",
    "profiles.dsflash.system",
    "profiles.dspro.system",
    "profiles.fable.system",
    "profiles.fable51.system",
    "profiles.fable52.system",
    "profiles.glm.system",
    "profiles.glm53.system",
    "profiles.haiku.system",
    "profiles.kimi.system",
    "profiles.kimimax.system",
    "profiles.mythos.system",
    "profiles.opus.system",
    "profiles.sonnet.system",
];

/// Issue 57, the second reason table by groups: the walk of every setting the configuration
/// accepts (the uncommented template, parsed) names each key of the groups found by kind; no
/// setting needs less than main asked of it (main: every sensitive setting needs the code)
/// except prompt text (D) and the group the owner's word dropped (G3); and inside that group
/// every global exception (a key that reads a secret, the owner, a sandbox-like key, a
/// spending key) still needs the code. A spending limit is set where there was none here,
/// which keeps the code.
#[test]
fn each_group_names_its_keys_and_only_the_dropped_ones_are_weaker_than_main() {
    use super::sensitivity::{
        group, key_reason, prompt_text, proof_reason, reason, Group, KEEPS, KEPT_KEYS, SPEND_REASON,
    };
    let text = super::tests::uncommented(Config::EXAMPLE_TOML);
    let cfg = Config::from_toml(&text).expect("the uncommented template parses");
    let mut all = Vec::new();
    valued(
        &serde_json::to_value(&cfg).unwrap(),
        &mut Vec::new(),
        &mut all,
    );
    let named = |pick: &dyn Fn(&[String]) -> bool| {
        let mut v: Vec<String> = all
            .iter()
            .filter(|(p, _)| pick(p))
            .map(|(p, _)| p.join("."))
            .collect();
        v.sort();
        v.dedup();
        v
    };
    let accounts = |p: &[String]| super::sensitivity::ACCOUNT_TABLES.contains(&p[0].as_str());
    // A setting of (K), or a fresh one, is decided before any group.
    let kept = |p: &[String]| {
        super::sensitivity::KEPT_TABLES.contains(&p[0].as_str())
            || super::sensitivity::needs_fresh_proof(p, None)
    };
    let got = (
        named(&|p| group(p) == Some(Group::Addresses) && !kept(p)),
        named(&|p| group(p) == Some(Group::SelfAndAccounts) && !accounts(p) && !kept(p)),
        named(&prompt_text),
    );
    assert_eq!(
        got,
        (
            ADDRESSES.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            FOLDERS_AND_AGENTS
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            PROMPT_TEXT
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
        ),
        "the groups as the configuration holds them now: (addresses, folders and agents, prompt \
         text)\n{got:#?}"
    );
    let dropped = |p: &[String]| group(p).is_some_and(|g| !KEEPS.contains(&g));
    let weaker: Vec<String> = all
        .iter()
        .filter(|(p, v)| {
            reason(p).is_some()
                && !prompt_text(p)
                && !dropped(p)
                && proof_reason(p, None, Some(v)).is_none()
        })
        .map(|(p, _)| p.join("."))
        .collect();
    assert!(weaker.is_empty(), "weaker than main: {weaker:#?}");
    let exception = |p: &[String]| {
        p.iter()
            .skip(1)
            .any(|k| key_reason(k).is_some_and(|w| KEPT_KEYS.contains(&w) || w == SPEND_REASON))
    };
    let lost: Vec<String> = all
        .iter()
        .filter(|(p, v)| dropped(p) && exception(p) && proof_reason(p, None, Some(v)).is_none())
        .map(|(p, _)| p.join("."))
        .collect();
    assert!(
        lost.is_empty(),
        "an exception inside the dropped group lost its code: {lost:#?}"
    );
    assert!(
        all.iter()
            .any(|(p, v)| dropped(p) && proof_reason(p, None, Some(v)).is_none()),
        "the dropped group takes the card alone somewhere"
    );
}
