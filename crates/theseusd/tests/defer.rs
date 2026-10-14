//! Deferred tools on a real daemon (theseus-7m7w.2): a stand-in Messages API
//! reads the requests the real `theseusd` sends. With `[tools.defer]` on (the
//! default; the other tests' notes turn it off), a CLI session's first request
//! declares the families its place does not load with `defer_loading` and
//! carries the provider's tool search tool first; `tool.defer` and health say
//! the same; and a call to a deferred tool nothing searched for is answered
//! with the search's words and counted.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`: every reference reads as the same test value.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
    \x20 read) printf '%s' test-secret-value-0000 ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    model: FakeModel,
}

impl Rig {
    fn start() -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.starts_with("HIDDEN") {
                vec![("term_open", json!({"argv": ["true"]}))]
            } else {
                vec![]
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        let op = path("bin/op");
        std::fs::write(&op, FAKE_OP).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        fn table<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
            t.entry(key)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
        }
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        // The default: on.
        table(table(&mut t, "tools"), "defer").insert("enabled".into(), true.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
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
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let r = Self { dir, daemon, model };
        r.wait("the secrets", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] == "ready")
        });
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s (daemon pid {}):\n{}",
                self.daemon.id(),
                std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| json!(e.to_string()))?;
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

    /// A turn in a new session: its result, once it ends.
    fn turn(&self, prompt: &str) -> Value {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn rows(&self, kind: &str) -> Vec<Value> {
        self.call("ledger.tail", json!({"n": 200, "kind": kind}))
            .unwrap()["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

fn tools_of(req: &Value) -> &Vec<Value> {
    req["tools"].as_array().unwrap()
}

fn find<'a>(req: &'a Value, name: &str) -> Option<&'a Value> {
    tools_of(req).iter().find(|t| t["name"] == name)
}

#[test]
fn a_daemons_first_request_defers_what_its_place_does_not_load() {
    let r = Rig::start();
    r.turn("hello");
    r.turn("a second session");
    let reqs = r.model.requests();
    let (a, b) = (&reqs[0], &reqs[1]);
    assert_eq!(
        tools_of(a)[0]["name"],
        "tool_search_tool_bm25",
        "{:?}",
        tools_of(a)
    );
    assert_eq!(tools_of(a)[0]["type"], "tool_search_tool_bm25_20251119");
    assert!(find(a, "fs_read").unwrap().get("defer_loading").is_none());
    assert_eq!(find(a, "term_open").unwrap()["defer_loading"], true);
    assert_eq!(find(a, "http_fetch").unwrap()["defer_loading"], true);
    // Two sessions of one kind of place share their tools, byte for byte.
    assert_eq!(a["tools"], b["tools"]);
    assert_eq!(
        serde_json::to_string(&a["tools"]).unwrap(),
        serde_json::to_string(&b["tools"]).unwrap()
    );

    // The listing and health say it.
    let list = r.call("tool.defer", Value::Null).unwrap();
    assert_eq!(list["enabled"], true);
    assert_eq!(list["mechanism"], "search");
    let cli = list["places"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["place"] == "cli")
        .unwrap();
    let family = |f: &str| {
        cli["families"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["family"] == f)
            .unwrap()
            .clone()
    };
    assert_eq!(family("files")["loaded"], true);
    assert_eq!(family("terminals")["loaded"], false);
    assert!(
        cli["tokens_sent"].as_u64().unwrap() < cli["tokens_all"].as_u64().unwrap(),
        "{cli}"
    );
    let h = r.call("health", Value::Null).unwrap();
    assert_eq!(h["tools_defer"]["enabled"], true, "{h}");
    // Each turn's use is in the ledger.
    let used = r.rows("tools.used");
    assert_eq!(used.len(), 2, "{used:?}");
}

#[test]
fn a_call_to_a_deferred_tool_nothing_found_is_not_run_and_is_counted() {
    let r = Rig::start();
    let res = r.turn("HIDDEN");
    assert_eq!(res["tool_calls"], 0, "{res}");
    let missed = r.rows("tools.missed");
    assert_eq!(missed.len(), 1, "{missed:?}");
    assert_eq!(missed[0]["data"]["tool"], "term_open");
    assert_eq!(missed[0]["data"]["family"], "terminals");
    assert!(r.rows("term.opened").is_empty(), "nothing opened");
    let h = r.call("health", Value::Null).unwrap();
    assert_eq!(h["tools_defer"]["missed"], 1, "{h}");
    // The answer the model read names the search.
    let second = r.model.requests().pop().unwrap();
    let last = serde_json::to_string(&second["messages"]).unwrap();
    assert!(
        last.contains("tool_search_tool_bm25") && last.contains("is not loaded"),
        "{last}"
    );
}
