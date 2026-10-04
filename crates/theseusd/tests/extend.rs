//! `extend.propose` (M7 43a) with the real `theseusd`, a stand-in for the
//! Messages API that asks for the proposal, and the real fake MCP server
//! (`theseus-sim fake-mcp`) as the proposed word counter: its tree is
//! frozen, its frozen copy started in L1 through the `mcp-sandbox` role and
//! tested, and the operator asked. `theseus confirm` from a job's shell
//! (`THESEUS_SESSION`) is refused and the question keeps waiting; from the
//! operator's own, it acks: `extend.acked`, and it loads (43b): its tools
//! run, a restart starts it after serving from its frozen copy, neither the
//! daemon's stop nor its `kill -9` leaves it running, and a revoke from a
//! job's shell is refused while the operator's stops it.
//!
//! As root, L1 refuses the trial (theseus-pv6i): the proposal says so, and
//! nothing is asked. Run the rest as an ordinary user (on a root-only
//! machine, as uid 65534: see the cloud report for 43a).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    _model: FakeModel,
}

fn bin(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_BIN_EXE_theseusd")).with_file_name(name);
    assert!(
        p.is_file(),
        "no {name} at {}: build the workspace",
        p.display()
    );
    p
}

fn root() -> bool {
    // SAFETY: no arguments.
    unsafe { libc::geteuid() == 0 }
}

impl Drop for Rig {
    /// A frozen copy is read-only: writable again, so the temp dir goes.
    fn drop(&mut self) {
        let _ = Command::new("chmod")
            .arg("-R")
            .arg("u+w")
            .arg(self.path("state/extensions"))
            .status();
    }
}

