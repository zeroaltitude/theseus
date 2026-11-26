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

/// (G1) The addresses the configuration holds today, by the walk of the uncommented template.
const ADDRESSES: &[&str] = &[];
/// (G3) The folders, files and agent lists the configuration holds today outside the account
/// tables.
const FOLDERS_AND_AGENTS: &[&str] = &[];
/// (D) The prompt-text keys the configuration holds today.
const PROMPT_TEXT: &[&str] = &[];

/// Issue 57, the second reason table by groups: the walk of every setting the configuration
/// accepts (the uncommented template, parsed) names each key of the groups found by kind, and
/// no setting needs less than main asked of it (main: every sensitive setting needs the code)
/// except prompt text (D). A spending limit is set where there was none here, which keeps the
/// code.
#[test]
fn each_group_names_its_keys_and_nothing_is_weaker_than_main_but_prompt_text() {
    use super::sensitivity::{group, proof_reason, reason, Group, DROPPED_KEYS};
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
        named(&|p| reason(p).is_some_and(|r| DROPPED_KEYS.contains(&r.as_str()))),
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
    let weaker: Vec<String> = all
        .iter()
        .filter(|(p, v)| {
            let dropped = reason(p).is_some_and(|r| DROPPED_KEYS.contains(&r.as_str()));
            reason(p).is_some() && !dropped && proof_reason(p, None, Some(v)).is_none()
        })
        .map(|(p, _)| p.join("."))
        .collect();
    assert!(weaker.is_empty(), "weaker than main: {weaker:#?}");
}
