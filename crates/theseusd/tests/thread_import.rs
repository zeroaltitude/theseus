//! `theseus thread import` (theseus-71ib, T11), the real CLI against a real
//! daemon on the stand-in model, over the invented export in
//! `crates/theseus-core/tests/fixtures/threads-import/`: the dry run's table
//! and plan file (and nothing written), `--apply all-ticked` making exactly
//! the ticked threads with cards whose handles `theseus history` reads back,
//! the model never sent a partner-confidential line, a second apply skipping
//! what exists, and a Vestige dump imported once.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

const KEY_VAR: &str = "THESEUS_THREAD_IMPORT_TEST_KEY";

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

/// The card the stand-in model writes for a thread, by the title in its
/// request: every line cites episodes the request offered.
fn cards(prompt: &str) -> Vec<(&'static str, Value)> {
    let lines = if prompt.contains("Thread: tide-gauges") {
        json!([
            {"text": "The gauge sync lives on lane/gauge-sync; the drift fix is pull request 12.", "episodes": ["E3"]},
            {"text": "The fix was to ship that night.", "episodes": ["E1"]},
        ])
    } else if prompt.contains("Thread: boat-ops") {
        json!([{"text": "The skiff's mooring line is to be replaced.", "episodes": ["E1"]}])
    } else {
        return vec![];
    };
    vec![("write_card", json!({ "lines": lines }))]
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
    let personas: toml::Table = "[wren]\nfiles = []\n".parse().unwrap();
    t.insert("personas".into(), personas.into());
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
            .env(KEY_VAR, "tv-thread-import-5d2c")
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

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../theseus-core/tests/fixtures/threads-import/openclaw.jsonl")
}

