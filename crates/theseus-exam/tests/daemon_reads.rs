//! The fixture writer's store reads identically in an unmodified daemon
//! (the 34a gate's first test, design §3.2).
//!
//! The exam's past is written into a scratch store; the library reads every
//! session and node of it, as the core's own readers render them for the
//! protocol (`Core::node_info`, `SessionRecord::info`); then the real
//! `theseusd` of this workspace, built from an untouched core, serves the
//! store, and every session and node it serves must equal the library's
//! reading, field for field. It serves at once, its history check says `ok`
//! over every record, and its open repairs nothing. After its stop, every
//! keyed node is still where the manifest says.
//!
//! The daemon is the `theseusd` beside this test's binary (a workspace test
//! run builds it), or `THESEUS_EXAM_THESEUSD`.
//!
//! Both exams: exam-v1's 48 sessions, and exam-v2's hundreds, generated from
//! templates and spread over months, with a background no item owns.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_core::rpc::Core;
use theseus_core::session::SessionRecord;
use theseus_core::store::Store;
use theseus_exam::fixture;
use theseus_exam::item::Exam;

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

fn theseusd() -> PathBuf {
    if let Some(p) = std::env::var_os("THESEUS_EXAM_THESEUSD") {
        return PathBuf::from(p);
    }
    // <target>/debug/deps/<this test> → <target>/debug/theseusd
    let exe = std::env::current_exe().unwrap();
    let p = exe
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("theseusd");
    assert!(
        p.is_file(),
        "no theseusd at {}: build it first (`cargo build -p theseusd`), or set THESEUS_EXAM_THESEUSD",
        p.display()
    );
    p
}

/// A spawned daemon, killed and reaped when dropped.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The template, safe to serve in a test: no web UI or Discord, every secret
/// a reference to the vault `Test` (the fake `op` answers), the model
/// endpoints on a port nothing answers, and `projects` as the workspace.
fn safe_config(bin: &Path, projects: &Path) -> String {
    let out = Command::new(bin).arg("example-config").output().unwrap();
    let mut t: toml::Table = String::from_utf8(out.stdout).unwrap().parse().unwrap();
    fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
        t.entry(key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap()
    }
    table(&mut t, "model").insert("api_base".into(), "http://127.0.0.1:9".into());
    for (_, p) in table(&mut t, "providers").iter_mut() {
        p.as_table_mut()
            .unwrap()
            .insert("api_base".into(), "http://127.0.0.1:9".into());
    }
    let secrets = table(&mut t, "secrets");
    let names: Vec<String> = secrets
        .keys()
        .filter(|k| k.as_str() != "github_token")
        .cloned()
        .collect();
    secrets.clear();
    for n in names {
        secrets.insert(n.clone(), format!("op://Test/{n}/credential").into());
    }
    table(&mut t, "discord").insert("enabled".into(), false.into());
    table(&mut t, "web").insert("enabled".into(), false.into());
    table(&mut t, "tools").insert("projects_dir".into(), projects.display().to_string().into());
    toml::to_string(&t).unwrap()
}

fn call(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    let s = UnixStream::connect(sock).map_err(|e| json!(e.to_string()))?;
    s.set_read_timeout(Some(Duration::from_secs(60))).unwrap();
    let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
    (&s).write_all(format!("{req}\n").as_bytes())
        .map_err(|e| json!(e.to_string()))?;
    for line in BufReader::new(&s).lines() {
        let v: Value = serde_json::from_str(&line.map_err(|e| json!(e.to_string()))?)
            .map_err(|e| json!(e.to_string()))?;
        if v["id"] == 1 {
            return match v.get("error") {
                Some(e) if !e.is_null() => Err(e.clone()),
                _ => Ok(v["result"].clone()),
            };
        }
    }
    Err(json!("the connection closed"))
}

