//! One scratch daemon per arm (row 55): its config, written from a base
//! config with `[memory]` set; its start on a copy of the exam's store; the
//! wait until it serves and its index tender holds the whole store; and its
//! stop, which leaves nothing running.
//!
//! - **The config.** The base config is the maintainer's scratch config (its
//!   profiles, providers and secrets). Each daemon's copy sets `[memory] mode
//!   = "live"` and `arm`, and turns off what would collide or reach outside:
//!   Discord, the web UI, and Theseus's own MCP server. The `none` daemon
//!   runs no index tender (it asks the index nothing); the others run one.
//! - **The start.** `theseusd --config … --state-dir … --socket …`, with
//!   `THESEUS_CONFIG`, `THESEUS_STATE_DIR`, `THESEUS_SOCKET` and
//!   `THESEUS_SESSION` taken out of its environment, its stderr in
//!   `<dir>/theseusd.log`.
//! - **The stop.** `shutdown` over the socket, then a kill after
//!   [`STOP_WAIT`]; then any process left whose command line names the
//!   daemon's directory (its tender, which a clean stop sends SIGTERM) gets
//!   [`STOP_WAIT`] to go, and a kill. A dropped `Daemon` is stopped the same
//!   way, so an error or a panic leaves none behind.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::client::Client;

/// How long a stop waits for the daemon, and then for what it left.
pub const STOP_WAIT: Duration = Duration::from_secs(20);
/// How long a start waits for the socket to answer.
pub const SERVE_WAIT: Duration = Duration::from_secs(60);

fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
    let v = t
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !v.is_table() {
        *v = toml::Value::Table(toml::Table::new());
    }
    v.as_table_mut().expect("made a table")
}

