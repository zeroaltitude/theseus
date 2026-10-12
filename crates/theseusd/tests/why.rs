//! `theseus why` against a real daemon (theseus-ighf): two turns on the
//! stand-in model, the first reading a file, then `theseus why` for the last
//! turn and, by a piece of its id, the first. It prints what each turn was
//! shown and the gate's posture for the tool the first called, and the
//! daemon writes nothing for it: its newest ledger row and its count of rows
//! are the same after as before.

mod common;

use std::path::PathBuf;
use std::process::Command;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

const KEY_VAR: &str = "THESEUS_WHY_TEST_KEY";

fn cli(dir: &std::path::Path, args: &[&str]) -> (String, String, i32) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(dir.join("sock"))
        .args(args)
        .current_dir(dir.join("projects"))
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

fn ask(dir: &std::path::Path, prompt: &str, session: Option<&str>) -> Value {
    let mut args = vec!["--json", "ask"];
    if let Some(s) = session {
        args.extend(["--session", s]);
    }
    args.push(prompt);
    let (out, err, code) = cli(dir, &args);
    assert_eq!(code, 0, "{err}");
    serde_json::from_str(out.trim()).unwrap()
}

/// The newest ledger row's position and the count of rows.
fn ledger_end(dir: &std::path::Path) -> (u64, u64) {
    let (out, err, code) = cli(dir, &["--json", "ledger", "--n", "1"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    (
        v["rows"][0]["position"].as_u64().unwrap(),
        v["total"].as_u64().unwrap(),
    )
}

/// A daemon on the stand-in model in a temp dir, `tweak` applied to its
/// config first: the model reads `notes.txt` when asked to.
struct Rig {
    dir: tempfile::TempDir,
    _daemon: Daemon,
    _model: FakeModel,
}

fn rig(tweak: impl FnOnce(&mut toml::Table)) -> Rig {
    let model = FakeModel::start(|prompt| {
        if prompt.contains("read the notes") {
            vec![("fs_read", json!({"path": "notes.txt"}))]
        } else {
            vec![]
        }
    });
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("notes.txt"), "low water at six\n").unwrap();
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
    tweak(&mut t);
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
            .env(KEY_VAR, "tv-why-3c9a1f")
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
        _model: model,
    }
}

#[test]
fn why_reads_the_last_turn_and_an_older_one_by_its_id_and_writes_nothing() {
    let r = rig(|_| {});
    let dir = r.dir.path();
    let first = ask(dir, "read the notes, then say what they hold", None);
    let sid = first["session_id"].as_str().unwrap().to_string();
    let second = ask(dir, "and say hello", Some(&sid));
    let (t1, t2) = (
        first["turn_id"].as_str().unwrap().to_string(),
        second["turn_id"].as_str().unwrap().to_string(),
    );
    assert_ne!(t1, t2);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let before = ledger_end(dir);

    // The last turn: its context, and that it called no tool.
    let (out, err, code) = cli(dir, &["why"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with(&format!("turn {t2} · session {sid}")),
        "{out}"
    );
    assert!(out.contains("context: "), "{out}");
    assert!(out.contains("tools: the turn called none"), "{out}");

    // The older one, by the end of its id: the tool it called and the gate's posture.
    let end = &t1[t1.len() - 6..];
    let (out, err2, code) = cli(dir, &["why", end]);
    assert_eq!(code, 0, "{err2}");
    assert!(out.starts_with(&format!("turn {t1} · ")), "{out}");
    assert!(
        out.contains("tools called, and the gate's posture for each:"),
        "{out}"
    );
    let line = out
        .lines()
        .find(|l| l.trim_start().starts_with("fs.read"))
        .unwrap_or("");
    assert!(line.contains("open"), "fs_read's posture: {out}");

    // --json: the four answers, the turn's among them.
    let (out, err3, code) = cli(dir, &["--json", "why", "--session", &sid, end]);
    assert_eq!(code, 0, "{err3}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["context"]["turn_id"], t1.as_str());
    assert!(v["policy"]["places"].is_array() && v["nodes"]["nodes"].is_array());

    // Reads alone: no row was written for any of it.
    assert_eq!(ledger_end(dir), before);
}

/// A recall seeded by an earlier session: the index (BM25 alone, with no
/// model files) holds what session one said, recall is live, and a second
/// session's turn about the same words is shown that note. `theseus why`
/// names it, its source session, and why it ranked (its BM25 rank), and the
/// turn's own records are read, not written.
#[test]
fn why_shows_a_seeded_recall_its_source_and_its_reason() {
    let r = rig(|t| {
        let table = |t: &mut toml::Table, k: &str| {
            t.entry(k)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
                .clone()
        };
        let mut index = table(t, "index");
        index.insert("enabled".into(), true.into());
        index.insert(
            "weights_dir".into(),
            "/nonexistent/theseus-test-models".into(),
        );
        t.insert("index".into(), index.into());
        let mut memory = table(t, "memory");
        memory.insert("mode".into(), "live".into());
        t.insert("memory".into(), memory.into());
    });
    let dir = r.dir.path();
    let first = ask(
        dir,
        "Remember: the tide table for harbour zeta puts low water at six.",
        None,
    );
    let seed = first["session_id"].as_str().unwrap().to_string();
    // The tender starts after serving and indexes the store: wait until a
    // search finds the note.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let (out, _, code) = cli(
            dir,
            &["--json", "memory", "search", "tide table harbour zeta"],
        );
        let found = code == 0
            && serde_json::from_str::<Value>(out.trim())
                .is_ok_and(|v| v["admitted"].as_array().is_some_and(|a| !a.is_empty()));
        if found {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the index never held the note"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let second = ask(
        dir,
        "What does the tide table for harbour zeta say about low water?",
        None,
    );
    let (out, err, code) = cli(dir, &["why"]);
    assert_eq!(code, 0, "{err}");
    assert!(
        out.starts_with(&format!("turn {} · ", second["turn_id"].as_str().unwrap())),
        "{out}"
    );
    assert!(out.contains("recalled: 1 of "), "{out}");
    assert!(
        out.contains(&format!("session {seed}")),
        "its source session: {out}"
    );
    assert!(out.contains("why: ranked 1 of "), "{out}");
    assert!(out.contains("bm25 #1"), "{out}");
}
