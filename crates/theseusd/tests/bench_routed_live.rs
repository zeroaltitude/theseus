//! The routed bench arm on a real daemon (the review of theseus-eo3h): a
//! trial as the adapter runs it (`theseus --spawn theseusd --json ask`, one
//! message, a fresh state directory), on `bench/theseus-bench-routed.toml`
//! with the stand-in model and the fake Jev in place of the two services.
//! - the first message of a fresh daemon is routed by Jev's verdict, live
//!   (not in shadow because the ladder is not yet read);
//! - the Jev key reaches Jev as a bearer token and nothing else: not the
//!   stand-in model, not a file under the trial's directory, not a ledger
//!   row, not a job's environment, not the CLI's output;
//! - the ledger reads the adapter's script runs give the rows its record
//!   reads.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted};

fn profile_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/theseus-bench-routed.toml")
}

/// Both keys, invented.
const KEY: &str = "tv-bench-key-7f3a9c";
const JEV: &str = "jv-bench-key-2b8e41";
/// The spend limit a trial on this profile needs (the review's finding).
const ROUTED_LIMIT: f64 = 10.0;

fn set(t: &mut toml::Table, path: &[&str], value: toml::Value) {
    let (last, tables) = path.split_last().unwrap();
    let mut at = t;
    for key in tables {
        at = at
            .entry(*key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap();
    }
    at.insert((*last).into(), value);
}

/// A trial's directory: the routed profile as it is but for the two API
/// bases (the stand-ins') and the task's directory.
fn trial(model: &FakeModel, jev: &FakeJev, spend_limit_usd: f64) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let text = std::fs::read_to_string(profile_path()).unwrap();
    let mut t: toml::Table = text.parse().unwrap();
    set(&mut t, &["model", "api_base"], model.base.clone().into());
    set(&mut t, &["judge", "api_base"], jev.base().into());
    set(
        &mut t,
        &["kernel", "spend_limit_usd"],
        spend_limit_usd.into(),
    );
    set(
        &mut t,
        &["tools", "projects_dir"],
        app.to_string_lossy().into_owned().into(),
    );
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

/// `theseus --spawn <theseusd> <args>` in a trial's directory, as the
/// adapter's run script runs it, with both keys in its environment.
fn theseus(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--spawn")
        .arg(&theseusd)
        .arg("--json")
        .args(args)
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
        // The CLI's seen file is the test's own, never the machine's (theseus-yus0).
        .env("XDG_STATE_HOME", dir.join("xdg-state"))
        .env("ANTHROPIC_API_KEY", KEY)
        .env("TYPESAFE_API_KEY", JEV)
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn script_mode(jev: &FakeJev, mode: &str, confidence: f64) {
    jev.script(
        "mode",
        Scripted::Choice {
            option: mode.into(),
            confidence,
        },
    );
}

/// Every file under `dir`, whole, for a needle.
fn files_holding(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            let ty = e.file_type().unwrap();
            if ty.is_dir() {
                stack.push(p);
            } else if ty.is_file() {
                let bytes = std::fs::read(&p).unwrap_or_default();
                if bytes.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                    found.push(p);
                }
            }
        }
    }
    found
}

fn no_calls(_: &str) -> Vec<(&'static str, Value)> {
    vec![]
}

