//! The state dir is named once, however the operator spelled it (theseus-0h9f3,
//! B6c L22). `--state-dir` and `THESEUS_STATE_DIR` reach the daemon unexpanded
//! when no shell did the work (a quoted argument, a unit's environment), and
//! every command that touches the directory must then name the same one a start
//! would: `~` as `$HOME`, `$NAME` and `${NAME}` as the variable. `theseusd
//! restore` took the flag raw and wrote a literal `~` or `$NAME` directory under
//! its cwd while a start expanded it, so the restored store was never the one
//! the daemon opened.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::{safe_note, table, Daemon};
use serde_json::json;
use theseus_core::session::SessionRecord;
use theseus_core::store::Store;

struct Rig {
    dir: tempfile::TempDir,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        for d in ["bin", "projects", "cwd", "home", "base"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        std::fs::write(path("bin/op"), "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { dir }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    /// The test's safe config in `config.toml`, with `tweak` applied.
    fn config(&self, tweak: impl FnOnce(&mut toml::Table)) {
        let theseusd = Path::new(env!("CARGO_BIN_EXE_theseusd"));
        let mut t: toml::Table = safe_note(theseusd, &self.path("projects"), 100.0)
            .parse()
            .unwrap();
        tweak(&mut t);
        std::fs::write(self.path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    }

    /// `theseusd`, run in an empty `cwd`, with `HOME` and `STATE_BASE` set,
    /// so a literal `~` or `$STATE_BASE` directory has one place to appear.
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_theseusd"));
        c.current_dir(self.path("cwd"))
            .arg("--config")
            .arg(self.path("config.toml"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.path("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("HOME", self.path("home"))
            .env("STATE_BASE", self.path("base"))
            .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .stdin(Stdio::null());
        c
    }

    /// A WAL with one session in it, to restore from.
    fn wal_with_one_session(&self) -> PathBuf {
        let src = self.path("src");
        let store = Store::open(&src.join("store")).unwrap();
        let rec = SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            Some("kept".into()),
        );
        store.put_session(&rec.session_id, &rec).unwrap();
        drop(store);
        src.join("store/wal")
    }

    fn entries(&self, dir: &str) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.path(dir))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}

fn said(o: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Each spelling of the same directory, as a start would resolve it, and
/// `restore` with it: the store is rebuilt in the expanded directory and the
/// cwd is left empty. Red if `restore` takes `--state-dir` as typed.
#[test]
fn restore_names_the_directory_a_start_would() {
    for (flag, expanded) in [
        ("~/restored", "home/restored"),
        ("~", "home"),
        ("$STATE_BASE/restored", "base/restored"),
        ("${STATE_BASE}/restored", "base/restored"),
    ] {
        let r = Rig::new();
        r.config(|_| {});
        let wal = r.wal_with_one_session();
        let out = r
            .command()
            .args(["--state-dir", flag, "restore", "--from"])
            .arg(&wal)
            .output()
            .unwrap();
        assert!(out.status.success(), "{flag}: {}", said(&out));
        let restored = Store::open(&r.path(expanded).join("store")).unwrap();
        assert_eq!(restored.session_count().unwrap(), 1, "{flag}");
        assert_eq!(
            r.entries("cwd"),
            Vec::<String>::new(),
            "{flag}: a literal directory was made in the cwd"
        );
    }
}

/// The same through `THESEUS_STATE_DIR`, which clap reads into the same field.
#[test]
fn restore_expands_the_environment_spelling_too() {
    let r = Rig::new();
    r.config(|_| {});
    let wal = r.wal_with_one_session();
    let out = r
        .command()
        .env("THESEUS_STATE_DIR", "${STATE_BASE}/from-env")
        .args(["restore", "--from"])
        .arg(&wal)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", said(&out));
    let restored = Store::open(&r.path("base/from-env/store")).unwrap();
    assert_eq!(restored.session_count().unwrap(), 1);
    assert_eq!(r.entries("cwd"), Vec::<String>::new());
}

/// What does not resolve stays as written, here as on a start (`config::
/// expand_with`): an unset variable is a directory of that name under the cwd,
/// the same place a start would open it. Not a refusal, not a guess.
#[test]
fn restore_leaves_what_does_not_resolve_as_a_start_does() {
    let r = Rig::new();
    r.config(|_| {});
    let wal = r.wal_with_one_session();
    let out = r
        .command()
        .env_remove("NOT_SET_ANYWHERE")
        .args(["--state-dir", "$NOT_SET_ANYWHERE/s", "restore", "--from"])
        .arg(&wal)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", said(&out));
    assert!(r.path("cwd/$NOT_SET_ANYWHERE/s/store").is_dir());
}

/// A `--stdio` daemon keeps its store beside the socket daemon's, in the same
/// state dir (`store-stdio`), and `persona.list` finds a persona's manifest
/// there: the persona folder is under the state dir the flag named, however
/// the flag was spelled. The manifest is written by the import itself.
#[test]
fn a_stdio_daemon_reads_personas_from_the_flagged_state_dir() {
    use theseus_protocol::{PersonaImportParams, PersonaListParams};

    let r = Rig::new();
    let from =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../theseus-core/tests/fixtures/agents/wren");
    let state = r.path("base/stdio-state");
    let draft = theseus_core::persona_import::run::import(
        &state,
        &PersonaImportParams {
            from: from.display().to_string(),
            name: "wren".into(),
            ..Default::default()
        },
        &theseus_core::config::PersonaSettings::default().import_files,
        64 * 1024,
        1,
    )
    .unwrap()
    .0
    .card
    .unwrap();
    r.config(|t| {
        let wren = table(table(t, "personas"), "wren");
        wren.insert("card".into(), draft.clone().into());
    });

    let log = std::fs::File::create(r.path("theseusd.log")).unwrap();
    let mut d = Daemon::spawn(
        r.command()
            .args(["--stdio", "--state-dir", "${STATE_BASE}/stdio-state"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log),
    );
    let mut client = common::stdio::StdioClient::new(&mut d);
    let list = client.call(
        theseus_protocol::method::PERSONA_LIST,
        serde_json::to_value(PersonaListParams::default()).unwrap(),
    );
    let log_text = std::fs::read_to_string(r.path("theseusd.log")).unwrap_or_default();
    let list = list.unwrap_or_else(|e| panic!("persona.list: {e}\n{log_text}"));
    let wren = list["personas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["key"] == json!("wren"))
        .unwrap_or_else(|| panic!("no wren in {list}\n{log_text}"));
    assert_eq!(
        wren["card_draft"],
        json!(true),
        "the manifest under the flagged state dir was not found: {wren}\n{log_text}"
    );
    assert!(r.path("base/stdio-state/store-stdio").is_dir());
    assert!(!r.path("base/stdio-state/store").exists());
    assert_eq!(r.entries("cwd"), Vec::<String>::new());
}
