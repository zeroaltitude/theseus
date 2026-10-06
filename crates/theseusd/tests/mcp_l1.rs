//! An MCP server in L1 (M7 43a), with the real `theseusd`, its real
//! `mcp-sandbox` role and L1 init, and the real fake server (`theseus-sim
//! fake-mcp`, bound into the view read-only): it starts after serving and
//! answers its handshake and its list; it runs in a network namespace of its
//! own with no route out, no network but loopback; and its whole tree ends
//! with the daemon's clean stop and with the daemon's `kill -9`.
//!
//! As root, L1 refuses every start (theseus-pv6i): the server fails with that
//! reason, and nothing of it runs at L0. Run the rest as an ordinary user
//! (on a root-only machine, as uid 65534: see the cloud report for 43a).

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Served;
use serde_json::Value;

fn sim_bin() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus-sim");
    assert!(
        p.is_file(),
        "no theseus-sim at {}: build the workspace",
        p.display()
    );
    p
}

fn root() -> bool {
    // SAFETY: no arguments.
    unsafe { libc::geteuid() == 0 }
}

/// `[mcp.servers.fake]` in L1, and the fake's directory bound read-only, as
/// a helper binary must be when the view would not show it.
fn l1_fake(t: &mut toml::Table) {
    l1_fake_with(t, &[]);
}

/// `l1_fake`, the fake given `args`.
fn l1_fake_with(t: &mut toml::Table, args: &[&str]) {
    let sim = sim_bin();
    let mut fake = toml::Table::new();
    let mut command = vec![sim.display().to_string(), "fake-mcp".into()];
    command.extend(args.iter().map(|a| a.to_string()));
    fake.insert("command".into(), command.into());
    fake.insert("sandbox".into(), "l1".into());
    fake.insert("start_timeout_secs".into(), 10.into());
    let mut servers = toml::Table::new();
    servers.insert("fake".into(), fake.into());
    let mut mcp = toml::Table::new();
    mcp.insert("servers".into(), servers.into());
    t.insert("mcp".into(), mcp.into());
    let sandbox = t
        .entry("sandbox")
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .unwrap();
    sandbox.insert(
        "ro_paths".into(),
        vec![sim.parent().unwrap().display().to_string()].into(),
    );
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

/// `pid` and every process below it, parents first.
fn tree(pid: u32) -> Vec<u32> {
    let mut out = vec![pid];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        let kids =
            std::fs::read_to_string(format!("/proc/{p}/task/{p}/children")).unwrap_or_default();
        out.extend(
            kids.split_whitespace()
                .filter_map(|k| k.parse::<u32>().ok()),
        );
        i += 1;
    }
    out
}

fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(s) => s
            .rfind(')')
            .and_then(|i| s[i + 2..].chars().next())
            .is_some_and(|c| c == 'Z' || c == 'X'),
    }
}

fn kill(pid: u32, sig: i32) {
    // SAFETY: plain integers; each pid is this test's daemon or a process it
    // saw that daemon start.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

/// Kills, at the end of a test, every process it saw, whatever happened.
struct Reap(Vec<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        for &pid in self.0.iter().rev() {
            if !gone(pid) {
                kill(pid, libc::SIGKILL);
            }
        }
    }
}

