//! `theseus tui` (theseus-7yx, step 10f) execs `theseus-tui`, found beside the
//! CLI or else on PATH, with the CLI's `--socket` and every further argument;
//! one found nowhere exits 2, saying where it looked and how to install it.
//! The workspace builds the real `theseus-tui` beside `target/debug/theseus`,
//! so each test links the CLI into a directory of its own, where what sits
//! beside it is the test's choice, and a shell script stands in for the TUI.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// A directory of the test's, on the target's filesystem, so the CLI can be
/// hard-linked into it.
fn scratch() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

/// The CLI, linked into `dir`.
fn cli_in(dir: &Path) -> PathBuf {
    let cli = dir.join("theseus");
    if std::fs::hard_link(THESEUS, &cli).is_err() {
        std::fs::copy(THESEUS, &cli).unwrap();
    }
    cli
}

/// A stand-in `theseus-tui` in `dir`: it prints its pid, then each argument.
fn stand_in(dir: &Path) {
    let tui = dir.join("theseus-tui");
    std::fs::write(
        &tui,
        "#!/bin/sh\necho \"pid $$\"\nfor a in \"$@\"; do echo \"arg [$a]\"; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&tui, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Runs `cli` with `args` and `PATH` set to `path`, and returns its pid, exit
/// code, stdout, and stderr.
fn run(
    cli: &Path,
    path: &Path,
    args: &[&str],
    socket_env: Option<&str>,
) -> (u32, i32, String, String) {
    let mut cmd = Command::new(cli);
    cmd.args(args)
        .env("PATH", path)
        .env_remove("THESEUS_SOCKET")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(s) = socket_env {
        cmd.env("THESEUS_SOCKET", s);
    }
    let child = cmd.spawn().unwrap();
    let pid = child.id();
    let out = child.wait_with_output().unwrap();
    (
        pid,
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// Beside the CLI: the stand-in runs in the CLI's own process (an exec, not a
/// child), with `--socket` and the arguments after `tui`, `--help` included.
#[test]
fn tui_execs_the_one_beside_it_with_the_socket_and_the_arguments() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let (pid, code, stdout, stderr) = run(
        &cli,
        empty.path(),
        &[
            "tui",
            "--socket",
            "/run/harbour/theseus.sock",
            "--notify",
            "off",
            "--help",
        ],
        None,
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "pid {pid}\narg [--socket]\narg [/run/harbour/theseus.sock]\narg [--notify]\narg [off]\n\
             arg [--help]\n"
        )
    );

    // `theseus tui --help` is the TUI's help, not clap's page for `tui`.
    let (pid, code, stdout, stderr) = run(&cli, empty.path(), &["tui", "--help"], None);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        format!("pid {pid}\narg [--socket]\narg [~/.theseus/theseus.sock]\narg [--help]\n")
    );
}

/// On PATH, when none is beside the CLI: the socket comes from
/// `THESEUS_SOCKET`, as for every command, and a `--socket` after the TUI's
/// own arguments is passed on as written (the TUI takes the last).
#[test]
fn tui_finds_it_on_path_and_passes_the_socket_from_the_environment() {
    let (bin, path) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(path.path());
    let (pid, code, stdout, stderr) = run(
        &cli,
        path.path(),
        &[
            "tui",
            "--notify",
            "bell",
            "--socket",
            "/run/harbour/later.sock",
        ],
        Some("/run/harbour/env.sock"),
    );
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(
        stdout,
        format!(
            "pid {pid}\narg [--socket]\narg [/run/harbour/env.sock]\narg [--notify]\narg [bell]\n\
             arg [--socket]\narg [/run/harbour/later.sock]\n"
        )
    );
}

/// Found nowhere: exit 2, with where it looked and how to install it; and
/// `--spawn`, which would show only a daemon of its own, is refused with 2.
#[test]
fn tui_found_nowhere_exits_2_saying_where_it_looked() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    let (_, code, stdout, stderr) = run(&cli, empty.path(), &["tui"], None);
    assert_eq!((code, stdout.as_str()), (2, ""), "{stderr}");
    let beside = bin.path().join("theseus-tui");
    let dir = bin.path().display();
    assert_eq!(
        stderr,
        format!(
            "theseus: theseus-tui is not installed: not beside this binary ({}), and not on PATH.\n\
             Install it beside theseus, from a checkout of Theseus:\n  \
             cargo build --release -p theseus-tui\n  \
             install -m 755 target/release/theseus-tui {dir}/\n",
            beside.display()
        )
    );

    stand_in(bin.path());
    let (_, code, stdout, stderr) = run(&cli, empty.path(), &["tui", "--spawn"], None);
    assert_eq!((code, stdout.as_str()), (2, ""));
    assert!(stderr.contains("it has no --spawn"), "{stderr}");
}