fn wait<T>(what: &str, log: &Path, mut f: impl FnMut() -> Option<T>) -> T {
    let t0 = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        let tail = std::fs::read_to_string(log).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(20).collect();
        assert!(
            t0.elapsed() < Duration::from_secs(40),
            "no {what} in 40 s; the daemon's log ends:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The session fields a reader of the store decides (the daemon adds live
/// ones, such as the execution's state, which the library has no view of).
fn stored_fields(s: &Value) -> Value {
    json!({
        "session_id": s["session_id"], "kind": s["kind"], "label": s["label"],
        "created_at_unix_ms": s["created_at_unix_ms"], "turns": s["turns"], "usage": s["usage"],
        "execution_id": s["execution_id"], "last_active_ms": s["last_active_ms"],
        "cost_usd": s["cost_usd"], "tool_calls": s["tool_calls"], "title": s["title"],
        "external_text": s["external_text"],
    })
}

#[test]
fn the_fixture_store_reads_identically_in_an_unmodified_daemon() {
    let exam = Exam::parse(theseus_exam::item::EXAM_V1).unwrap();
    assert_eq!(reads_identically(&exam), 48);
}

/// exam-v2's store: hundreds of sessions, months apart, and the background.
#[test]
fn the_v2_store_reads_identically_in_an_unmodified_daemon() {
    let exam = Exam::parse(theseus_exam::item::EXAM_V2).unwrap();
    let sessions = reads_identically(&exam);
    assert!(sessions >= 500, "{sessions} sessions");
}

/// Write `exam`'s past, read it with the library, serve it with the daemon,
/// and compare; the number of sessions compared.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn reads_identically(exam: &Exam) -> usize {
    let bin = theseusd();
    let dir = tempfile::tempdir().unwrap();
    let path = |p: &str| dir.path().join(p);
    for d in ["bin", "projects", "state"] {
        std::fs::create_dir_all(path(d)).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path("bin/op"), FAKE_OP).unwrap();
    std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(path("config.toml"), safe_config(&bin, &path("projects"))).unwrap();

    // Write the past, and read it as the library reads it.
    let m = fixture::write(exam, &path("state/store")).unwrap();
    let (lib_sessions, lib_nodes) = {
        let store = Store::open(&path("state/store")).unwrap();
        let recs: Vec<SessionRecord> = store.list_sessions().unwrap();
        let sessions: Vec<Value> = m
            .sessions
            .iter()
            .map(|e| {
                let r = recs.iter().find(|r| r.session_id == e.session_id).unwrap();
                stored_fields(&serde_json::to_value(r.info()).unwrap())
            })
            .collect();
        let nodes: Vec<Value> = m
            .sessions
            .iter()
            .map(|e| {
                let ns = store.session_nodes(&e.session_id).unwrap();
                serde_json::to_value(
                    ns.iter()
                        .map(|(p, n)| Core::node_info(*p, n))
                        .collect::<Vec<_>>(),
                )
                .unwrap()
            })
            .collect();
        (sessions, nodes)
    };

    // The daemon serves it.
    let log = path("theseusd.log");
    let logf = std::fs::File::create(&log).unwrap();
    let mut cmd = Command::new(&bin);
    cmd.arg("--config")
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
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(logf);
    let mut d = Daemon(cmd.spawn().unwrap());
    let sock = path("sock");
    let h = wait("socket", &log, || call(&sock, "health", Value::Null).ok());
    assert_eq!(h["sessions"], m.sessions.len(), "{h}");
    let phase = |h: &Value, name: &str| -> Option<Value> {
        h["startup"]
            .as_array()?
            .iter()
            .find(|p| p["name"] == name && !p["end_us"].is_null())
            .map(|p| p["detail"].clone())
    };
    let store = phase(&h, "store").expect("the store phase");
    assert_eq!(store["last_position"], m.last_position, "{store}");
    assert_eq!(store["index_repaired"], false, "{store}");
    let verify = wait("the history check", &log, || {
        phase(&call(&sock, "health", Value::Null).ok()?, "store.verify")
    });
    assert_eq!(verify["outcome"], "ok", "{verify}");
    assert!(
        verify["records"].as_u64().unwrap() >= m.last_position,
        "{verify}"
    );

    let list = call(&sock, "session.list", json!({})).unwrap();
    let served = list["sessions"].as_array().unwrap();
    assert_eq!(served.len(), m.sessions.len());
    let mut nodes_compared = 0;
    for ((e, lib_s), lib_n) in m.sessions.iter().zip(&lib_sessions).zip(&lib_nodes) {
        let s = served
            .iter()
            .find(|s| s["session_id"] == e.session_id.as_str())
            .unwrap();
        assert_eq!(&stored_fields(s), lib_s, "session {}/{}", e.item, e.key);
        assert_eq!(
            s["execution_state"], "waiting",
            "session {}/{}",
            e.item, e.key
        );
        let h = call(
            &sock,
            "session.history",
            json!({"session_id": e.session_id}),
        )
        .unwrap();
        assert_eq!(
            &stored_fields(&h["session"]),
            lib_s,
            "history's session {}/{}",
            e.item,
            e.key
        );
        assert_eq!(&h["nodes"], lib_n, "the nodes of {}/{}", e.item, e.key);
        nodes_compared += lib_n.as_array().unwrap().len();
    }
    // Every keyed node is among them, as the manifest names it.
    for (k, entry) in &m.nodes {
        let lib = lib_nodes
            .iter()
            .flat_map(|n| n.as_array().unwrap())
            .find(|n| n["node_id"] == entry.node_id.as_str())
            .unwrap_or_else(|| panic!("{k} is not served"));
        assert_eq!(lib["position"], entry.position, "{k}");
        assert_eq!(lib["at_unix_ms"], entry.at_ms, "{k}");
        assert_eq!(lib["text"], entry.text.as_str(), "{k}");
    }
    assert!(
        nodes_compared > m.nodes.len(),
        "a tool node is three: {nodes_compared}"
    );

    // A clean stop; the keyed nodes are where they were.
    call(&sock, "shutdown", Value::Null).ok();
    wait("the stop", &log, || d.0.try_wait().unwrap());
    let store = Store::open(&path("state/store")).unwrap();
    for e in &m.sessions {
        let ns = store.session_nodes(&e.session_id).unwrap();
        let after: Vec<_> = ns.iter().map(|(p, n)| Core::node_info(*p, n)).collect();
        let before = &lib_nodes[m.sessions.iter().position(|x| x == e).unwrap()];
        assert_eq!(
            &serde_json::to_value(after).unwrap(),
            before,
            "{}/{} after the stop",
            e.item,
            e.key
        );
    }
    m.sessions.len()
}
