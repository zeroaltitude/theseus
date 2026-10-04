//! MCP servers with the real `theseusd` and the real fake server
//! (`theseus-sim fake-mcp`, beside the daemon's binary), over stdio (M7
//! 36b): a server starts after serving, never before (its `mcp.started` row
//! follows `server.serving`); its tools are listed; a start offers the
//! stored list at once, with its server not up; and after the daemon's
//! `kill -9`, no server is left running, since each ends when its stdin
//! closes.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::{Daemon, Served};
use serde_json::{json, Value};

/// The fake's binary, beside the daemon's: the workspace's build of it.
fn sim_bin() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus-sim");
    assert!(
        p.is_file(),
        "no theseus-sim at {}: build the workspace (`cargo nextest run --workspace` builds it)",
        p.display()
    );
    p
}

fn mcp_server(t: &mut toml::Table, command: Vec<String>) {
    let mut fake = toml::Table::new();
    fake.insert("command".into(), command.into());
    fake.insert("start_timeout_secs".into(), 5.into());
    let mut servers = toml::Table::new();
    servers.insert("fake".into(), fake.into());
    let mut mcp = toml::Table::new();
    mcp.insert("servers".into(), servers.into());
    t.insert("mcp".into(), mcp.into());
}

fn fake_mcp(t: &mut toml::Table) {
    mcp_server(t, vec![sim_bin().display().to_string(), "fake-mcp".into()]);
}

/// The process's state letter, `None` once it is gone.
fn state(pid: u32) -> Option<char> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..].chars().next()
}

fn ended(pid: u32) -> bool {
    matches!(state(pid), None | Some('Z') | Some('X'))
}

fn kill(pid: u32, sig: i32) {
    // SAFETY: plain integers; each pid is this test's daemon or a server it
    // saw that daemon start.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// Kills, at the end of a test, every server it saw, whatever happened.
struct Reap(Vec<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for &pid in &self.0 {
            if !ended(pid) {
                kill(pid, libc::SIGKILL);
            }
        }
    }
}

