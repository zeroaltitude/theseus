//! Bare `theseus` and `theseus chat` (theseus-cyha): on a terminal both exec
//! `theseus-tui chat` (found beside the CLI or on PATH, with `--socket`
//! first), and a pipe, a script or any argument keeps today's usage text and
//! exit 2. A pseudo-terminal stands in for the person's terminal, and a shell
//! script for `theseus-tui`.

use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap()
}

fn cli_in(dir: &Path) -> PathBuf {
    let cli = dir.join("theseus");
    if std::fs::hard_link(THESEUS, &cli).is_err() {
        std::fs::copy(THESEUS, &cli).unwrap();
    }
    cli
}

/// A stand-in `theseus-tui` that prints each argument.
fn stand_in(dir: &Path) {
    let tui = dir.join("theseus-tui");
    std::fs::write(
        &tui,
        "#!/bin/sh\nfor a in \"$@\"; do echo \"arg [$a]\"; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&tui, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A pseudo-terminal: the master we read, and the slave the child gets as its
/// stdin and stdout.
fn pty() -> (std::fs::File, OwnedFd) {
    let (mut m, mut s) = (0, 0);
    // SAFETY: openpty writes two descriptors, which are ours to own.
    let rc = unsafe {
        libc::openpty(
            &mut m,
            &mut s,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    assert_eq!(rc, 0, "openpty");
    // SAFETY: both are fresh descriptors from openpty.
    unsafe { (std::fs::File::from_raw_fd(m), OwnedFd::from_raw_fd(s)) }
}

/// Run `cli args` with a terminal for stdin and stdout; the exit code and what
/// it wrote to the terminal.
fn on_a_terminal(cli: &Path, path: &Path, args: &[&str]) -> (i32, String) {
    let (mut master, slave) = pty();
    let mut child = Command::new(cli)
        .args(args)
        .env("PATH", path)
        .env("NO_COLOR", "1")
        .env_remove("THESEUS_SOCKET")
        .stdin(Stdio::from(slave.try_clone().unwrap()))
        .stdout(Stdio::from(slave))
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The slave is closed here, so the master reads to the end once the child
    // exits.
    let mut out = Vec::new();
    let _ = master.read_to_end(&mut out);
    let status = child.wait().unwrap();
    (
        status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out).replace('\r', ""),
    )
}

#[test]
fn bare_theseus_on_a_terminal_opens_the_chat() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let (code, out) = on_a_terminal(&cli, empty.path(), &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "arg [--socket]\narg [~/.theseus/theseus.sock]\narg [chat]\n"
    );
}

#[test]
fn theseus_chat_passes_the_session_and_the_environments_socket() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let (code, out) = on_a_terminal(&cli, empty.path(), &["chat", "a1b2c3"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "arg [--socket]\narg [~/.theseus/theseus.sock]\narg [chat]\narg [a1b2c3]\n"
    );
    let (code, out) = on_a_terminal(
        &cli,
        empty.path(),
        &["--socket", "/run/harbour/t.sock", "chat"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(
        out,
        "arg [--socket]\narg [/run/harbour/t.sock]\narg [chat]\n"
    );
}

#[test]
fn dash_c_and_dash_r_reopen_and_pick_in_the_chat() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let want = |last: &str| {
        format!("arg [--socket]\narg [~/.theseus/theseus.sock]\narg [chat]\narg [{last}]\n")
    };
    for (args, flag) in [
        (vec!["-c"], "-c"),
        (vec!["-r"], "-r"),
        (vec!["chat", "-c"], "-c"),
        (vec!["chat", "-r"], "-r"),
    ] {
        let (code, out) = on_a_terminal(&cli, empty.path(), &args);
        assert_eq!(code, 0, "{args:?}: {out}");
        assert_eq!(out, want(flag), "{args:?}");
    }
    // A session and `-c` together, or `-c` beside other words, are usage errors.
    let (code, _) = on_a_terminal(&cli, empty.path(), &["chat", "a1b2c3", "-c"]);
    assert_eq!(code, 2);
    let (code, _) = on_a_terminal(&cli, empty.path(), &["-c", "extra"]);
    assert_eq!(code, 2);
}

#[test]
fn a_pipe_keeps_the_usage_text_and_exit_2() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let out = Command::new(&cli)
        .env("PATH", empty.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Usage: theseus"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stdout.is_empty(), "nothing on stdout");
}

#[test]
fn a_terminal_on_stdin_alone_is_not_enough() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let (master, slave) = pty();
    let out = Command::new(&cli)
        .env("PATH", empty.path())
        .stdin(Stdio::from(slave))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    drop(master);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout is a pipe, so it is the usage text"
    );
}

#[test]
fn help_on_a_terminal_is_still_help() {
    let (bin, empty) = (scratch(), scratch());
    let cli = cli_in(bin.path());
    stand_in(bin.path());
    let (code, out) = on_a_terminal(&cli, empty.path(), &["--help"]);
    assert_eq!(code, 0);
    assert!(out.contains("Usage: theseus"), "{out}");
    assert!(
        out.contains("chat"),
        "the chat is listed among the commands:\n{out}"
    );
}
