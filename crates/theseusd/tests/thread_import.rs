//! `theseus thread import --vestige` (theseus-71ib), the real CLI against a
//! real daemon: a Vestige dump imported once, through the session import's
//! `import.episodes`. The thread importer that shared the command (`--from`,
//! `--apply`) is gone (the owner, 2026-11-21), and the CLI says so.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use common::Daemon;

const KEY_VAR: &str = "THESEUS_THREAD_IMPORT_TEST_KEY";

fn cli(dir: &Path, args: &[&str]) -> (String, String, i32) {
    let out = Command::new(common::cli_bin())
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
    _model: FakeModel,
}

fn rig() -> Rig {
    let model = FakeModel::start(|_| vec![]);
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
        _model: model,
    }
}

#[test]
fn a_vestige_dump_is_imported_once_and_the_thread_importer_is_gone() {
    let r = rig();
    a_vestige_dump_once(&r);
    let (out, err, code) = cli(r.dir.path(), &["thread", "import", "--from", "openclaw"]);
    assert_ne!(code, 0, "{out}\n{err}");
    assert!(err.contains("--from"), "{err}");
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