fn until(s: &Served, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(h) = s.call("health", Value::Null) {
            if ok(&h) {
                return h;
            }
        }
        assert!(Instant::now() < deadline, "no {what} in 20 s:\n{}", s.log());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn row_position(s: &Served, kind: &str) -> Option<u64> {
    let t = s.call("ledger.tail", json!({"n": 1000})).unwrap();
    t["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == kind)
        .and_then(|r| r["position"].as_u64())
}

/// It starts after serving, lists its tools, and dies with the daemon's
/// `kill -9`: its stdin closes, and it ends.
#[test]
fn a_server_starts_after_serving_and_none_outlives_the_daemons_kill_9() {
    sim_bin();
    let s = Served::start(|_| {}, fake_mcp);
    let h = until(&s, "the server ready", |h| h["mcp"][0]["state"] == "ready");
    let pid = h["mcp"][0]["pid"].as_u64().unwrap() as u32;
    let _seen = Reap(vec![pid]);
    assert!(!ended(pid));
    assert_eq!(h["mcp"][0]["tools"], 5, "{}", h["mcp"]);
    // No spawn before serving: its row follows `server.serving`.
    let deadline = Instant::now() + Duration::from_secs(5);
    let started = loop {
        if let Some(p) = row_position(&s, "mcp.started") {
            break p;
        }
        assert!(Instant::now() < deadline, "no mcp.started row");
        std::thread::sleep(Duration::from_millis(10));
    };
    let serving = row_position(&s, "server.serving").unwrap();
    assert!(
        started > serving,
        "started at {started}, serving at {serving}"
    );
    let list = s.call("mcp.list", Value::Null).unwrap();
    let names: Vec<&str> = list["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["wire_name"].as_str())
        .collect();
    assert!(names.contains(&"mcp__fake__echo"), "{names:?}");
    // The daemon's kill -9: nothing stops the server but its stdin closing.
    kill(s.daemon.id(), libc::SIGKILL);
    let t0 = Instant::now();
    while !ended(pid) {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "the MCP server outlived its daemon's kill -9"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A clean stop, then the same state started again with a server that never
/// answers its handshake.
fn restarted_with_a_silent_server(s: Served) -> Served {
    let _ = s.call("shutdown", Value::Null);
    let Served { dir, mut daemon } = s;
    let t0 = Instant::now();
    while daemon.try_wait().is_none() {
        assert!(t0.elapsed() < Duration::from_secs(10), "no stop");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The same state, and a server that never answers.
    let path = |p: &str| dir.path().join(p);
    let mut t: toml::Table = std::fs::read_to_string(path("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    mcp_server(&mut t, vec!["sleep".into(), "30".into()]);
    std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    let log = std::fs::File::create(path("theseusd2.log")).unwrap();
    let _again = Daemon::spawn(
        Command::new(env!("CARGO_BIN_EXE_theseusd"))
            .arg("--config")
            .arg(path("config.toml"))
            .arg("--state-dir")
            .arg(path("state"))
            .arg("--socket")
            .arg(path("sock"))
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    path("bin").display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(log),
    );
    Served {
        dir,
        daemon: _again,
    }
}

/// A clean stop, then a start whose server never answers its handshake:
/// the tools the server listed last time are offered at once (health's
/// `stored`, and `mcp.list`), and the server is `starting`.
#[test]
fn a_start_offers_the_stored_list_before_its_server_is_up() {
    sim_bin();
    let s = Served::start(|_| {}, fake_mcp);
    let h = until(&s, "the server ready", |h| h["mcp"][0]["state"] == "ready");
    let pid = h["mcp"][0]["pid"].as_u64().unwrap() as u32;
    let _seen = Reap(vec![pid]);
    // The stored list is written off the runtime's workers, a moment after.
    std::thread::sleep(Duration::from_millis(300));
    let s = restarted_with_a_silent_server(s);
    let h = until(&s, "health", |h| h["mcp"][0]["name"] == "fake");
    let m = &h["mcp"][0];
    assert_ne!(m["state"], "ready", "{m}");
    assert_eq!(
        (m["stored"].as_bool(), m["tools"].as_u64()),
        (Some(true), Some(5)),
        "{m}"
    );
    let list = s.call("mcp.list", Value::Null).unwrap();
    assert_eq!(list["tools"].as_array().unwrap().len(), 5, "{list}");
    let _ = s.call("shutdown", Value::Null);
}

/// The prompts (36c): `mcp.prompt.list` gives the fake's three, with their
/// arguments; and a start whose server never answers lists the stored ones
/// at once, before it is up, as `mcp.list` does too.
#[test]
fn a_start_lists_the_stored_prompts_before_its_server_is_up() {
    sim_bin();
    let s = Served::start(|_| {}, fake_mcp);
    let h = until(&s, "the server ready", |h| h["mcp"][0]["state"] == "ready");
    let pid = h["mcp"][0]["pid"].as_u64().unwrap() as u32;
    let _seen = Reap(vec![pid]);
    let live = s.call("mcp.prompt.list", Value::Null).unwrap();
    let names: Vec<&str> = live["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(names, ["fake/greet", "fake/brief", "fake/review"], "{live}");
    assert_eq!(live["prompts"][0]["arguments"][0]["name"], "name");
    assert_eq!(live["prompts"][0]["arguments"][0]["required"], true);
    assert_eq!(live["prompts"][0]["stored"], false);
    // A prompt that cannot run is an error before any turn.
    let e = s
        .call(
            "turn.submit",
            json!({"input": "", "prompt": {"server": "fake", "name": "greet", "arguments": {}}}),
        )
        .unwrap_err();
    assert!(e.contains("needs its argument: name"), "{e}");
    // The stored list is written off the runtime's workers, a moment after.
    std::thread::sleep(Duration::from_millis(300));
    let s = restarted_with_a_silent_server(s);
    let h = until(&s, "health", |h| h["mcp"][0]["name"] == "fake");
    assert_ne!(h["mcp"][0]["state"], "ready", "{}", h["mcp"]);
    assert_eq!(h["mcp"][0]["prompts"], 3, "{}", h["mcp"]);
    let stored = s.call("mcp.prompt.list", Value::Null).unwrap();
    assert_eq!(stored["prompts"].as_array().unwrap().len(), 3, "{stored}");
    assert!(stored["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["stored"] == true));
    let list = s.call("mcp.list", Value::Null).unwrap();
    assert_eq!(list["prompts"].as_array().unwrap().len(), 3, "{list}");
    let _ = s.call("shutdown", Value::Null);
}
