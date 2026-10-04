//! A closed pipe (theseus-gi7). `theseusd config | head` panicked when head
//! went away, and a release build, whose panics abort, exited 134. The
//! print-only subcommands now end quietly, with success, when their reader
//! is gone. The daemon itself still ignores SIGPIPE, so a client that
//! disconnects before its answer never kills it.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::Daemon;

fn theseusd() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_theseusd"))
}

/// `theseusd` with a stand-in `op` first on its PATH (the loader wants one,
/// and it answers nothing) and no inherited Theseus settings.
fn command(dir: &Path) -> Command {
    let bin = dir.join("bin");
    if !bin.join("op").exists() {
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("op"), "#!/bin/sh\nexit 1\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let mut c = Command::new(theseusd());
    c.env(
        "PATH",
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    )
    .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
    .env_remove("THESEUS_OP_TOKEN_FILE")
    .env_remove("THESEUS_CONFIG")
    .env_remove("THESEUS_STATE_DIR")
    .env_remove("THESEUS_SOCKET")
    .env_remove("THESEUS_RESTARTED_ONTO_VAULT")
    .env_remove("THESEUS_CONFIG_VAULT_FIRST")
    .stdin(Stdio::null());
    c
}

/// A config file that is safe to serve here.
fn config(dir: &Path) -> PathBuf {
    let projects = dir.join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, common::safe_note(&theseusd(), &projects, 100.0)).unwrap();
    path
}

/// A pipe whose reader is already gone: every write to it fails.
fn closed_pipe() -> Stdio {
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    writer.into()
}

#[test]
fn the_print_only_subcommands_end_quietly_when_their_reader_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let cfg = cfg.to_str().unwrap();
    let state = dir.path().join("state");
    let state = state.to_str().unwrap();
    for args in [
        vec!["example-config"],
        vec!["example-bindings"],
        vec!["--config", cfg, "--state-dir", state, "config"],
    ] {
        let out = command(dir.path())
            .args(&args)
            .stdout(closed_pipe())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "theseusd {}: {}\n{err}",
            args.join(" "),
            out.status
        );
        assert!(!err.contains("panicked"), "{err}");
        // With a reader, the same subcommand prints all of it.
        let out = command(dir.path()).args(&args).output().unwrap();
        assert!(out.status.success() && !out.stdout.is_empty());
    }
}

/// A client of `theseusd --stdio` that is gone before its answer: the
/// answer's write meets a closed pipe, and the daemon, which ignores
/// SIGPIPE, ends the connection and exits cleanly instead of dying of the
/// signal. (Its socket writes never raise the signal, so a pipe is where a
/// default SIGPIPE would kill it: this stdout, or a child's stdin.)
#[test]
fn a_client_that_goes_away_before_its_answer_never_kills_the_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = config(dir.path());
    let log = std::fs::File::create(dir.path().join("theseusd.log")).unwrap();
    let (stdin, mut ask) = std::io::pipe().unwrap();
    let mut daemon = Daemon::spawn(
        command(dir.path())
            .arg("--config")
            .arg(&cfg)
            .arg("--state-dir")
            .arg(dir.path().join("state"))
            .arg("--stdio")
            .stdin(stdin)
            .stdout(closed_pipe())
            .stderr(log),
    );
    ask.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"health\",\"params\":null}\n")
        .unwrap();
    drop(ask);
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(s) = daemon.try_wait() {
            break s;
        }
        assert!(Instant::now() < deadline, "theseusd --stdio did not end");
        std::thread::sleep(Duration::from_millis(5));
    };
    use std::os::unix::process::ExitStatusExt;
    let log = std::fs::read_to_string(dir.path().join("theseusd.log")).unwrap();
    assert_eq!(status.signal(), None, "killed by a signal: {status}\n{log}");
    assert!(status.success(), "{status}\n{log}");
    assert!(log.contains("serving protocol on stdio"), "{log}");
}