fn wait_gone(pids: &[u32], what: &str) {
    let t0 = Instant::now();
    while let Some(p) = pids.iter().find(|p| !gone(**p)) {
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "pid {p} outlived {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The server: the role, the init, and the fake, the fake last.
fn started(s: &Served) -> Vec<u32> {
    let h = until(s, "the server ready", |h| h["mcp"][0]["state"] == "ready");
    assert_eq!(h["mcp"][0]["tools"], 5, "{}", h["mcp"]);
    let role = h["mcp"][0]["pid"].as_u64().unwrap() as u32;
    let t0 = Instant::now();
    loop {
        let t = tree(role);
        if t.len() >= 3 {
            return t;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "no server below its role: {t:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn cmdline(pid: u32) -> String {
    std::fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| String::from_utf8_lossy(&b).replace('\0', " "))
        .unwrap_or_default()
}

/// It starts, answers, sees no network, and ends with the daemon's stop.
#[test]
fn a_server_in_l1_starts_answers_sees_no_network_and_ends_with_the_daemons_stop() {
    let s = Served::start(|_| {}, l1_fake);
    if root() {
        // L1 refuses a root daemon's every start, and nothing runs at L0.
        let h = until(&s, "the refusal", |h| {
            h["mcp"][0]["last_error"]
                .as_str()
                .is_some_and(|e| e.contains("RLIMIT_NPROC"))
        });
        assert_ne!(h["mcp"][0]["state"], "ready", "{}", h["mcp"]);
        assert!(h["mcp"][0]["pid"].is_null(), "{}", h["mcp"]);
        return;
    }
    let pids = started(&s);
    let _seen = Reap(pids.clone());
    let (role, server) = (pids[0], *pids.last().unwrap());
    assert!(cmdline(role).contains("mcp-sandbox"), "{}", cmdline(role));
    assert!(
        cmdline(pids[1]).contains("job-sandbox"),
        "{}",
        cmdline(pids[1])
    );
    assert!(cmdline(server).contains("fake-mcp"), "{}", cmdline(server));
    // Its own network namespace, with loopback alone and no route.
    let ns = |p: &str| std::fs::read_link(p).unwrap();
    assert_ne!(
        ns(&format!("/proc/{server}/ns/net")),
        ns("/proc/self/ns/net"),
        "a network namespace of its own"
    );
    let dev = std::fs::read_to_string(format!("/proc/{server}/net/dev")).unwrap();
    let ifaces: Vec<&str> = dev
        .lines()
        .skip(2)
        .filter_map(|l| l.split(':').next().map(str::trim))
        .collect();
    assert_eq!(ifaces, ["lo"], "{dev}");
    let routes = std::fs::read_to_string(format!("/proc/{server}/net/route")).unwrap();
    assert_eq!(routes.lines().count(), 1, "no route: {routes}");
    // Its tools, listed through the board.
    let list = s.call("mcp.list", Value::Null).unwrap();
    assert_eq!(list["tools"].as_array().unwrap().len(), 5, "{list}");
    // The daemon's clean stop ends the role, the init, and the server.
    let _ = s.call("shutdown", Value::Null);
    wait_gone(&pids, "the daemon's stop");
}

/// The daemon's `kill -9`: the role sees its daemon go and ends the
/// server's whole namespace.
#[test]
fn a_server_in_l1_ends_with_the_daemons_kill_9() {
    if root() {
        return;
    }
    let s = Served::start(|_| {}, l1_fake);
    let pids = started(&s);
    let _seen = Reap(pids.clone());
    kill(s.daemon.id(), libc::SIGKILL);
    wait_gone(&pids, "the daemon's kill -9");
}

/// The daemon's `kill -9` with a server that never reads its input's end
/// (theseus-grxh): the fake above ends of itself when the daemon's end of its
/// stdin closes, so the test before this one passes with no watch at all.
/// This one (`fake-mcp --outlive-stdin`) runs until it is killed, so only the
/// role's watch of the daemon's pidfd ends it: the role, the init, and the
/// server all go.
#[test]
fn a_server_that_outlives_its_input_ends_with_the_daemons_kill_9() {
    if root() {
        return;
    }
    let s = Served::start(|_| {}, |t| l1_fake_with(t, &["--outlive-stdin"]));
    let pids = started(&s);
    let _seen = Reap(pids.clone());
    let server = *pids.last().unwrap();
    assert!(
        cmdline(server).contains("--outlive-stdin"),
        "{}",
        cmdline(server)
    );
    kill(s.daemon.id(), libc::SIGKILL);
    wait_gone(&pids, "the daemon's kill -9");
}
