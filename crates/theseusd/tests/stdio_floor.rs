//! The tool floor names the store and spool a `--stdio` daemon actually
//! serves (theseus-0h9f3 L21, `(P1)`). The store is single-process, so a
//! spawned stdio server opens its own directory rather than fighting a
//! running daemon for one: `<state>/store-stdio`, with its completion spool
//! beside it as `<state>/spool-stdio`. The floor held the bare literals
//! `<state>/store` and `<state>/spool` -- the SOCKET daemon's pair -- so
//! under `--stdio` it named a pair that was not in use and missed the pair
//! that was, and a tool call naming the owner's own `--stdio` session
//! history was never held for his approval.
//!
//! This test measures the pair in use **from the daemon**, not from a
//! reading of its source: it runs a real `theseusd --stdio`, sees which
//! directories that process made, and asserts the floor covers exactly
//! those. `--stdio` is reachable in ordinary use (`theseus --spawn` spawns
//! it), and the stdio daemon builds the same `Core` over the same floor.

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
    .env_remove("THESEUS_SOCKET");
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

/// Run one `theseusd --stdio` over a fresh state dir, ask it for `health`,
/// and let it end when its client goes away. Returns the state dir it
/// served.
fn served_once(dir: &Path) -> PathBuf {
    let state = dir.join("state");
    let cfg = config(dir);
    let log = std::fs::File::create(dir.join("theseusd.log")).unwrap();
    let (stdin, mut ask) = std::io::pipe().unwrap();
    let mut daemon = Daemon::spawn(
        command(dir)
            .arg("--config")
            .arg(&cfg)
            .arg("--state-dir")
            .arg(&state)
            .arg("--stdio")
            .stdin(stdin)
            .stdout(Stdio::null())
            .stderr(log),
    );
    ask.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"health\",\"params\":null}\n")
        .unwrap();
    drop(ask);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if daemon.try_wait().is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "theseusd --stdio did not end\n{}",
            std::fs::read_to_string(dir.join("theseusd.log")).unwrap_or_default()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let log = std::fs::read_to_string(dir.join("theseusd.log")).unwrap_or_default();
    assert!(log.contains("serving protocol on stdio"), "{log}");
    // Canonical, because the floor canonicalizes every path it holds and
    // these are compared by value.
    state.canonicalize().unwrap()
}

/// Every path no L1 view shows, for a state dir: the floor the runtime
/// builds, through the one function `theseusd check` and the tool runtime
/// both read (`toolrun::sandbox_for`).
///
/// The state dir goes on the **config**, because `sandbox_for` takes no
/// directory any more (theseus-0h9f3, the owner's word of 2026-11-11 11:34).
/// It was a second argument here, with `[server].state_dir` left at its
/// default: this test measured a floor over `state` that no daemon on this
/// config would ever have built, and it was only right because the floor was
/// a pure function of the argument. Now it is the floor a daemon serving out
/// of `state` really gets.
fn floor_of(state: &Path) -> Vec<PathBuf> {
    let mut cfg = theseus_core::Config::example();
    cfg.server.state_dir = state.to_string_lossy().into_owned();
    theseus_core::toolrun::sandbox_for(&cfg).job_view().hidden
}

/// The pair a `--stdio` daemon makes is the pair the floor floors. The
/// directories are read back off the disk the daemon wrote, so this cannot
/// drift from the daemon's own resolution the way the floor's literals did.
#[test]
fn the_store_and_spool_a_stdio_daemon_makes_are_on_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let state = served_once(dir.path());

    // What that process actually made, by name.
    let mut made: Vec<String> = std::fs::read_dir(&state)
        .unwrap()
        .flatten()
        .filter(|e| e.metadata().map(|m| m.is_dir()).unwrap_or(false))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("store") || n.starts_with("spool"))
        .collect();
    made.sort();
    assert_eq!(
        made,
        ["spool-stdio", "store-stdio"],
        "a --stdio daemon serves its own pair, and only it: {made:?}"
    );

    // ... and each one is on the floor. Before this row the floor held
    // `<state>/store` and `<state>/spool`, so both of these assertions
    // failed and the pair in use was read at the tool's own posture.
    let floor = floor_of(&state);
    for name in &made {
        let p = state.join(name);
        assert!(
            floor.iter().any(|f| f == &p),
            "the --stdio daemon serves {p:?}, which is not on the floor: {floor:?}"
        );
    }

    // The socket daemon's pair is floored too, in this mode as in the other:
    // it is Theseus's own state whether a daemon is serving out of it or
    // not, and the next socket start will serve it.
    for name in ["store", "spool"] {
        let p = state.join(name);
        assert!(
            floor.iter().any(|f| f == &p),
            "{p:?} is not on the floor: {floor:?}"
        );
    }
}
