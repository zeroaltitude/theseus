//! `theseus import sync` (theseus-pjmj7), the real CLI and its exporter against a
//! real daemon, on OpenClaw stores made at run time (scripts/tests/openclaw_fixture.py,
//! invented text, a planted secret built from parts): the first run asks for its
//! start; a run takes the sessions (a DM, a channel, a cron run, a zstd event, a
//! deleted session from the archive); a second run writes nothing; a session that
//! grew is appended to and a new one comes in; health names the run; the secret is stored
//! as it came; and the sync is refused inside a job. Skips without
//! python3.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::Served;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn python3() -> bool {
    Command::new("python3").arg("--version").output().is_ok()
}

fn fixture(args: &[&str]) -> String {
    let out = Command::new("python3")
        .arg("-B")
        .arg(root().join("scripts/tests/openclaw_fixture.py"))
        .args(args)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn cli(s: &Served, args: &[&str]) -> (i32, String, String) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(s.path("sock"))
        .args(args)
        .env_remove("THESEUS_SESSION")
        .env_remove("THESEUS_SOCKET")
        .env("TZ", "America/Phoenix")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Every byte under `dir` holds `needle`.
fn holds(dir: &Path, needle: &str) -> bool {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                if b.windows(needle.len()).any(|w| w == needle.as_bytes()) {
                    return true;
                }
            }
        }
    }
    false
}

/// The key the fixture plants, built from parts.
fn token() -> String {
    format!("{}_{}", "ghp", "Zy7aB9cD2eF4gH6iJ8kL0mN2oP4qR6sT8uV0")
}

fn served(state: &Path) -> Served {
    let state = state.to_string_lossy().into_owned();
    Served::start(
        |_| {},
        move |t| {
            let mut o = toml::Table::new();
            o.insert("state_dir".into(), state.into());
            o.insert(
                "owner_ids".into(),
                vec![toml::Value::from("U-OWNER")].into(),
            );
            let mut i = toml::Table::new();
            i.insert("openclaw".into(), o.into());
            t.insert("import".into(), i.into());
        },
    )
}

#[test]
fn a_sync_takes_what_changed_appends_what_grew_and_health_names_it() {
    if !python3() {
        eprintln!("skipped: no python3");
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let oc = scratch.path().join("openclaw");
    fixture(&["build", oc.to_str().unwrap()]);
    let s = served(&oc);
    let since = fixture(&["since"]);

    // The first run needs its start: nothing is imported to follow.
    let (code, _, err) = cli(&s, &["import", "sync"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("pass --since"), "{err}");

    // A dry run reads and writes nothing.
    let (code, out, err) = cli(&s, &["import", "sync", "--since", &since, "--dry-run"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(out.starts_with("dry run: 5 sessions"), "{out}");
    let (_, listed, _) = cli(&s, &["import", "list"]);
    assert!(listed.contains("no imports"), "{listed}");

    // The run: five sessions, the DM's operator and agent, the channel's other person.
    let (code, out, err) = cli(&s, &["import", "sync", "--since", &since, "--no-people"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.starts_with(
            "sync: 5 sessions read (11 messages): 5 new, 0 appended to, 0 already held, 0 refused;"
        ),
        "{out}"
    );
    let (_, listed, _) = cli(&s, &["import", "list"]);
    assert!(
        listed.contains("5 sessions, 11 nodes") && listed.contains("openclaw-sync 5"),
        "{listed}"
    );
    let (_, health, _) = cli(&s, &["health"]);
    assert!(health.contains("sync: "), "{health}");
    assert!(health.contains("5 new, 0 appended"), "{health}");

    // The record is raw: the planted key is in the daemon's files as it came, with its text.
    assert!(
        holds(&s.path("state"), "boathouse ledger"),
        "the grep reads the state"
    );
    assert!(
        holds(&s.path("state"), &token()),
        "the store holds the key as the session said it"
    );

    // A second run writes nothing, and a cursor refuses another start.
    let (code, out, err) = cli(&s, &["import", "sync", "--no-people"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.starts_with("sync: 0 sessions read (0 messages): 0 new, 0 appended to"),
        "{out}"
    );
    let (code, _, err) = cli(&s, &["import", "sync", "--since", &since]);
    assert_eq!(code, 1);
    assert!(err.contains("a cursor exists"), "{err}");

    // The DM grew and a session began: one is appended to, one is new.
    fixture(&["grow", oc.to_str().unwrap()]);
    let (code, out, err) = cli(&s, &["import", "sync", "--no-people"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(
        out.starts_with(
            "sync: 2 sessions read (5 messages): 1 new, 1 appended to, 0 already held, 0 refused;"
        ),
        "{out}"
    );
    let (_, listed, _) = cli(&s, &["import", "list"]);
    assert!(listed.contains("6 sessions, 14 nodes"), "{listed}");
    assert!(
        holds(&s.path("state"), &token()),
        "the key stays as the session said it"
    );
}

/// Inside a job the sync is refused before the exporter runs.
#[test]
fn the_sync_is_refused_inside_a_job() {
    if !python3() {
        return;
    }
    let scratch = tempfile::tempdir().unwrap();
    let oc = scratch.path().join("openclaw");
    fixture(&["build", oc.to_str().unwrap()]);
    let s = served(&oc);
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(s.path("sock"))
        .args(["import", "sync", "--since", "2026-01-01T00:00:00Z"])
        .env("THESEUS_SESSION", "ses_0000aa1b2c3")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("theseus import sync refused"), "{err}");
    assert!(err.contains("Run it from your own shell"), "{err}");
    assert!(!holds(&s.path("state"), "boathouse ledger"));
}

/// The exporter's own tests, in the gate.
#[test]
fn the_exporters_own_tests_pass() {
    if !python3() {
        return;
    }
    let out = Command::new("python3")
        .arg("-B")
        .args(["-m", "unittest", "scripts/tests/test_openclaw_export.py"])
        .current_dir(root())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("OK"), "{err}");
}
