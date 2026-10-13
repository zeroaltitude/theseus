//! `session.fork` on a real daemon (theseus-2qzx, R19): the live check. The
//! real CLI spawns the real `theseusd --stdio` on a config with no vault and
//! the stand-in model: three turns, `theseus fork` at the second, a turn in
//! the fork, `theseus history` of both, and `theseus reach` of turn one. The
//! daemon's state dir is shared between the CLI's runs, so each is a daemon
//! started on the same store, as a restart is.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use serde_json::{json, Value};

const KEY_VAR: &str = "THESEUS_FORK_TEST_KEY";

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

/// A temp dir with `config.toml` (the safe template, every secret from
/// `KEY_VAR`, both providers at the stand-in) and `projects/`.
fn rig(model: &FakeModel) -> tempfile::TempDir {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut t: toml::Table = common::safe_note(&theseusd, &projects, 100.0)
        .parse()
        .unwrap();
    set(&mut t, &["model", "api_base"], model.base.clone().into());
    let names = |t: &toml::Table, key: &str| -> Vec<String> {
        t.get(key)
            .and_then(toml::Value::as_table)
            .map(|v| v.keys().cloned().collect())
            .unwrap_or_default()
    };
    for name in names(&t, "providers") {
        set(
            &mut t,
            &["providers", name.as_str(), "api_base"],
            model.base.clone().into(),
        );
    }
    for name in names(&t, "secrets") {
        set(
            &mut t,
            &["secrets", name.as_str()],
            format!("env:{KEY_VAR}").into(),
        );
    }
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

/// `theseus --spawn <theseusd> ARGS…` in `dir`, run to its end.
fn cli(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .current_dir(dir.join("projects"))
        .arg("--spawn")
        .arg(&theseusd)
        .args(args)
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
        .env("XDG_STATE_HOME", dir.join("xdg-state"))
        .env(KEY_VAR, "tv-fork-test-5d1e")
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn json_of(dir: &Path, args: &[&str]) -> Value {
    let mut full = vec!["--json"];
    full.extend_from_slice(args);
    let (code, out, err) = cli(dir, &full);
    assert_eq!(code, 0, "theseus {args:?}: {err}");
    serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{args:?}: {e}: {out}"))
}

/// The text of each message of a request, in order.
fn texts(request: &Value) -> Vec<String> {
    request["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            m["content"]
                .as_array()
                .map(|b| {
                    b.iter()
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn a_fork_of_the_second_turn_takes_the_other_road_and_both_stay() {
    let model = FakeModel::start(|_| vec![]);
    let dir = rig(&model);
    let first = json_of(dir.path(), &["ask", "turn one"]);
    let source = first["session_id"].as_str().unwrap().to_string();
    for t in ["turn two", "turn three"] {
        json_of(dir.path(), &["ask", "-s", &source, t]);
    }

    // The nodes of the source, as history names them.
    let (_, history, _) = cli(dir.path(), &["history", &source]);
    let named: Vec<&str> = history
        .lines()
        .filter_map(|l| l.split_whitespace().find(|w| w.starts_with("msg·")))
        .collect();
    assert_eq!(named.len(), 6, "{history}");
    // Fork at the answer to turn two.
    let at = named[3];
    let (code, forked, err) = cli(dir.path(), &["fork", at, "--title", "the other road"]);
    assert_eq!(code, 0, "{err}");
    eprintln!("$ theseus fork {at} --title \"the other road\"\n{forked}");
    assert!(forked.contains("forked ses_…"), "{forked}");
    assert!(forked.contains(&format!("at {at} (position")), "{forked}");
    assert!(forked.contains("\"the other road\""), "{forked}");
    assert!(
        forked.contains("not restored: files and what programs did after the fork point"),
        "{forked}"
    );

    // A turn in the fork.
    let sessions = json_of(dir.path(), &["sessions"]);
    let fork = sessions["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["title"] == "the other road")
        .unwrap_or_else(|| panic!("{sessions}"))["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    json_of(dir.path(), &["ask", "-s", &fork, "the other road, please"]);
    let request = model.requests().pop().unwrap();
    let said = texts(&request);
    assert_eq!(said.len(), 5, "{said:?}");
    assert_eq!(said[0], "turn one");
    assert_eq!(said[2], "turn two");
    assert_eq!(said[4], "the other road, please");
    assert!(!said.iter().any(|t| t.contains("turn three")), "{said:?}");

    // History of both: the fork shows the prefix marked as the source's,
    // then its own turn; the source shows all three of its own.
    let (_, fork_history, _) = cli(dir.path(), &["history", &fork]);
    eprintln!("$ theseus history <the fork>\n{fork_history}");
    assert!(
        fork_history.contains("── inherited from ses_…"),
        "{fork_history}"
    );
    assert!(
        fork_history.contains("── this session's own turns"),
        "{fork_history}"
    );
    assert!(
        fork_history.contains("the other road, please"),
        "{fork_history}"
    );
    assert!(!fork_history.contains("turn three"), "{fork_history}");
    let (_, source_history, _) = cli(dir.path(), &["history", &source]);
    assert!(source_history.contains("turn three"), "{source_history}");
    assert!(
        !source_history.contains("the other road, please"),
        "{source_history}"
    );
    assert!(!source_history.contains("── inherited"), "{source_history}");

    // Reach of turn one lists the fork as an exposure.
    let first_node = named[0];
    let reach = json_of(dir.path(), &["reach", first_node]);
    let forks = reach["forks"].as_array().unwrap();
    assert_eq!(forks.len(), 1, "{reach}");
    assert_eq!(forks[0]["session_id"], json!(fork), "{reach}");
    let (_, plain, _) = cli(dir.path(), &["reach", first_node]);
    eprintln!("$ theseus reach {first_node}\n{plain}");
    assert!(plain.contains("forked from"), "{plain}");
    assert!(plain.contains("by reference"), "{plain}");
}
