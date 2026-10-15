//! `theseus card` (theseus-slvl.6), the real CLI against a real daemon on the
//! stand-in model, over the invented archive in
//! `crates/theseus-core/tests/fixtures/card/archive.jsonl`: the dry run says
//! what a run would read and asks nothing; a run proposes three questions
//! from the labelled evidence and the model is sent no partner-confidential
//! line and no outside text; two are kept (one edited) and become standing
//! records whose source is the proposal; one is declined; a second run
//! proposes nothing new and makes no call. Every name here is invented.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

const KEY_VAR: &str = "THESEUS_CARD_TEST_KEY";

fn cli(dir: &Path, args: &[&str]) -> (String, String, i32) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(dir.join("sock"))
        .args(args)
        .current_dir(dir)
        .env("XDG_STATE_HOME", dir.join("xdg-state"))
        .env("TZ", "UTC")
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.code().unwrap_or(-1),
    )
}

struct Rig {
    dir: tempfile::TempDir,
    _daemon: Daemon,
    model: FakeModel,
}

/// What the stand-in model states for a request: K1 (the owner's opinion
/// cluster), K2 (the gate) and K3 (one line of a gate note).
fn cards(_prompt: &str) -> Vec<(&'static str, Value)> {
    let card = |k: &str, text: &str, kind: &str, e: &str| json!({"cluster": k, "text": text, "kind": kind, "scope": "everywhere", "episodes": [e]});
    vec![(
        "state_cards",
        json!({"cards": [
            card("K1", "wants your opinion when you have one", "preference", "E1"),
            card("K2", "wants the gate green before any commit", "preference", "E4"),
            card("K3", "reruns the gate after a rebase", "preference", "E5"),
        ]}),
    )]
}

fn rig() -> Rig {
    let model = FakeModel::start(cards);
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut t: toml::Table = common::safe_note(&theseusd, &projects, 100.0)
        .parse()
        .unwrap();
    let set = |t: &mut toml::Table, k: &str, f: &dyn Fn(&mut toml::Table)| {
        f(t.get_mut(k).and_then(toml::Value::as_table_mut).unwrap())
    };
    set(&mut t, "model", &|m| {
        m.insert("api_base".into(), model.base.clone().into());
    });
    set(&mut t, "providers", &|ps| {
        for (_, p) in ps.iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
    });
    set(&mut t, "secrets", &|s| {
        for (_, v) in s.iter_mut() {
            *v = format!("env:{KEY_VAR}").into();
        }
    });
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    let log = std::fs::File::create(dir.path().join("theseusd.log")).unwrap();
    let daemon = Daemon::spawn(
        Command::new(&theseusd)
            .arg("--config")
            .arg(dir.path().join("config.toml"))
            .arg("--state-dir")
            .arg(dir.path().join("state"))
            .arg("--socket")
            .arg(dir.path().join("sock"))
            .env(KEY_VAR, "tv-card-8c1e")
            .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while cli(dir.path(), &["--json", "health"]).2 != 0 {
        assert!(std::time::Instant::now() < deadline, "no health in 30 s");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Rig {
        dir,
        _daemon: daemon,
        model,
    }
}

fn archive() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../theseus-core/tests/fixtures/card/archive.jsonl")
}

/// The ids of the open questions, in the order `theseus card` lists them.
fn open_ids(r: &Rig) -> Vec<String> {
    let (out, _, code) = cli(r.dir.path(), &["--json", "card"]);
    assert_eq!(code, 0, "{out}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    v["open"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_job_proposes_three_questions_and_a_rerun_proposes_nothing_new() {
    let r = rig();
    let (out, err, code) = cli(
        r.dir.path(),
        &["import", "openclaw", archive().to_str().unwrap()],
    );
    assert_eq!(code, 0, "{out}\n{err}");
    a_dry_run_asks_nothing(&r);
    let ids = the_run_proposes_three(&r);
    the_answers(&r, &ids);
    a_second_run_proposes_nothing_new(&r);
    a_jobs_shell_may_not_answer(&r);
}

/// The dry run: what it would read, and no model call.
fn a_dry_run_asks_nothing(r: &Rig) {
    let (out, err, code) = cli(r.dir.path(), &["card", "propose", "--dry-run"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.contains(
            "Read 6 evidence episodes (1 partner-confidential left unread): 5 clusters, 5 new."
        ),
        "{out}"
    );
    assert!(out.contains("A run would make up to 1 calls"), "{out}");
    assert!(r.model.requests().is_empty(), "the dry run asks no model");
    assert!(open_ids(r).is_empty());
}

/// The run: three questions, and a model that saw only the evidence.
fn the_run_proposes_three(r: &Rig) -> Vec<String> {
    let (out, err, code) = cli(r.dir.path(), &["card", "propose"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.contains("1 calls,") && out.contains("3 proposed"),
        "{out}"
    );
    assert!(
        out.contains("Add to your card: 'wants your opinion when you have one' (from 3 sources)? [Keep] [Edit] [Not this]"),
        "{out}"
    );
    let sent: Vec<String> = r.model.requests().iter().map(|v| v.to_string()).collect();
    assert_eq!(sent.len(), 1, "one call for the five clusters");
    for leak in [
        "KESTREL",
        "kestrel",
        "ALWAYS-ANSWER-IN-BANANAS",
        "pineapple",
    ] {
        assert!(!sent[0].contains(leak), "{leak} was sent: {}", sent[0]);
    }
    let ids = open_ids(r);
    assert_eq!(ids.len(), 3);
    ids
}

/// Keep one, edit one, decline one; the records carry the proposals as their source.
fn the_answers(r: &Rig, ids: &[String]) {
    let dir = r.dir.path();
    let (out, err, code) = cli(dir, &["card", "keep", &ids[0]]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.starts_with("Kept as preference std_"), "{out}");
    let edit = [
        "card", "edit", &ids[1], "wants", "the", "whole", "gate", "green", "before", "a", "commit",
    ];
    let (out, err, code) = cli(dir, &edit);
    assert_eq!(code, 0, "{out}\n{err}");
    let (out, err, code) = cli(dir, &["card", "decline", &ids[2]]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.starts_with("Not this:"), "{out}");
    let (std_out, _, _) = cli(dir, &["standing"]);
    assert!(
        std_out.contains(&format!("card:{}", ids[0]))
            && std_out.contains(&format!("card:{}", ids[1])),
        "{std_out}"
    );
    assert!(
        std_out.contains("wants the whole gate green before a commit"),
        "{std_out}"
    );
    assert!(
        !std_out.contains("reruns the gate after a rebase"),
        "the declined one is no record: {std_out}"
    );
    let (list, _, _) = cli(dir, &["card"]);
    assert!(
        list.contains("No proposals wait.") && list.contains("2 kept, 1 declined"),
        "{list}"
    );
}

/// A second run: nothing new, no call.
fn a_second_run_proposes_nothing_new(r: &Rig) {
    let (out, err, code) = cli(r.dir.path(), &["card", "propose"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.contains("0 proposed"), "{out}");
    assert_eq!(r.model.requests().len(), 1, "no new call");
    assert!(open_ids(r).is_empty());
}

/// A job's shell may not answer for the owner.
fn a_jobs_shell_may_not_answer(r: &Rig) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let refused = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(r.dir.path().join("sock"))
        .args(["card", "propose"])
        .env("THESEUS_SESSION", "ses_0000aa1b2c3")
        .current_dir(r.dir.path())
        .output()
        .unwrap();
    assert_ne!(refused.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("theseus card propose refused"));
}
