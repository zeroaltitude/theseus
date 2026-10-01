//! `theseusd install` as the operator runs it (theseus-7hh): the real binary,
//! its real effective uid, and a scratch `HOME`. Nothing here reaches the
//! machine's own systemd directories, accounts, or `/`:
//! - with no mode, it lists the modes and what each needs;
//! - `--separate --apply` without root refuses, and says to use sudo, before
//!   it looks at anything;
//! - `--user` writes its unit under the scratch `XDG_CONFIG_HOME`, checks
//!   clean, changes nothing a second time, and `--remove` takes it away.
//!
//! Every test that could reach the machine is skipped when it runs as root.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn theseusd() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_theseusd"))
}

/// Whether this process runs as root: the `Uid:` line's second field.
fn root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|l| l.split_whitespace().nth(1).map(|e| e == "0"))
        .unwrap()
}

/// `theseusd install …` with a scratch home, and no Theseus settings but a
/// stand-in config source and token file.
fn install(home: &Path, args: &[&str]) -> Output {
    Command::new(theseusd())
        .arg("install")
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("THESEUS_CONFIG", "op://Example/theseus-config/notesPlain")
        .env("THESEUS_OP_TOKEN_FILE", home.join("op-token"))
        .current_dir(home)
        .output()
        .unwrap()
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn with_no_mode_it_lists_the_modes_and_what_each_needs() {
    let home = tempfile::tempdir().unwrap();
    let out = install(home.path(), &[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let says = text(&out.stdout);
    for line in [
        "--user       Your own daemon as a systemd user service",
        "Needs nothing but you",
        "--separate   The daemon as its own `theseus` user",
        "Needs root: run it with sudo.",
    ] {
        assert!(says.contains(line), "{line:?} missing from:\n{says}");
    }
}

#[test]
fn separate_without_root_refuses_and_names_sudo() {
    if root() {
        eprintln!("skipped: as root, this would install for real");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    for args in [
        &["--separate", "--apply"][..],
        &["--separate", "--remove", "--apply"],
        &["--separate", "--check"],
    ] {
        let out = install(home.path(), args);
        assert!(!out.status.success(), "{args:?} succeeded");
        let err = text(&out.stderr);
        assert!(
            err.contains("`--separate` needs root: run it with sudo") && err.contains("sudo "),
            "{args:?}: {err}"
        );
    }
}

#[test]
fn user_writes_its_unit_under_the_scratch_home_checks_clean_and_removes_it() {
    if root() {
        eprintln!("skipped: --user refuses root");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let unit = home.path().join("config/systemd/user/theseusd.service");
    let plan = install(home.path(), &["--user"]);
    assert!(plan.status.success(), "{}", text(&plan.stderr));
    assert!(
        text(&plan.stdout).contains("1 to do"),
        "{}",
        text(&plan.stdout)
    );
    assert!(!unit.exists(), "the plan wrote the unit");

    let apply = install(home.path(), &["--user", "--apply"]);
    assert!(apply.status.success(), "{}", text(&apply.stderr));
    let written = std::fs::read_to_string(&unit).unwrap();
    for line in [
        format!(
            "ExecStart={} --config op://Example/theseus-config/notesPlain --op-token-file {}",
            theseusd().display(),
            home.path().join("op-token").display()
        ),
        "Delegate=yes".into(),
        "KillSignal=SIGINT".into(),
        "Environment=\"PATH=/usr/bin:/bin\"".into(),
    ] {
        assert!(
            written.lines().any(|l| l == line),
            "{line:?} missing from:\n{written}"
        );
    }

    let check = install(home.path(), &["--user", "--check"]);
    assert_eq!(check.status.code(), Some(0), "{}", text(&check.stdout));
    let again = install(home.path(), &["--user", "--apply"]);
    assert!(
        text(&again.stdout).contains("Nothing to do"),
        "{}",
        text(&again.stdout)
    );

    std::fs::write(&unit, "[Unit]\n").unwrap();
    let differs = install(home.path(), &["--user", "--check"]);
    assert_eq!(differs.status.code(), Some(1), "{}", text(&differs.stdout));
    // A unit someone else wrote is not the installer's to remove.
    let keep = install(home.path(), &["--user", "--remove", "--apply"]);
    assert!(keep.status.success(), "{}", text(&keep.stderr));
    assert!(unit.exists());

    install(home.path(), &["--user", "--apply"]);
    let remove = install(home.path(), &["--user", "--remove", "--apply"]);
    assert!(remove.status.success(), "{}", text(&remove.stderr));
    assert!(!unit.exists());
}
