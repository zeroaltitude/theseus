//! Credential requests under L1 (M4 18d), with the real `theseusd`, its real
//! job wrappers and L1 init, the real helper (`theseus-cred`, this binary
//! bound into the job's view), a fake `op`, and a stand-in model that asks
//! for the calls. Every job here that asks runs in a real L1 sandbox:
//! - a notify secret is given at once, with its notice, and its value is in
//!   no file under the state dir and not in the daemon's log (B1's check);
//!   the job's socket goes once the job has settled;
//! - an approve secret waits for the operator, and a decline is the error
//!   the helper prints;
//! - a name the broker may not hand out is refused, and an L0 job has no
//!   socket and no helper;
//! - a cancelled job's socket goes.
//!
//! The rig's state directory and socket are inside its workspace root, so
//! the view hides them only because 17b hides them.

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

/// What the fake `op` resolves every secret to.
const VALUE: &str = "test-secret-value-0000";

/// The tool calls the stand-in model makes for each prompt, by its start.
type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    _model: FakeModel,
}

/// A job's script that asks for `$1` and says what it got, never the value:
/// the helper's exit code, whether the value is the fake `op`'s (by its
/// prefix and length, so the call's input, which is stored, never holds it),
/// the helper's error, and where the helper is.
const ASK: &str = r#"v="$(theseus-cred get "$1" 2>/tmp/err)"; rc=$?
echo "rc=$rc"
case "$v" in test-secret-value-*) [ ${#v} -eq 22 ] && echo got=yes || echo got=no ;; *) echo got=no ;; esac
echo "err=$(tr '\n' ' ' </tmp/err)"
echo "helper=$(command -v theseus-cred || echo none)"
"#;

impl Rig {
    fn start(tweak: impl FnOnce(&mut toml::Table)) -> Self {
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
        for d in ["bin", "projects", "home"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(
            path("bin/op"),
            "#!/bin/sh\n\
             case \"$1\" in\n\
             \x20 inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n\
             \x20 read) printf '%s' test-secret-value-0000 ;;\n\
             \x20 *) exit 1 ;;\n\
             esac\n",
        )
        .unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
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
        tbl(&mut t, "tools").insert("proc_sync_secs".into(), 30.into());
        tbl(&mut t, "secrets").insert(
            "github_token".into(),
            "op://Test/github_token/credential".into(),
        );
        let gh: toml::Table = toml::from_str("env = { GH_TOKEN = \"github_token\" }").unwrap();
        tbl(tbl(&mut t, "broker"), "programs").insert("gh".into(), toml::Value::Table(gh));
        tweak(&mut t);
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            Command::new(&theseusd)
                .arg("--config")
                .arg(path("config.toml"))
                .arg("--state-dir")
                .arg(path("projects/state"))
                .arg("--socket")
                .arg(path("projects/sock"))
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
                .env_remove("THESEUS_OPERATOR_UMASK")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(log),
        );
        let mut r = Self {
            dir,
            daemon,
            script,
            _model: model,
        };
        r.until("the secrets", |h| h["secrets"]["state"] == "ready");
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn asks(&self, prompt: &str, calls: Vec<(&'static str, Value)>) {
        self.script.lock().unwrap().push((prompt.into(), calls));
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
        call_at(&self.path("projects/sock"), method, params)
    }

    /// A new session whose model makes `calls`, and its id.
    fn session(&self, prompt: &str, calls: Vec<(&'static str, Value)>) -> Value {
        self.asks(prompt, calls);
        self.call("session.open", json!({"label": prompt})).unwrap()["session_id"].clone()
    }

    /// A turn in a new session whose model makes `calls`, and the text of
    /// each tool result, once the turn ends.
    fn turn(&self, prompt: &str, calls: Vec<(&'static str, Value)>) -> Vec<String> {
        let sid = self.session(prompt, calls);
        let res = submit(&self.path("projects/sock"), &sid, prompt);
        assert_eq!(res["stop_reason"], "no_tool_calls", "{res}\n{}", self.log());
        self.results(&sid)
    }

    fn results(&self, sid: &Value) -> Vec<String> {
        let h = self
            .call("session.history", json!({"session_id": sid}))
            .unwrap();
        h["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["kind"] == "tool_result")
            .map(|n| n["text"].as_str().unwrap_or_default().to_string())
            .collect()
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

    /// The jobs whose credential sockets the spool holds now.
    fn sockets(&self) -> Vec<String> {
        std::fs::read_dir(self.path("projects/state/spool/broker"))
            .map(|d| {
                d.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Wait, bounded, until no job's socket is served.
    fn no_sockets(&self) {
        let t0 = Instant::now();
        while !self.sockets().is_empty() {
            assert!(
                t0.elapsed() < Duration::from_secs(10),
                "still served: {:?}\n{}",
                self.sockets(),
                self.log()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

/// B1's check: the files under `under` that hold the value's bytes.
fn holding(under: &Path) -> Vec<PathBuf> {
    fn walk(p: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        for e in rd.flatten() {
            let path = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => walk(&path, out),
                Ok(t) if t.is_file() => {
                    let held = std::fs::read(&path)
                        .is_ok_and(|b| b.windows(VALUE.len()).any(|w| w == VALUE.as_bytes()));
                    if held {
                        out.push(path);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(under, &mut out);
    out
}

fn call_at(sock: &Path, method: &str, params: Value) -> Result<Value, Value> {
    let s = UnixStream::connect(sock).map_err(|e| json!(e.to_string()))?;
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

fn submit(sock: &Path, sid: &Value, prompt: &str) -> Value {
    call_at(
        sock,
        "turn.submit",
        json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
    )
    .unwrap()
}

/// `key=value` lines into their values.
fn said(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key}= in:\n{text}"))
        .to_string()
}

/// An L1 job that asks for `secret` (`ASK`).
fn ask_call(secret: &str) -> (&'static str, Value) {
    let argv = ["sh", "-c", ASK, "ask", secret];
    ("proc_run", json!({"argv": argv, "sandbox": true}))
}

/// What an L0 job finds of the helper and the socket: neither.
const L0: &str = r#"echo "helper=$(command -v theseus-cred || echo none)"
if [ -e /run/theseus ]; then echo run=yes; else echo run=no; fi
"#;

/// The design's test (§3's 18d block): a job in L1 asks for a secret whose
/// posture is notify (github_token, granted to gh, notify by default), and
/// gets it at once: the helper is on its PATH, its value is the one the
/// board holds, `secret.requested` and `secret.granted { via: request }`
/// say so, and the value is in no file under the state dir nor in the log.
/// The job's socket goes once the job has settled.
#[test]
fn a_job_in_l1_gets_a_notify_secret_at_once_and_it_is_written_nowhere() {
    let r = Rig::start(|_| {});
    let out = r.turn("ask in L1", vec![ask_call("github_token")]);
    let text = &out[0];
    assert_eq!(said(text, "rc"), "0", "{text}\n{}", r.log());
    assert_eq!(said(text, "got"), "yes", "{text}");
    assert_eq!(said(text, "helper"), "/run/theseus/bin/theseus-cred");
    let asked = r.ledger("secret.requested");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(
        (&asked[0]["outcome"], &asked[0]["posture"]),
        (&json!("granted"), &json!("notify"))
    );
    assert_eq!(asked[0]["command"], "sh");
    let granted: Vec<Value> = r
        .ledger("secret.granted")
        .into_iter()
        .filter(|g| g["via"] == "request")
        .collect();
    assert_eq!(granted.len(), 1, "{granted:?}");
    let h = r.call("health", Value::Null).unwrap();
    assert_eq!(h["cred_requests"]["granted"], 1, "{}", h["cred_requests"]);
    r.no_sockets();
    let state = holding(&r.path("projects/state"));
    assert!(state.is_empty(), "the value is in {state:?}");
    let log = std::fs::read_to_string(r.path("theseusd.log")).unwrap();
    assert!(!log.contains(VALUE), "the value is in the daemon's log");
}

/// Approve waits for the operator, here through `action.confirm` from the
/// test's process (outside every job), and a decline is the error the
/// helper prints and exits 1 with; `secret.declined` says who.
#[test]
fn an_approve_secret_waits_and_a_decline_is_the_helpers_error() {
    if let Some(job) = common::job_above_this_test() {
        eprintln!("skipped: this test runs under job {job}, whose answer is refused");
        return;
    }
    let r = Rig::start(|t| {
        let s: toml::Table = toml::from_str("posture = \"approve\"").unwrap();
        let broker = t["broker"].as_table_mut().unwrap();
        let mut secrets = toml::Table::new();
        secrets.insert("github_token".into(), toml::Value::Table(s));
        broker.insert("secrets".into(), toml::Value::Table(secrets));
    });
    let sid = r.session("ask and wait", vec![ask_call("github_token")]);
    let sock = r.path("projects/sock");
    let turn = {
        let sid = sid.clone();
        std::thread::spawn(move || submit(&sock, &sid, "ask and wait"))
    };
    let t0 = Instant::now();
    let q = loop {
        let listed = r.call("confirm.list", Value::Null).unwrap();
        let asks: Vec<Value> = listed["confirms"]
            .as_array()
            .or_else(|| listed.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(q) = asks.iter().find(|c| c["tool"] == "cred.request") {
            break q.clone();
        }
        assert!(
            t0.elapsed() < Duration::from_secs(30),
            "no request waits: {listed}\n{}",
            r.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        q["reason"]
            .as_str()
            .unwrap()
            .contains("asks for `github_token`"),
        "{q}"
    );
    r.call(
        "action.confirm",
        json!({"correlation_id": q["correlation_id"], "approve": false, "note": "not now"}),
    )
    .unwrap();
    let res = turn.join().unwrap();
    assert_eq!(res["stop_reason"], "no_tool_calls", "{res}");
    let text = &r.results(&sid)[0];
    assert_eq!(said(text, "rc"), "1", "{text}");
    assert_eq!(said(text, "got"), "no");
    assert!(said(text, "err").contains("declined: not now"), "{text}");
    let declined = r.ledger("secret.declined");
    assert_eq!(declined[0]["why"], "not now", "{declined:?}");
    let asked = r.ledger("secret.requested");
    assert_eq!(asked[0]["outcome"], "waiting");
    r.no_sockets();
}

/// A name the broker may not hand out is refused (an input error), and an
/// L0 job has no socket and no helper.
#[test]
fn an_unknown_name_is_refused_and_an_l0_job_has_no_socket() {
    let r = Rig::start(|_| {});
    let out = r.turn("ask for a key", vec![ask_call("anthropic_api_key")]);
    let text = &out[0];
    assert_eq!(said(text, "rc"), "1", "{text}");
    assert!(
        said(text, "err").contains("is not a secret a job may ask for"),
        "{text}"
    );
    let l0 = ("proc_run", json!({"argv": ["sh", "-c", L0]}));
    let out = r.turn("look at L0", vec![l0]);
    let text = &out[0];
    assert_eq!(said(text, "helper"), "none", "{text}");
    assert_eq!(said(text, "run"), "no", "{text}");
    assert!(r.ledger("secret.declined").len() == 1);
    r.no_sockets();
}

/// A cancelled job's socket goes: a job in L1 that sleeps past the turn's
/// wait, its socket served, then its execution cancelled.
#[test]
fn a_cancelled_jobs_socket_goes() {
    let r = Rig::start(|t| {
        t["tools"]
            .as_table_mut()
            .unwrap()
            .insert("proc_sync_secs".into(), 1.into());
    });
    let sleep = (
        "proc_run",
        json!({"argv": ["sleep", "30"], "sandbox": true}),
    );
    let sid = r.session("sleep in L1", vec![sleep]);
    let res = submit(&r.path("projects/sock"), &sid, "sleep in L1");
    assert_eq!(res["stop_reason"], "no_tool_calls", "{res}");
    assert_eq!(r.sockets().len(), 1, "served while the job runs");
    r.call(
        "execution.cancel",
        json!({"execution_id": res["execution_id"]}),
    )
    .unwrap();
    r.no_sockets();
}