#[test]
fn a_dry_run_then_the_ticked_threads_with_cards_and_a_vestige_dump() {
    let r = rig();
    let dir = r.dir.path();
    let (out, err, code) = cli(dir, &["import", "openclaw", fixture().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}\n{err}");
    let plan = dry_run(&r);
    apply_the_ticked(&r, &plan);
    the_cards_and_their_handles(&r);
    the_model_saw_no_partner_line(&r);
    a_second_apply_skips(&r);
    a_vestige_dump_once(&r);
}

/// The dry run: the table, the plan file, and nothing made.
fn dry_run(r: &Rig) -> String {
    let dir = r.dir.path();
    let dry = [
        "thread",
        "import",
        "--from",
        "openclaw",
        "--since-days",
        "3650",
    ];
    let (out, err, code) = cli(dir, &dry);
    assert_eq!(code, 0, "{out}\n{err}");
    for want in [
        "tide-gauges",
        "boat-ops",
        "archive",
        "dm-harbor-moorings",
        "confidential-4777",
        "wren",
        "reef-lab/gauge-log",
        "left out:",
    ] {
        assert!(out.contains(want), "{want} in:\n{out}");
    }
    assert!(!out.contains("dm-garden-irrigation"), "{out}");
    for leak in ["KESTREL", "kestrel", "term-sheet"] {
        assert!(!out.contains(leak), "{leak} in the table:\n{out}");
    }
    let plan_path = dir.join("thread-import-plan.txt");
    let plan = std::fs::read_to_string(&plan_path).unwrap();
    assert_eq!(
        plan.lines().filter(|l| l.starts_with("[ ] ")).count(),
        5,
        "{plan}"
    );
    let (listed, _, _) = cli(dir, &["thread", "list"]);
    assert!(listed.starts_with("No threads here"), "{listed}");
    assert!(r.model.requests().is_empty(), "the dry run asks no model");

    plan
}

/// Nothing ticked is refused; two ticked are made.
fn apply_the_ticked(r: &Rig, plan: &str) {
    let dir = r.dir.path();
    let plan_path = dir.join("thread-import-plan.txt");
    let (_, err, code) = cli(
        dir,
        &[
            "thread",
            "import",
            "--from",
            "openclaw",
            "--apply",
            "all-ticked",
        ],
    );
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("no thread is ticked"), "{err}");
    let ticked: String = plan
        .lines()
        .map(|l| {
            if l.contains(" tide-gauges ") || l.contains(" boat-ops ") {
                l.replacen("[ ]", "[x]", 1)
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&plan_path, ticked).unwrap();
    let (out, err, code) = cli(
        dir,
        &[
            "thread",
            "import",
            "--from",
            "openclaw",
            "--since-days",
            "3650",
            "--apply",
            "all-ticked",
        ],
    );
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.contains("Made the thread tide-gauges: 2 card lines citing 2 episodes"),
        "{out}"
    );
    assert!(
        out.contains("Made the thread boat-ops: 1 card lines citing 1 episodes"),
        "{out}"
    );
    assert!(out.contains("2 made, 0 skipped"), "{out}");
}

/// Exactly those two threads; the card cites by handle, and a handle names its node.
fn the_cards_and_their_handles(r: &Rig) {
    let dir = r.dir.path();
    let (listed, _, _) = cli(dir, &["--json", "thread", "list"]);
    let listed: Value = serde_json::from_str(listed.trim()).unwrap();
    assert_eq!(listed["total"], 2, "{listed}");
    let (shown, err, code) = cli(dir, &["--json", "thread", "show", "tide-gauges"]);
    assert_eq!(code, 0, "{err}");
    let shown: Value = serde_json::from_str(shown.trim()).unwrap();
    let t = &shown["thread"];
    assert_eq!(t["persona"], "wren");
    assert_eq!(t["working_set"]["repos"][0], "reef-lab/gauge-log");
    assert!(
        t["session_id"].is_null(),
        "no session until its first message"
    );
    let card = t["card"].as_array().unwrap();
    assert_eq!(card.len(), 2, "{card:?}");
    let first = card[0].as_str().unwrap();
    assert!(
        first.starts_with("The gauge sync lives on lane/gauge-sync"),
        "{first}"
    );
    let handle_of = |line: &str| {
        let h = line
            .rsplit('[')
            .next()
            .unwrap()
            .trim_end_matches(']')
            .to_string();
        assert!(h.starts_with("m:"), "{line}");
        h
    };
    // The summary's handle names the summary node of the first episode; the
    // owner line's handle names the message it cites (episode 3's first).
    let (hist, err, code) = cli(dir, &["history", &handle_of(first)]);
    assert_eq!(code, 0, "{err}");
    assert!(
        hist.contains("imported_summary") && hist.contains("7e48"),
        "{hist}"
    );
    let (hist, err, code) = cli(dir, &["history", &handle_of(card[1].as_str().unwrap())]);
    assert_eq!(code, 0, "{err}");
    assert!(
        hist.contains("imp·7e4a_0 imported"),
        "the handle names the cited message: {hist}"
    );
}

/// The model saw two cards' worth of episodes and no partner line.
fn the_model_saw_no_partner_line(r: &Rig) {
    let sent: Vec<String> = r.model.requests().iter().map(|v| v.to_string()).collect();
    assert_eq!(sent.len(), 2, "one call a thread");
    for body in &sent {
        for leak in ["KESTREL", "term-sheet", "second tranche"] {
            assert!(!body.contains(leak), "{leak} was sent: {body}");
        }
    }
}

/// A second apply: the threads exist.
fn a_second_apply_skips(r: &Rig) {
    let dir = r.dir.path();
    let (out, _, code) = cli(
        dir,
        &[
            "thread",
            "import",
            "--from",
            "openclaw",
            "--since-days",
            "3650",
            "--apply",
            "all-ticked",
        ],
    );
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("0 made, 2 skipped") && out.contains("exists"),
        "{out}"
    );
    assert_eq!(r.model.requests().len(), 2, "no new call");
}

/// A Vestige dump: imported once.
fn a_vestige_dump_once(r: &Rig) {
    let dir = r.dir.path();
    // A Vestige dump: imported once.
    let dump = dir.join("vestige.json");
    std::fs::write(
        &dump,
        r#"{"memories": [{"content": "The skiff key hangs in the boathouse.", "tags": ["boats"]},
            "Low water is at six on the north quay."]}"#,
    )
    .unwrap();
    let v = ["thread", "import", "--vestige", dump.to_str().unwrap()];
    let (out, err, code) = cli(dir, &v);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.contains("2 notes read, 2 imported, 0 already imported"),
        "{out}"
    );
    let (out, _, _) = cli(dir, &v);
    assert!(
        out.contains("2 notes read, 0 imported, 2 already imported"),
        "{out}"
    );
    let (listed, _, _) = cli(dir, &["import", "list"]);
    assert!(
        listed.contains("vestige:") && listed.contains("2 sessions"),
        "{listed}"
    );
}