impl Rig {
    fn start() -> Self {
        let script = Script::default();
        let asks = script.clone();
        let model = FakeModel::start(move |prompt| {
            asks.lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.starts_with(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        for d in ["bin", "projects/wc", "home"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        let exe = |p: PathBuf, body: &str| {
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exe(
            path("bin/op"),
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
             \x20 read) printf '%s' test-secret-value-0000 ;;\n\
             \x20 *) exit 1 ;;\n\
             esac\n",
        );
        // The word counter the model wrote: the fake, from its frozen copy.
        exe(
            path("projects/wc/server.sh"),
            &format!(
                "#!/bin/sh\nexec {} fake-mcp\n",
                bin("theseus-sim").display()
            ),
        );
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        let mut t: toml::Table = common::safe_note(&theseusd, &path("projects"), 100.0)
            .parse()
            .unwrap();
        fn tbl<'a>(t: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
            t.entry(key)
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
        }
        tbl(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in tbl(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        tbl(&mut t, "policy").insert("enforcement".into(), "notify".into());
        // The fake's binary, bound into the view: it is not under the
        // workspace.
        let sim_dir = bin("theseus-sim").parent().unwrap().display().to_string();
        tbl(&mut t, "sandbox").insert("ro_paths".into(), vec![sim_dir].into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let daemon = spawn(dir.path());
        let mut r = Self {
            dir,
            daemon,
            script,
            _model: model,
        };
        r.until("the secrets", |h| h["secrets"]["state"] == "ready");
        r
    }

    /// A new daemon on the same state, once the last one has gone.
    fn respawn(&mut self) {
        let t0 = Instant::now();
        while self.daemon.try_wait().is_none() {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "the daemon did not stop"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        self.daemon = spawn(self.dir.path());
        self.until("the secrets", |h| h["secrets"]["state"] == "ready");
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn until(&mut self, what: &str, ok: impl Fn(&Value) -> bool) -> Value {
        let t0 = Instant::now();
        loop {
            if let Ok(h) = self.call("health", Value::Null) {
                if ok(&h) {
                    return h;
                }
            }
            if let Some(status) = self.daemon.try_wait() {
                panic!("theseusd exited ({status}) before {what}:\n{}", self.log());
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        let s = UnixStream::connect(self.path("sock")).map_err(|e| json!(e.to_string()))?;
        s.set_read_timeout(Some(Duration::from_secs(90))).unwrap();
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

    /// A turn in a new session whose model makes `calls`: its session, and
    /// the text of each tool result.
    fn turn(&self, prompt: &str, calls: Vec<(&'static str, Value)>) -> (String, Vec<String>) {
        self.script.lock().unwrap().push((prompt.into(), calls));
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        let sid = s["session_id"].as_str().unwrap().to_string();
        let res = self
            .call(
                "turn.submit",
                json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
            )
            .unwrap();
        assert_eq!(res["stop_reason"], "no_tool_calls", "{res}\n{}", self.log());
        let h = self
            .call("session.history", json!({"session_id": sid}))
            .unwrap();
        let results = h["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"] == "tool_result")
            .map(|n| n["text"].as_str().unwrap_or_default().to_string())
            .collect();
        (sid, results)
    }

    fn ledger(&self, kind: &str) -> Vec<Value> {
        let t = self.call("ledger.tail", json!({"n": 2000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == kind)
            .map(|r| r["data"].clone())
            .collect()
    }

    /// `theseus extend list`'s output.
    fn extend_list(&self) -> String {
        let out = Command::new(bin("theseus"))
            .arg("--socket")
            .arg(self.path("sock"))
            .args(["extend", "list"])
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// `theseus confirm <id>`, with `env` set: its status and output.
    fn confirm(&self, id: &str, env: &[(&str, &str)]) -> (bool, String) {
        let out = Command::new(bin("theseus"))
            .arg("--socket")
            .arg(self.path("sock"))
            .args(["confirm", id])
            .env_remove("THESEUS_SESSION")
            .envs(env.iter().copied())
            .output()
            .unwrap();
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

fn proposal(r: &Rig) -> (&'static str, Value) {
    (
        "extend_propose",
        json!({
            "name": "wordcount",
            "dir": r.path("projects/wc").display().to_string(),
            "command": ["sh", "server.sh"],
            "description": "Counts words.",
            "tests": [
                {"tool": "echo", "arguments": {"text": "one two three"}, "expect": {"contains": "one two three"}},
                {"tool": "add", "arguments": {"a": 1, "b": 2}, "expect": {"contains": "3"}}
            ]
        }),
    )
}

/// The daemon on `dir`'s config and state, its log appended to.
fn spawn(dir: &Path) -> Daemon {
    let path = |p: &str| dir.join(p);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path("theseusd.log"))
        .unwrap();
    Daemon::spawn(
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
            .env("HOME", path("home"))
            .env("OP_SERVICE_ACCOUNT_TOKEN", "test-not-a-token")
            .env_remove("THESEUS_OP_TOKEN_FILE")
            .env_remove("THESEUS_CONFIG")
            .env_remove("THESEUS_STATE_DIR")
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_SESSION")
            .env_remove("THESEUS_OPERATOR_UMASK")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log),
    )
}

/// As root, L1 refuses the trial: the proposal says why, its manifest is
/// `failed`, and nothing is asked.
fn refused_as_root(r: &Rig, text: &str, tested: &Value) {
    assert!(text.contains("did not come up in L1"), "{text}");
    assert!(text.contains("RLIMIT_NPROC"), "{text}");
    assert!(tested["error"].is_string(), "{tested}");
    let asks = r.call("confirm.list", Value::Null).unwrap();
    assert!(asks["confirms"].as_array().unwrap().is_empty(), "{asks}");
    let listed = r.extend_list();
    assert!(
        listed.starts_with("wordcount ") && listed.contains("  failed  "),
        "{listed}"
    );
    assert!(listed.contains("did not come up in L1"), "{listed}");
}

/// The brief's live check, offline: proposed, frozen, tried in L1, and put
/// to the operator; a job's shell cannot ack it; the operator's shell does;
/// and no extension's tool is offered or runs.
#[test]
fn a_proposed_extension_is_tried_in_l1_and_acked_from_the_operators_shell_alone() {
    let mut r = Rig::start();
    let (sid, results) = r.turn("propose the counter", vec![proposal(&r)]);
    let text = &results[0];
    let tested = &r.ledger("extend.tested")[0];
    assert_eq!(r.ledger("extend.proposed").len(), 1, "{}", r.log());
    if root() {
        return refused_as_root(&r, text, tested);
    }
    assert!(text.contains("2 of 2 tests passed"), "{text}\n{}", r.log());
    assert_eq!(
        (
            tested["passed"].as_u64(),
            tested["tools"].as_array().map(Vec::len)
        ),
        (Some(2), Some(5)),
        "{tested}"
    );
    // Nothing of the trial runs on.
    let h = r.until("the trial's end", |h| {
        h["mcp"].as_array().is_none_or(|m| m.is_empty())
    });
    assert!(
        h["mcp"].as_array().is_none_or(|m| m.is_empty()),
        "{}",
        h["mcp"]
    );
    // Its role is reaped: no zombie is left of it.
    r.until("no zombie", |h| h["children"]["zombies"] == 0);
    // The question, as `theseus confirm` lists it.
    let asks = r.call("confirm.list", Value::Null).unwrap();
    let q = &asks["confirms"][0];
    assert_eq!(q["tool"], "extend.ack", "{asks}");
    assert_eq!(q["session_id"], sid.as_str());
    assert!(
        q["reason"]
            .as_str()
            .unwrap()
            .ends_with(": 5 tools, 2 of 2 tests passed, no network?"),
        "{q}"
    );
    let id = q["correlation_id"].as_str().unwrap().to_string();
    let listed = r.extend_list();
    assert!(
        listed.contains("  proposed  2 of 2 tests passed  no network"),
        "{listed}"
    );
    assert!(
        listed.contains(&format!("waiting: theseus confirm {id}")),
        "{listed}"
    );
    // From a job's shell: refused, and it still waits.
    let (ok, said) = r.confirm(&id, &[("THESEUS_SESSION", sid.as_str())]);
    assert!(!ok && said.contains("theseus confirm refused"), "{said}");
    assert!(r.ledger("extend.acked").is_empty());
    assert_eq!(
        r.call("confirm.list", Value::Null).unwrap()["confirms"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // From the operator's own: acked.
    let (ok, said) = r.confirm(&id, &[]);
    assert!(ok, "{said}");
    assert!(said.contains("nothing resumes"), "{said}");
    assert_eq!(r.ledger("extend.acked")[0]["name"], "wordcount");
    assert!(
        r.extend_list().contains("  acked by "),
        "{}",
        r.extend_list()
    );
    let h = r.call("health", Value::Null).unwrap();
    assert_eq!(h["extensions"]["acked"], 1, "{}", h["extensions"]);
    // Acked, it loads (43b): its tools are offered from the next turn.
    let list = r.call("mcp.list", Value::Null).unwrap();
    assert_eq!(list["tools"].as_array().unwrap().len(), 5, "{list}");
    assert!(r.call("confirm.list", Value::Null).unwrap()["confirms"]
        .as_array()
        .unwrap()
        .is_empty());
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

impl Rig {
    /// `theseus <args>`, with `env` set: its status and output.
    fn cli(&self, args: &[&str], env: &[(&str, &str)]) -> (bool, String) {
        let out = Command::new(bin("theseus"))
            .arg("--socket")
            .arg(self.path("sock"))
            .args(args)
            .env_remove("THESEUS_SESSION")
            .envs(env.iter().copied())
            .output()
            .unwrap();
        (
            out.status.success(),
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
        )
    }

    /// `ext-wordcount` ready: its role and every process below it (the
    /// role, the init, the server), once the server is there.
    fn ext_ready(&mut self) -> Vec<u32> {
        let h = self.until("ext-wordcount ready", |h| {
            h["mcp"].as_array().is_some_and(|m| {
                m.iter()
                    .any(|s| s["name"] == "ext-wordcount" && s["state"] == "ready")
            })
        });
        let s = h["mcp"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "ext-wordcount")
            .unwrap()
            .clone();
        let role = s["pid"].as_u64().unwrap() as u32;
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

    /// The wire names `mcp.list` offers.
    fn mcp_tools(&self) -> Vec<String> {
        let list = self.call("mcp.list", Value::Null).unwrap();
        list["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["wire_name"].as_str().unwrap().to_string())
            .collect()
    }

    /// The ledger's kinds, oldest first.
    fn kinds(&self) -> Vec<(String, Value)> {
        let t = self.call("ledger.tail", json!({"n": 5000})).unwrap();
        t["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["kind"].as_str().unwrap().to_string(), r["data"].clone()))
            .collect()
    }
}

/// 43b's live check, offline: acked, the extension loads and its tool runs;
/// a stop ends its processes, and a start offers its tools at once and
/// starts it after serving, from the frozen copy; a `kill -9` of the daemon
/// leaves none of it running; a revoke from a job's shell is refused, and
/// the operator's stops it and drops its tools.
#[test]
fn an_acked_extension_loads_survives_a_restart_and_is_revoked() {
    let mut r = Rig::start();
    let (_, results) = r.turn("propose the counter", vec![proposal(&r)]);
    if root() {
        // L1 refuses the trial (the first test says how): nothing to load.
        assert!(
            results[0].contains("did not come up in L1"),
            "{}",
            results[0]
        );
        return;
    }
    let asks = r.call("confirm.list", Value::Null).unwrap();
    let id = asks["confirms"][0]["correlation_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (ok, said) = r.confirm(&id, &[]);
    assert!(ok, "{said}");
    // Loaded: the row, its tools in the catalog, its server in L1.
    let loaded = &r.ledger("extend.loaded")[0];
    assert_eq!(loaded["server"], "ext-wordcount", "{loaded}");
    assert!(r
        .mcp_tools()
        .iter()
        .any(|t| t == "mcp__ext-wordcount__echo"));
    let pids = r.ext_ready();
    let _seen = Reap(pids.clone());
    let listed = r.extend_list();
    assert!(
        listed.contains("loaded as ext-wordcount  ready"),
        "{listed}"
    );
    // The next turn calls it, at notify.
    let (_, results) = r.turn(
        "count the words",
        vec![(
            "mcp__ext-wordcount__echo",
            json!({"text": "four words right here"}),
        )],
    );
    assert!(
        results[0].contains("four words right here"),
        "{results:?}\n{}",
        r.log()
    );
    // The daemon's stop ends it, its role and its namespace.
    let _ = r.call("shutdown", Value::Null);
    wait_gone(&pids, "the daemon's stop");
    // A start offers its tools at once, and starts it after serving.
    r.respawn();
    assert!(r
        .mcp_tools()
        .iter()
        .any(|t| t == "mcp__ext-wordcount__echo"));
    let pids = r.ext_ready();
    let _seen2 = Reap(pids.clone());
    let kinds = r.kinds();
    let serving = kinds
        .iter()
        .rposition(|(k, _)| k == "server.serving")
        .expect("the start's serving row");
    let started = kinds
        .iter()
        .rposition(|(k, d)| k == "mcp.started" && d["server"] == "ext-wordcount")
        .expect("its start");
    assert!(serving < started, "it started after serving");
    // From the frozen copy: the role's server runs there.
    let frozen = r.extend_list();
    assert!(frozen.contains("state/extensions/wordcount/"), "{frozen}");
    let (_, results) = r.turn(
        "count again",
        vec![(
            "mcp__ext-wordcount__echo",
            json!({"text": "after the restart"}),
        )],
    );
    assert!(results[0].contains("after the restart"), "{results:?}");
    // The daemon's kill -9 leaves none of it running.
    kill(r.daemon.id(), libc::SIGKILL);
    wait_gone(&pids, "the daemon's kill -9");
    r.respawn();
    let pids = r.ext_ready();
    let _seen3 = Reap(pids.clone());
    revoked_from_the_operators_shell_alone(&r, &pids);
}

/// A revoke from a job's shell is refused, and it stays; the operator's
/// stops its processes and drops its tools, and keeps the frozen copy.
fn revoked_from_the_operators_shell_alone(r: &Rig, pids: &[u32]) {
    // A revoke from a job's shell is refused; it stays.
    let (ok, said) = r.cli(
        &["extend", "revoke", "wordcount"],
        &[("THESEUS_SESSION", "ses_0000aa1b2c3")],
    );
    assert!(
        !ok && said.contains("theseus extend revoke refused"),
        "{said}"
    );
    assert!(r.ledger("extend.revoked").is_empty());
    // The operator's: stopped, its tools gone, the frozen copy kept.
    let (ok, said) = r.cli(&["extend", "revoke", "wordcount"], &[]);
    assert!(ok && said.starts_with("Revoked wordcount "), "{said}");
    assert_eq!(r.ledger("extend.revoked")[0]["name"], "wordcount");
    wait_gone(pids, "the revoke");
    assert!(r.mcp_tools().is_empty(), "{:?}", r.mcp_tools());
    let h = r.call("health", Value::Null).unwrap();
    assert!(
        h["mcp"]
            .as_array()
            .is_none_or(|m| m.iter().all(|s| s["name"] != "ext-wordcount")),
        "{}",
        h["mcp"]
    );
    let listed = r.extend_list();
    assert!(!listed.contains("loaded as"), "{listed}");
    assert!(listed.contains("  revoked  "), "{listed}");
}
