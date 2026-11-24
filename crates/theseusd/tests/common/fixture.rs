//! What a test's `Rig` shares when it keeps its daemon's files in one temp
//! dir: paths in it, the daemon's log, a call on its socket, a wait with the
//! log in the failure, and the spawn that serves the rig's `config.toml`.
//! A `Rig` says where the dir is; a method it defines itself wins over these.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::{rpc, tail, Daemon};

pub trait Fixture {
    /// The temp dir the daemon's files are in.
    fn dir(&self) -> &Path;

    /// Where the daemon's socket is.
    fn sock(&self) -> PathBuf {
        self.path("sock")
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir().join(p)
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    /// The log's last 30 lines.
    fn log_tail(&self) -> String {
        tail(&self.log(), 30)
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        rpc::call(&self.sock(), method, params)
    }

    /// Wait for `f` to give something: 40 s on a quiet machine, and in proportion to what
    /// `busy_factor` measured on a busy one, up to 100 s (nextest ends a test at 120 s).
    /// The factor is measured once, at the first wait; a failure says it beside the one now.
    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        static FACTOR: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        static LIMIT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        let limit =
            *LIMIT.get_or_init(|| (40 * *FACTOR.get_or_init(theseus_sim::busy_factor)).min(100));
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(limit),
                "no {what} in {limit} s (busy factor {} at the first wait, {} now); the daemon's log ends:\n{}",
                FACTOR.get().copied().unwrap_or(0),
                theseus_sim::busy_factor(),
                tail(&self.log(), 30)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// `health` on the socket once it answers; a daemon that has exited is said at once, with its
    /// status and log, and not waited for as a slow one.
    fn socket_or_exit(&self, d: &mut Daemon) -> Option<Value> {
        if let Some(status) = d.try_wait() {
            panic!(
                "theseusd exited ({status}) before the socket; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
            );
        }
        self.call("health", Value::Null).ok()
    }

    /// The daemon on the rig's `config.toml`, its `bin` first on `PATH`, once
    /// its socket answers.
    fn spawn(&self) -> Daemon {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let mut d = Daemon::spawn(
            std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"))
                .arg("--config")
                .arg(self.path("config.toml"))
                .arg("--state-dir")
                .arg(self.path("state"))
                .arg("--socket")
                .arg(self.path("sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        self.path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        self.wait("the socket", || self.socket_or_exit(&mut d));
        d
    }

    /// `spawn`, with a debug build's plant that opens the DM place's session
    /// at the bind, as before theseus-emqx, for tests that submit to it by hand.
    fn spawn_open_at_bind(&self) -> Daemon {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
        let mut d = Daemon::spawn(
            std::process::Command::new(env!("CARGO_BIN_EXE_theseusd"))
                .arg("--config")
                .arg(self.path("config.toml"))
                .arg("--state-dir")
                .arg(self.path("state"))
                .arg("--socket")
                .arg(self.path("sock"))
                .env(
                    "PATH",
                    format!(
                        "{}:{}",
                        self.path("bin").display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                )
                .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
                .env("THESEUS_TEST_OPEN_AT_BIND", "1")
                .env_remove("THESEUS_OP_TOKEN_FILE")
                .env_remove("THESEUS_CONFIG")
                .env_remove("THESEUS_STATE_DIR")
                .env_remove("THESEUS_SOCKET")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        self.wait("the socket", || self.socket_or_exit(&mut d));
        d
    }
}
