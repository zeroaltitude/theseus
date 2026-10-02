//! `theseusd install` as the operator runs it (theseus-7hh): the real binary,
//! its real effective uid, and a scratch `HOME`. Nothing here reaches the
//! machine's own systemd directories, accounts, or `/`:
//! - with no mode, it lists the modes and what each needs;
//! - `--separate --apply` without root refuses, and says to use sudo, before
//!   it looks at anything;
//! - `--user` writes its unit under the scratch `XDG_CONFIG_HOME`, checks
//!   clean, changes nothing a second time, and `--remove` takes it away;
//! - `--op-token-file` works before the subcommand and after it, and the plan's
//!   hint is a command that runs (theseus-w1nf);
//! - a token file that is wrong is named in the plan with the fix, and
//!   `--apply` writes nothing until it is right.
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

/// `theseusd` with a scratch home, and no Theseus settings but a stand-in
/// config source: no token file, which a test names by flag or variable.
fn theseusd_in(home: &Path) -> Command {
    let mut c = Command::new(theseusd());
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("THESEUS_CONFIG", "op://Example/theseus-config/notesPlain")
        .current_dir(home);
    c
}

/// `theseusd install …` with a scratch home, and a stand-in config source
/// and token file (named by the variable the daemon reads).
fn install(home: &Path, args: &[&str]) -> Output {
    theseusd_in(home)
        .arg("install")
        .args(args)
        .env("THESEUS_OP_TOKEN_FILE", home.join("op-token"))
        .output()
        .unwrap()
}

/// The scratch home's token file: the operator's own (this process made it),
/// with this mode, holding a stand-in text that no output may carry.
fn token(home: &Path, mode: u32) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let at = home.join("op-token");
    std::fs::write(&at, "SENTINEL-not-a-token-DO-NOT-PRINT\n").unwrap();
    std::fs::set_permissions(&at, std::fs::Permissions::from_mode(mode)).unwrap();
    at
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
    token(home.path(), 0o600);
    let unit = home.path().join("config/systemd/user/theseusd.service");
    let plan = install(home.path(), &["--user"]);
    assert!(plan.status.success(), "{}", text(&plan.stderr));
    assert!(
        text(&plan.stdout).contains("1 to do, 1 already done."),
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

#[test]
fn the_token_file_flag_works_before_or_after_the_subcommand() {
    if root() {
        eprintln!("skipped: --user refuses root");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let tok = token(home.path(), 0o600);
    let tok = tok.to_str().unwrap();
    let want = format!(
        "ExecStart={} --config op://Example/theseus-config/notesPlain --op-token-file {tok}",
        theseusd().display()
    );
    for args in [
        &["--op-token-file", tok, "install", "--user"][..],
        &["install", "--user", "--op-token-file", tok],
        &["install", "--op-token-file", tok, "--user"],
    ] {
        let out = theseusd_in(home.path()).args(args).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        let says = text(&out.stdout);
        assert!(
            says.contains(&want),
            "{args:?}: {want:?} missing from:\n{says}"
        );
        assert!(says.contains("  ok      token  "), "{args:?}:\n{says}");
    }
}

#[test]
fn the_plans_hint_is_a_command_that_runs() {
    if root() {
        eprintln!("skipped: --user refuses root");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    // No token file named: the plan says how to name one.
    let plan = theseusd_in(home.path())
        .args(["install", "--user"])
        .output()
        .unwrap();
    assert!(plan.status.success(), "{}", text(&plan.stderr));
    let says = text(&plan.stdout);
    let hint = says
        .lines()
        .find_map(|l| l.split_once("then re-run: "))
        .unwrap_or_else(|| panic!("no hint in:\n{says}"))
        .1;
    let command = hint
        .split_once(" <file>.")
        .unwrap_or_else(|| panic!("the hint does not end with `<file>.`: {hint}"))
        .0;
    assert_eq!(
        command,
        format!("{} install --user --op-token-file", theseusd().display())
    );
    // The line as it stands is the plan's own flag order; it must run.
    let tok = token(home.path(), 0o600);
    let mut words = command.split_whitespace();
    let program = words.next().unwrap();
    let out = theseusd_in(home.path())
        .args(words)
        .arg(&tok)
        .output()
        .unwrap();
    assert!(out.status.success(), "{command}: {}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("  ok      token  "),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(program, theseusd().to_str().unwrap());
}

#[test]
fn a_wrong_token_file_is_named_in_the_plan_and_apply_writes_nothing_until_it_is_right() {
    if root() {
        eprintln!("skipped: --user refuses root");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let unit = home.path().join("config/systemd/user/theseusd.service");
    let tok = token(home.path(), 0o644);
    let word = tok.display().to_string();
    let plan = install(home.path(), &["--user"]);
    assert_eq!(plan.status.code(), Some(1), "{}", text(&plan.stdout));
    let says = text(&plan.stdout);
    assert!(
        says.contains(&format!(
            "mode 0644 is looser than 0600. Fix: chmod 600 {word}"
        )),
        "{says}"
    );
    assert!(
        says.contains("1 refused: --apply changes nothing until each is resolved."),
        "{says}"
    );

    let apply = install(home.path(), &["--user", "--apply"]);
    assert!(!apply.status.success(), "{}", text(&apply.stdout));
    assert!(
        text(&apply.stderr).contains("nothing was changed: 1 refused"),
        "{}",
        text(&apply.stderr)
    );
    assert!(
        text(&apply.stdout).contains("mode 0644 is looser than 0600"),
        "{}",
        text(&apply.stdout)
    );
    assert!(!unit.exists(), "a refused --apply wrote the unit");
    for o in [&plan, &apply] {
        let all = [text(&o.stdout), text(&o.stderr)].concat();
        assert!(
            !all.contains("SENTINEL"),
            "the token's text reached the output:\n{all}"
        );
    }

    token(home.path(), 0o600);
    let apply = install(home.path(), &["--user", "--apply"]);
    assert!(apply.status.success(), "{}", text(&apply.stderr));
    assert!(unit.exists());
}