/// The config of the daemon that runs `arm` (`none`, `bm25`, `baseline`),
/// from `base`.
pub fn config_for(base: &toml::Table, arm: &str) -> toml::Table {
    let mut t = base.clone();
    let m = table(&mut t, "memory");
    m.insert("mode".into(), "live".into());
    m.insert("arm".into(), arm.into());
    table(&mut t, "discord").insert("enabled".into(), false.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    if t.contains_key("mcp_server") {
        table(&mut t, "mcp_server").insert("enabled".into(), false.into());
    }
    table(&mut t, "index").insert("enabled".into(), (arm != "none").into());
    t
}

/// The config of the replay's daemon, from `base`: an index tender, and
/// memory off, so it writes no recall of its own.
pub fn replay_config(base: &toml::Table) -> toml::Table {
    let mut t = config_for(base, "none");
    let m = table(&mut t, "memory");
    m.insert("mode".into(), "off".into());
    m.remove("arm");
    table(&mut t, "index").insert("enabled".into(), true.into());
    t
}

/// A running scratch daemon, stopped when dropped.
pub struct Daemon {
    child: Option<Child>,
    pub arm: String,
    pub dir: PathBuf,
    pub state: PathBuf,
    pub socket: PathBuf,
    pub log: PathBuf,
    pub index: bool,
}

/// `dir`'s scratch daemon: its config at `<dir>/config.toml`, its state at
/// `<dir>/state` (whose `store` the caller put there), its socket at
/// `<dir>/sock`.
pub fn start(
    theseusd: &Path,
    dir: &Path,
    arm: &str,
    config: &toml::Table,
    env: &[(OsString, Option<OsString>)],
) -> Result<Daemon> {
    let cfg = dir.join("config.toml");
    std::fs::write(&cfg, toml::to_string(config)?)
        .with_context(|| format!("writing {}", cfg.display()))?;
    let state = dir.join("state");
    let socket = dir.join("sock");
    let log = dir.join("theseusd.log");
    let logf =
        std::fs::File::create(&log).with_context(|| format!("creating {}", log.display()))?;
    let mut cmd = Command::new(theseusd);
    cmd.arg("--config")
        .arg(&cfg)
        .arg("--state-dir")
        .arg(&state)
        .arg("--socket")
        .arg(&socket)
        .env_remove("THESEUS_CONFIG")
        .env_remove("THESEUS_STATE_DIR")
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(logf);
    for (k, v) in env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    let child = cmd
        .spawn()
        .with_context(|| format!("starting {}", theseusd.display()))?;
    let index = config
        .get("index")
        .and_then(|i| i.get("enabled"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(true);
    Ok(Daemon {
        child: Some(child),
        arm: arm.into(),
        dir: dir.to_path_buf(),
        state,
        socket,
        log,
        index,
    })
}

impl Daemon {
    /// The pid, while it runs.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    fn tail(&self) -> String {
        let t = std::fs::read_to_string(&self.log).unwrap_or_default();
        let lines: Vec<&str> = t.lines().rev().take(20).collect();
        lines.into_iter().rev().collect::<Vec<_>>().join("\n")
    }

    fn exited(&mut self) -> Option<String> {
        let c = self.child.as_mut()?;
        match c.try_wait() {
            Ok(Some(s)) => Some(format!("{s}")),
            _ => None,
        }
    }

    /// Wait until the socket answers `health`.
    pub fn wait_serving(&mut self) -> Result<()> {
        let t0 = Instant::now();
        loop {
            if let Ok(mut c) = Client::connect(&self.socket) {
                if c.call("health", Value::Null, Duration::from_secs(10))
                    .is_ok()
                {
                    return Ok(());
                }
            }
            if let Some(s) = self.exited() {
                bail!(
                    "the {} daemon exited ({s}) before serving; its log ends:\n{}",
                    self.arm,
                    self.tail()
                );
            }
            if t0.elapsed() > SERVE_WAIT {
                bail!(
                    "the {} daemon did not serve in {} s; its log ends:\n{}",
                    self.arm,
                    SERVE_WAIT.as_secs(),
                    self.tail()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Wait until its tender has read the whole store, through
    /// `last_position` (and embedded it, when it has the model: `hybrid`
    /// mode); its status then. A daemon without an index has nothing to wait
    /// for.
    pub fn wait_indexed(
        &mut self,
        last_position: u64,
        settle: Duration,
        log: &mut dyn FnMut(&str),
    ) -> Result<Option<Value>> {
        if !self.index {
            return Ok(None);
        }
        let sock = self.state.join("index").join("sock");
        let t0 = Instant::now();
        let mut told = Instant::now();
        let mut last = Value::Null;
        loop {
            let status = Client::connect(&sock)
                .and_then(|mut c| c.call("index.status", json!({}), Duration::from_secs(30)));
            if let Ok(s) = status {
                let vectors = s["mode"].as_str() == Some("hybrid");
                if settled(&s, last_position, vectors) {
                    return Ok(Some(s));
                }
                last = s;
            }
            if let Some(s) = self.exited() {
                bail!(
                    "the {} daemon exited ({s}) while its tender read the store; its log ends:\n{}",
                    self.arm,
                    self.tail()
                );
            }
            if t0.elapsed() > settle {
                bail!(
                    "the {} daemon's tender did not hold the store in {} s: {last}",
                    self.arm,
                    settle.as_secs()
                );
            }
            if told.elapsed() >= Duration::from_secs(10) {
                log(&format!(
                    "{}: waiting for the tender: state {}, position {} of {last_position}, {} of {} chunks embedded",
                    self.arm,
                    last["state"],
                    last["position"],
                    last["vectors"]["vectors"],
                    last["vectors"]["chunks"]
                ));
                told = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Stop it: `shutdown`, a kill after [`STOP_WAIT`], and then whatever
    /// it left that names its directory. Says what it had to kill.
    pub fn stop(&mut self) -> Result<Vec<String>> {
        let Some(mut child) = self.child.take() else {
            return Ok(Vec::new());
        };
        let mut killed = Vec::new();
        if let Ok(mut c) = Client::connect(&self.socket) {
            let _ = c.call("shutdown", Value::Null, Duration::from_secs(10));
        }
        let t0 = Instant::now();
        loop {
            if child.try_wait()?.is_some() {
                break;
            }
            if t0.elapsed() > STOP_WAIT {
                let _ = child.kill();
                let _ = child.wait();
                killed.push(format!("theseusd {}", child.id()));
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let t0 = Instant::now();
        loop {
            let left = processes_naming(&self.dir);
            if left.is_empty() {
                break;
            }
            if t0.elapsed() > STOP_WAIT {
                for (pid, what) in left {
                    let _ = Command::new("kill")
                        .args(["-KILL", &pid.to_string()])
                        .status();
                    killed.push(format!("{what} {pid}"));
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(killed)
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// The status when the tender has read the whole store (and embedded it, if
/// `vectors`).
pub fn settled(s: &Value, last_position: u64, vectors: bool) -> bool {
    let ready = s["state"].as_str() == Some("ready") && s["lag"]["bytes"].as_u64() == Some(0);
    let through = s["position"].as_u64().is_some_and(|p| p >= last_position);
    let embedded = !vectors || {
        let v = &s["vectors"];
        v["pending"].as_u64() == Some(0) && v["vectors"].as_u64() == v["chunks"].as_u64()
    };
    ready && through && embedded
}

/// The processes whose command line names `dir`, with their program's name:
/// a daemon's tender, or a daemon itself.
pub fn processes_naming(dir: &Path) -> Vec<(u32, String)> {
    let needle = dir.as_os_str().as_encoded_bytes().to_vec();
    let me = std::process::id();
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in rd.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        if pid == me {
            continue;
        }
        let Ok(cmd) = std::fs::read(e.path().join("cmdline")) else {
            continue;
        };
        if cmd.windows(needle.len()).any(|w| w == needle.as_slice()) {
            let prog = cmd.split(|b| *b == 0).next().unwrap_or_default();
            let prog = Path::new(std::str::from_utf8(prog).unwrap_or("?"))
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            out.push((pid, prog));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_arms_config_sets_memory_and_turns_off_what_would_collide() {
        let base: toml::Table = r#"
[model]
live = "glm"
[memory]
recall_deadline_ms = 900
[web]
port = 7433
[mcp_server]
enabled = true
port = 7434
"#
        .parse()
        .unwrap();
        for arm in ["none", "bm25", "baseline", "+activation"] {
            let t = config_for(&base, arm);
            assert_eq!(t["memory"]["mode"].as_str(), Some("live"));
            assert_eq!(t["memory"]["arm"].as_str(), Some(arm));
            assert_eq!(t["memory"]["recall_deadline_ms"].as_integer(), Some(900));
            assert_eq!(t["model"]["live"].as_str(), Some("glm"));
            assert_eq!(t["web"]["enabled"].as_bool(), Some(false));
            assert_eq!(t["discord"]["enabled"].as_bool(), Some(false));
            assert_eq!(t["mcp_server"]["enabled"].as_bool(), Some(false));
            assert_eq!(t["index"]["enabled"].as_bool(), Some(arm != "none"));
        }
        assert!(!config_for(&toml::Table::new(), "bm25").contains_key("mcp_server"));
    }

    #[test]
    fn a_tender_is_settled_once_it_holds_the_store() {
        let s = |state: &str, pos: u64, lag: u64, chunks: u64, vectors: u64, pending: u64| {
            json!({"state": state, "position": pos, "lag": {"bytes": lag},
                   "vectors": {"chunks": chunks, "vectors": vectors, "pending": pending}})
        };
        assert!(settled(&s("ready", 90, 0, 10, 0, 10), 90, false));
        assert!(!settled(&s("ready", 89, 0, 10, 10, 0), 90, false));
        assert!(!settled(&s("ready", 90, 5, 10, 10, 0), 90, false));
        assert!(!settled(&s("catching_up", 90, 0, 10, 10, 0), 90, false));
        assert!(!settled(&s("ready", 90, 0, 10, 4, 6), 90, true));
        assert!(settled(&s("ready", 95, 0, 10, 10, 0), 90, true));
    }
}
