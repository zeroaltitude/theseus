//! scripts/prompt-step.sh, the gate's prompt phase (theseus-4vsrz, B7): it skips with a line when the binaries
//! the census runs on are missing, and it never fails the gate -- not on a missing binary, and not on a census
//! that errors. That is the whole point of the phase: the cliff's figure is recorded and warned of, never gated.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn step(target: &Path) -> Output {
    Command::new(root().join("scripts/prompt-step.sh"))
        .env("THESEUS_PROMPT_TARGET", target)
        .env("THESEUS_SIM", target.join("debug/theseus-sim"))
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// An executable at `path` that exits with `code`, saying nothing.
fn exe(path: &Path, code: u8) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/usr/bin/env bash\nexit {code}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn a_missing_binary_is_a_skip_that_says_which_one() {
    let d = tempfile::tempdir().unwrap();
    let none = step(d.path());
    assert!(none.status.success());
    assert!(
        text(&none).contains("prompt: skipped: no") && text(&none).contains("theseus-sim"),
        "{}",
        text(&none)
    );
    // The bench binary without the daemon it counts with is still a skip, and names the daemon.
    exe(&d.path().join("debug/theseus-sim"), 0);
    let no_daemon = step(d.path());
    assert!(no_daemon.status.success());
    assert!(
        text(&no_daemon).contains("prompt: skipped: no") && text(&no_daemon).contains("theseusd"),
        "{}",
        text(&no_daemon)
    );
}

/// A census that errors is reported and the phase still returns 0: this phase is a record and a warning, and a
/// second hard gate on an uncalibrated share would redden departures for ordinary drift (theseus-4vsrz).
#[test]
fn a_census_that_fails_is_reported_and_never_fails_the_phase() {
    let d = tempfile::tempdir().unwrap();
    exe(&d.path().join("debug/theseusd"), 0);
    exe(&d.path().join("debug/theseus-sim"), 3);
    let out = step(d.path());
    assert!(out.status.success(), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains("prompt: the census did not finish (exit 3)"),
        "{said}"
    );
    assert!(said.contains("never fails"), "{said}");
}