/// The first message of a fresh one-shot daemon, which Jev says is deep
/// coding, runs on Opus (the table's first for that mode), recorded live.
#[test]
fn the_first_message_of_a_fresh_daemon_is_routed_live() {
    let model = FakeModel::start(no_calls);
    let jev = FakeJev::start().unwrap();
    script_mode(&jev, "deep_coding", 0.95);
    let dir = trial(&model, &jev, ROUTED_LIMIT);
    let (code, out, err) = theseus(dir.path(), &["ask", "fix the failing build in this repo"]);
    assert_eq!(code, 0, "{out}\n{err}");
    let (_, ledger, lerr) = theseus(dir.path(), &["ledger", "-n", "1000", "-k", "route.decided"]);
    let rows: Value =
        serde_json::from_str(&ledger).unwrap_or_else(|e| panic!("{e}: {ledger}\n{lerr}"));
    let rows = rows["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 1, "{ledger}");
    let d = &rows[0]["data"];
    assert_eq!(d["mode"], "deep_coding", "{d}");
    assert_eq!(d["profile"], "opus", "{d}");
    assert_eq!(d["late"], false, "{d}");
    assert_ne!(d["reason"].as_str().unwrap_or(""), "shadow", "{d}");
    assert!(rows[0]["position"].is_u64(), "{}", rows[0]);
    let models: Vec<String> = model
        .requests()
        .iter()
        .map(|r| r["model"].as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(models, ["claude-opus-5-5"], "{models:?}");
    // The judge.call rows the record reads.
    let (_, calls, _) = theseus(dir.path(), &["ledger", "-n", "1000", "-k", "judge.call"]);
    let calls: Value = serde_json::from_str(&calls).unwrap();
    assert!(
        calls["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["data"]["pack"] == "route.v3" && r["data"]["cost_micros"].is_u64()),
        "{calls}"
    );
    // The key reached Jev, as a bearer token of its length.
    let seen = jev.seen();
    assert!(!seen.is_empty());
    assert!(
        seen.iter().all(|s| s.bearer_len == Some(JEV.len())),
        "{seen:?}"
    );
}

/// The Jev key is in no file, no row, no output, and no request to the model.
#[test]
fn the_jev_key_reaches_jev_alone() {
    let model = FakeModel::start(|prompt| {
        if prompt.contains("environment") {
            vec![("proc_run", json!({"argv": ["sh", "-c", "env"]}))]
        } else {
            vec![]
        }
    });
    let jev = FakeJev::start().unwrap();
    script_mode(&jev, "deep_coding", 0.95);
    let dir = trial(&model, &jev, ROUTED_LIMIT);
    let (code, out, err) = theseus(dir.path(), &["ask", "print your environment"]);
    assert_eq!(code, 0, "{out}\n{err}");
    // Let the judge's writes land, then read everything back.
    let (_, hist, _) = theseus(dir.path(), &["history"]);
    let (_, ledger, _) = theseus(dir.path(), &["ledger", "-n", "1000"]);
    for (what, text) in [
        ("ask", &out),
        ("stderr", &err),
        ("history", &hist),
        ("ledger", &ledger),
    ] {
        assert!(!text.contains(JEV), "the Jev key is in the {what}");
    }
    for r in model.raw_requests() {
        let s = String::from_utf8_lossy(&r);
        assert!(!s.contains(JEV), "the Jev key reached the model's API");
    }
    // The job's environment held neither key.
    let ran_env = model
        .requests()
        .iter()
        .any(|r| r.to_string().contains("PATH="));
    assert!(ran_env, "the job's `env` did not run");
    // A stop writes the daemon's last checkpoint; the shot's daemon has
    // gone with the ask. Nothing on disk holds the key.
    let held = files_holding(dir.path(), JEV);
    assert!(held.is_empty(), "files hold the Jev key: {held:?}");
}

/// A task's shell can read the daemon's environment (`/proc/<pid>/environ`,
/// the same user, L0): the scrubber holds both keys' values back from what the
/// model and the record see, as it does the plain arm's one.
#[test]
fn a_job_that_reads_the_daemons_environment_sees_neither_key() {
    let model = FakeModel::start(|prompt| {
        if prompt.contains("environ") {
            let script = "for f in /proc/[0-9]*/environ; do tr '\\0' '\\n' < $f 2>/dev/null; done \
                          | grep -a -E 'TYPESAFE_API_KEY|ANTHROPIC_API_KEY' | sort -u";
            vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
        } else {
            vec![]
        }
    });
    let jev = FakeJev::start().unwrap();
    script_mode(&jev, "routine_coding", 0.95);
    let dir = trial(&model, &jev, ROUTED_LIMIT);
    let (code, out, err) = theseus(dir.path(), &["ask", "read /proc environ"]);
    assert_eq!(code, 0, "{out}\n{err}");
    let (_, hist, _) = theseus(dir.path(), &["history"]);
    let seen = model.requests().last().unwrap().to_string();
    assert!(
        seen.contains("TYPESAFE_API_KEY"),
        "the job did not read the environment: {seen}"
    );
    for (what, text) in [
        ("model request", &seen),
        ("history", &hist),
        ("ask", &out),
        ("stderr", &err),
    ] {
        assert!(
            !text.contains(JEV) && !text.contains(KEY),
            "a key's value is in the {what}"
        );
    }
}

/// At the plain arm's $2 limit the money gate refuses the first call Jev
/// sends to Opus or Fable: the profiles cap no output, so the reservation is
/// 128,000 tokens at the model's price (about $2.6 and $6.5). The adapter's
/// routed default is higher (`ROUTED_SPEND_LIMIT_USD`); a mode that reaches
/// Haiku or Sonnet is unaffected.
#[test]
fn at_two_dollars_an_opus_or_fable_route_is_refused_by_the_money_gate() {
    for (mode, profile, model, refused) in [
        ("deep_coding", "opus", "claude-opus-5-5", true),
        ("sophisticated", "fable", "claude-fable-5-1", true),
        ("routine_coding", "haikuhi", "claude-haiku-5-5", false),
    ] {
        let stand_in = FakeModel::start(no_calls);
        let jev = FakeJev::start().unwrap();
        script_mode(&jev, mode, 0.95);
        let dir = trial(&stand_in, &jev, 2.0);
        let (code, out, err) = theseus(dir.path(), &["ask", "do the task"]);
        let turn: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
        assert_eq!(turn["profile"], profile, "{mode}: {out}\n{err}");
        assert_eq!(turn["model"], model, "{mode}: {out}");
        if refused {
            assert_eq!(
                (code, &turn["stop_reason"]),
                (5, &json!("budget")),
                "{mode}: {out}\n{err}"
            );
            assert!(stand_in.requests().is_empty(), "{mode}: a call went out");
        } else {
            assert_eq!(code, 0, "{mode}: {out}\n{err}");
        }
    }
}
