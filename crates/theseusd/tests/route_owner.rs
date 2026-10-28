//! Only Jev elevates, and a raised session steps back down (theseus-p8dy), on
//! a real daemon: one session on the routed bench profile, one `theseus ask`
//! a message, with the stand-in model and a scripted fake Jev. One daemon
//! serves them all, as the owner's does, and the first message waits for its
//! ladder's first read: before it route.v4 judges in shadow (theseus-289c), so
//! a daemon a message (`--spawn`) would race it. Jev answers `max` at
//! 0.65 and then 0.9, then `sophisticated`, then `chat` twice; the ledger's
//! `route.decided` rows say what each turn ran on and why.

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

/// The conversation's daemon, once its ladder has read route.v9 live.
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
                p.iter()
                    .any(|l| l.as_str().is_some_and(|l| l.starts_with("route.v9: live")))
            });
        if code == 0 && live {
            return d;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the ladder never read route.v9 live: {out}"
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

#[test]
fn jev_raises_a_session_and_two_chats_bring_it_back() {
    let model = FakeModel::start(|_| vec![]);
    let jev = FakeJev::start().unwrap();
    let dir = trial(&model, &jev);
    let _daemon = daemon(dir.path());
    let mut session: Option<String> = None;
    let steps = [
        ("chat", "max", 0.65),
        ("chat", "max", 0.9),
        ("sophisticated", "medium", 0.95),
        ("chat", "medium", 0.95),
        ("chat", "medium", 0.95),
    ];
    for (i, (mode, effort, c)) in steps.into_iter().enumerate() {
        script(&jev, mode, effort, c);
        let mut args = vec!["ask"];
        if let Some(s) = &session {
            args.extend(["-s", s]);
        }
        args.push("and what comes after that?");
        let (code, out, err) = theseus(dir.path(), &args);
        assert_eq!(code, 0, "message {i}: {out}\n{err}");
        let v: Value = serde_json::from_str(out.lines().last().unwrap()).unwrap();
        session = Some(v["session_id"].as_str().unwrap().to_string());
    }
    let (_, ledger, e) = theseus(dir.path(), &["ledger", "-n", "1000", "-k", "route.decided"]);
    let rows: Value =
        serde_json::from_str(&ledger).unwrap_or_else(|x| panic!("{x}: {ledger}\n{e}"));
    let mut rows: Vec<Value> = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["data"].clone())
        .collect();
    rows.sort_by_key(|d| d["turn"].as_str().unwrap_or("").to_string());
    let line = |d: &Value| {
        format!(
            "{} {:.2} → {} {} switch={} | effort {} {:.2} {} ran {}",
            d["mode"],
            d["confidence"].as_f64().unwrap_or(0.0),
            d["profile"],
            d["reason"],
            d["switch"],
            d["effort"],
            d["effort_confidence"].as_f64().unwrap_or(0.0),
            d["effort_reason"],
            d["effort_ran"]
        )
    };
    for d in &rows {
        eprintln!("{}", line(d));
    }
    assert_eq!(rows.len(), 5, "{ledger}");
    let get = |i: usize, k: &str| rows[i][k].as_str().unwrap_or("").to_string();
    // Max at 0.65 runs high; at 0.9 it runs max.
    assert_eq!(
        (get(0, "effort_reason"), get(0, "effort_ran")),
        ("held_high".into(), "high".into())
    );
    assert_eq!(
        (get(1, "effort_reason"), get(1, "effort_ran")),
        ("applied".into(), "max".into())
    );
    // Sophisticated raises the session; the first chat holds, the second steps down.
    assert_eq!(
        (get(2, "profile"), get(2, "reason")),
        ("opus".into(), "verdict".into())
    );
    assert_eq!(rows[2]["switch"], true);
    assert_eq!(
        (get(3, "profile"), get(3, "reason")),
        ("opus".into(), "cache_hold".into())
    );
    assert_eq!(
        (get(4, "profile"), get(4, "reason")),
        ("bench".into(), "step_down".into())
    );
    assert_eq!(rows[4]["switch"], true);
}
