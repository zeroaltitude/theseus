//! The operator's pin on a real daemon: one session on the routed bench
//! profile, one `theseus ask` a message, with the stand-in model and a
//! scripted fake Jev. The daemon is the owner's shape, and the first message
//! waits for its ladder's first read: before it the route pack judges in
//! shadow (theseus-289c), so a daemon a message (`--spawn`) would race it. The
//! ledger's `route.decided` rows say what each turn ran on and why.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use serde_json::Value;
use theseus_judge::fake::{FakeJev, Scripted};

const KEY: &str = "tv-bench-key-7f3a9c";
const JEV: &str = "jv-bench-key-2b8e41";

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

fn trial(model: &FakeModel, jev: &FakeJev) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("app");
    std::fs::create_dir_all(&app).unwrap();
    let profile =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bench/theseus-bench-routed.toml");
    let mut t: toml::Table = std::fs::read_to_string(profile).unwrap().parse().unwrap();
    set(&mut t, &["model", "api_base"], model.base.clone().into());
    set(&mut t, &["judge", "api_base"], jev.base().into());
    set(&mut t, &["kernel", "spend_limit_usd"], 10.0.into());
    set(&mut t, &["kernel", "daily_spend_ceiling_usd"], 20.0.into());
    // Every message is its own daemon's first: a cache hold above the cold
    // line would wait for a second agreeing turn, so keep the line high.
    set(&mut t, &["routing", "cold_switch_tokens"], 1_000_000.into());
    // The gate's line this test draws between 0.65 and 0.9 (the bench
    // profile's own is the owner's live 0.40).
    set(&mut t, &["routing", "high_effort_confidence"], 0.70.into());
    set(
        &mut t,
        &["tools", "projects_dir"],
        app.to_string_lossy().into_owned().into(),
    );
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

fn theseus(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(dir.join("sock"))
        .arg("--json")
        .args(args)
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
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

/// The conversation's daemon, once its ladder has read the route pack live.
fn daemon(dir: &Path) -> common::Daemon {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let d = common::Daemon::spawn(
        Command::new(&theseusd)
            .arg("--config")
            .arg(dir.join("config.toml"))
            .arg("--state-dir")
            .arg(dir.join("state"))
            .arg("--socket")
            .arg(dir.join("sock"))
            .env("XDG_STATE_HOME", dir.join("xdg-state"))
            .env("ANTHROPIC_API_KEY", KEY)
            .env("TYPESAFE_API_KEY", JEV)
            .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
            .env_remove("THESEUS_OP_TOKEN_FILE"),
    );
    let t0 = Instant::now();
    loop {
        let (code, out, _) = theseus(dir, &["health"]);
        let packs = serde_json::from_str::<Value>(&out).map(|h| h["judge"]["packs"].clone());
        let live = packs
            .as_ref()
            .ok()
            .and_then(Value::as_array)
            .is_some_and(|p| {
                p.iter().any(|l| {
                    l.as_str()
                        .is_some_and(|l| l.starts_with("route.v") && l.contains(": live"))
                })
            });
        if code == 0 && live {
            return d;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the ladder never read the route pack live: {out}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn script(jev: &FakeJev, mode: &str, effort: &str, confidence: f64) {
    for (q, option) in [("mode", mode), ("reply_effort", effort)] {
        jev.script(
            q,
            Scripted::Choice {
                option: option.into(),
                confidence,
            },
        );
    }
}

// ---- the pins (theseus-h66v4, row 1): cases W39 and W40 of route.v10's test set
//
// The design's sixty cases are 58 replay cases (`crates/theseus-judge/packs/route.v10.cases.toml`)
// and these two, because a pin is the operator's word and is never asked of Jev. They run on
// today's acting pack, route.v8, whose `mode` verdict a pinned turn still records (it is
// asked in shadow and never applied); once `decide()` reads route.v10's `model` answer
// (row 3), the pinned turn asks no `model` question at all, and these two keep holding.
// W40 is a `must` case of the design's fourteen: the pin wins over the Fable case.

/// The ledger's `route.decided` rows, oldest turn first.
fn decided(dir: &Path) -> Vec<Value> {
    let (_, ledger, e) = theseus(dir, &["ledger", "-n", "1000", "-k", "route.decided"]);
    let rows: Value =
        serde_json::from_str(&ledger).unwrap_or_else(|x| panic!("{x}: {ledger}\n{e}"));
    let mut rows: Vec<Value> = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["data"].clone())
        .collect();
    rows.sort_by_key(|d| d["turn"].as_str().unwrap_or("").to_string());
    rows
}

/// The `model` each request to the stand-in model named, in order.
fn models_sent(model: &FakeModel) -> Vec<String> {
    model
        .requests()
        .iter()
        .filter_map(|q| q["model"].as_str().map(str::to_string))
        .collect()
}

/// W39: `-P fable` on "thanks!" is honored. The pin names the model, so the turn ran on Fable
/// (`reason = pinned`) though Jev, asked, called the message a thank-you for Haiku.
#[test]
fn w39_a_pinned_fable_on_a_thank_you_is_honored() {
    let model = FakeModel::start(|_| vec![]);
    let jev = FakeJev::start().unwrap();
    let dir = trial(&model, &jev);
    let _daemon = daemon(dir.path());
    script(&jev, "trivial", "low", 0.97);
    let (code, out, err) = theseus(dir.path(), &["ask", "-P", "fable", "thanks!"]);
    assert_eq!(code, 0, "{out}\n{err}");
    let rows = decided(dir.path());
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["reason"], "pinned", "{:?}", rows[0]);
    assert_eq!(rows[0]["profile"], "fable", "{:?}", rows[0]);
    assert_eq!(
        rows[0]["switch"], false,
        "a pin is not a switch Jev made: {:?}",
        rows[0]
    );
    assert_eq!(models_sent(&model), ["claude-fable-5-1"]);
}

/// W40 (`must`): a pinned Haiku beats a Fable-shaped design question: the operator's named model
/// wins over the case the table would send to Fable.
///
/// The design words the pin as `theseus profile use haiku`. Today that command changes the
/// daemon's *live* profile (the baseline Jev routes from, `[model] live`), which is no pin: with
/// it set to Haiku, Jev's `sophisticated` verdict still moved this same question to Opus
/// (`reason = verdict`, seen 2026-10-30 on this lane). The per-turn pin, `-P haiku`, is the one
/// `routing::chosen` reads, so that is what this test pins. Making `profile use` a pin is a
/// decision for the row that rewrites `decide()` (the report names it).
#[test]
fn w40_a_pinned_haiku_beats_a_fable_shaped_question() {
    let model = FakeModel::start(|_| vec![]);
    let jev = FakeJev::start().unwrap();
    let dir = trial(&model, &jev);
    let _daemon = daemon(dir.path());
    // Jev, asked, calls it the hardest reasoning there is.
    script(&jev, "sophisticated", "max", 0.97);
    let (code, out, err) = theseus(
        dir.path(),
        &[
            "ask",
            "-P",
            "haiku",
            "design the memory layer for a million turns",
        ],
    );
    assert_eq!(code, 0, "{out}\n{err}");
    let rows = decided(dir.path());
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["profile"], "haiku", "{:?}", rows[0]);
    assert_eq!(rows[0]["reason"], "pinned", "{:?}", rows[0]);
    assert_eq!(models_sent(&model), ["claude-haiku-5-5"]);
}
