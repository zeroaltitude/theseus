//! The language-server board (L2, theseus-n88g.8) with the real `theseusd`
//! and the real `theseus-lsp-fake`, driven by a stand-in for the Messages
//! API: no server at the daemon's start, one started by the model's first
//! `lsp_definition` (through the children registry, in its own process
//! group, with its log), health's `lsp` block, and no server left once the
//! daemon is killed with SIGKILL: its stdin closes, and it ends.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Served;
use serde_json::{json, Value};

/// The fake server's binary, beside the daemon's: the workspace's build.
fn fake_bin() -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_theseusd")).with_file_name("theseus-lsp-fake");
    assert!(
        p.is_file(),
        "no theseus-lsp-fake at {}: build the workspace (`cargo nextest run --workspace` builds it)",
        p.display()
    );
    p
}

/// The process's state letter, `None` once it is gone.
fn state(pid: u32) -> Option<char> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..].chars().next()
}

fn ended(pid: u32) -> bool {
    matches!(state(pid), None | Some('Z') | Some('X'))
}

/// Its process group, from `/proc`.
fn pgid(pid: u32) -> Option<u32> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    s[s.rfind(')')? + 2..]
        .split_whitespace()
        .nth(2)?
        .parse()
        .ok()
}

/// Kills, at the end of the test, the server it saw, whatever happened.
struct Reap(Option<u32>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(pid) = self.0.filter(|p| !ended(*p)) {
            // SAFETY: plain integers: the fake this test saw start.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
        }
    }
}

fn until(s: &Served, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let h = s.call("health", Value::Null).unwrap();
        if ok(&h) {
            return h;
        }
        assert!(
            Instant::now() < deadline,
            "no {what} in 20 s: {h}\n{}",
            s.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A daemon on the template, its model the stand-in at `base`, every secret
/// from a file, and `[lsp]` on with the fake as the server of `.fake` files.
fn served(fake: &Path, base: &str) -> (Served, tempfile::TempDir) {
    let keys = tempfile::tempdir().unwrap();
    let key = keys.path().join("key");
    std::fs::write(&key, "test-not-a-key").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let base = base.to_string();
    let s = Served::start(
        |dir| {
            let p = dir.join("projects");
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("fake.toml"), "").unwrap();
            std::fs::write(p.join("a.fake"), "def total\nlet x = total\n").unwrap();
        },
        |t| {
            fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
                t.entry(key)
                    .or_insert_with(|| toml::Value::Table(Default::default()))
                    .as_table_mut()
                    .unwrap()
            }
            table(t, "model").insert("api_base".into(), base.clone().into());
            for (_, p) in table(t, "providers").iter_mut() {
                p.as_table_mut()
                    .unwrap()
                    .insert("api_base".into(), base.clone().into());
            }
            // Every secret from a file, so the model's key resolves.
            for (_, v) in table(t, "secrets").iter_mut() {
                *v = format!("file:{}", key.display()).into();
            }
            let lsp: toml::Table = toml::from_str(&format!(
                "enabled = true\n[servers.fake]\ncommand = [{:?}]\nextensions = [\"fake\"]\nroots = [\"fake.toml\"]\n",
                fake.display().to_string()
            ))
            .unwrap();
            t.insert("lsp".into(), lsp.into());
        },
    );
    (s, keys)
}

#[test]
fn a_server_starts_at_the_first_call_and_none_outlives_the_daemons_kill() {
    let fake = fake_bin();
    let model = FakeModel::start(|prompt| {
        if prompt.contains("where is total defined") {
            vec![(
                "lsp_definition",
                json!({"path": "a.fake", "line": 2, "symbol": "total"}),
            )]
        } else {
            vec![]
        }
    });
    let (s, _keys) = served(&fake, &model.base);
    let h = until(&s, "the model's key", |h| {
        h["secrets"]["state"]
            .as_str()
            .is_some_and(|x| x != "resolving")
    });
    assert_eq!(
        h["lsp"],
        json!([]),
        "no server at the daemon's start: {}",
        h["lsp"]
    );
    let session = s.call("session.open", json!({"label": "lsp"})).unwrap();
    let r = s
        .call(
            "turn.submit",
            json!({"session_id": session["session_id"], "input": "where is total defined?",
                "author": "test", "attachments": []}),
        )
        .unwrap();
    assert_eq!(r["stop_reason"], "no_tool_calls", "{r}\n{}", s.log());
    // The model got the definition, with the lines around it.
    let results: Vec<String> = model
        .requests()
        .iter()
        .flat_map(|q| q["messages"].as_array().cloned().unwrap_or_default())
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter(|b| b["type"] == "tool_result")
        .map(|b| b["content"].to_string())
        .collect();
    assert!(
        results
            .iter()
            .any(|r| r.contains("a.fake:1:5") && r.contains("def total")),
        "{results:?}"
    );
    let h = until(&s, "the server ready", |h| h["lsp"][0]["state"] == "ready");
    let server = &h["lsp"][0];
    assert_eq!(server["server"], "fake", "{server}");
    let pid = server["pid"].as_u64().unwrap() as u32;
    let _reap = Reap(Some(pid));
    assert!(!ended(pid));
    assert_eq!(pgid(pid), Some(pid), "a process group of its own");
    let logs: Vec<_> = std::fs::read_dir(s.path("state/lsp"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(logs.len(), 1, "its stderr's log");
    for kind in ["lsp.started", "lsp.ready"] {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let rows = s
                .call("ledger.tail", json!({"kind": kind}))
                .unwrap()
                .to_string();
            if rows.contains(&format!("\"pid\":{pid}")) {
                break;
            }
            assert!(Instant::now() < deadline, "no {kind} row: {rows}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // SAFETY: plain integers: the daemon this test spawned.
    unsafe {
        libc::kill(s.daemon.id() as libc::pid_t, libc::SIGKILL);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ended(pid) {
        assert!(
            Instant::now() < deadline,
            "the server outlived its daemon's SIGKILL"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
