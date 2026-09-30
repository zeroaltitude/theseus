//! The secret broker (theseus-dcy), with the real `theseusd`, its real job
//! wrappers, and stand-in programs on the daemon's own PATH, driven by a
//! stand-in for the Messages API. Each secret's value, from a fake `op`, is
//! `tv-<name>-7f3a9c`, so a scan for the marker finds any of them.
//! - `gh api user`, run by its own argv, gets `GH_TOKEN`, and nothing else
//!   of the vault: no other value, and no AWS key;
//! - `sh -c 'gh api user'` gets nothing, and its result says why;
//! - a secret whose posture is `approve` makes its call wait, and the call
//!   gets it once approved;
//! - a secret that did not resolve is withheld, and the result says so;
//! - no value is in the store, the spool, or the log, and no `op` ran for a
//!   grant;
//! - the ledger names each grant, and health counts its uses.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

/// What marks every secret's value.
const MARK: &str = "-7f3a9c";

/// A fake `op`: each reference's value is `tv-<its item>-7f3a9c`, and
/// `broken_token` never resolves. Each run is a line of `op.calls`, naming
/// the references it was asked for.
fn fake_op(dir: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         calls='{calls}'\n\
         case \"$1\" in\n\
         \x20 inject)\n\
         \x20   t=$(mktemp); cat > \"$t\"\n\
         \x20   echo \"inject $(grep -o 'op://Test/[^/]*' \"$t\" | tr '\\n' ' ')\" >> \"$calls\"\n\
         \x20   if grep -q broken_token \"$t\"; then rm -f \"$t\"; echo broken >&2; exit 1; fi\n\
         \x20   sed -E 's#\\{{\\{{ op://Test/([^/]+)/[^}}]* \\}}\\}}#tv-\\1{MARK}#g' \"$t\"; rm -f \"$t\" ;;\n\
         \x20 read)\n\
         \x20   for a; do ref=\"$a\"; done\n\
         \x20   echo \"read $ref\" >> \"$calls\"\n\
         \x20   name=$(echo \"$ref\" | cut -d/ -f4)\n\
         \x20   [ \"$name\" = broken_token ] && {{ echo '[ERROR] no such item' >&2; exit 1; }}\n\
         \x20   printf 'tv-%s{MARK}' \"$name\" ;;\n\
         \x20 *) exit 1 ;;\n\
         esac\n",
        calls = dir.join("op.calls").display()
    )
}

/// A stand-in for `gh` and its kin: which granted variables it holds, and
/// their lengths, how many values of the vault its environment holds, and
/// how many AWS names. Never a value.
const STUB: &str = "#!/bin/sh\n\
    for v in GH_TOKEN GHX_TOKEN GHB_TOKEN; do\n\
    \x20 eval \"x=\\${$v-}\"\n\
    \x20 [ -n \"$x\" ] && echo \"$v set, length ${#x}\"\n\
    done\n\
    echo \"values in env: $(env | grep -c -- '-7f3a9c')\"\n\
    echo \"aws names: $(env | grep -c '^AWS_')\"\n\
    echo \"login zeroaltitude\"\n";

struct Rig {
    dir: tempfile::TempDir,
    /// Killed and reaped when the rig goes.
    _daemon: Daemon,
    script: Script,
    _model: FakeModel,
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
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        let exe = |p: PathBuf, body: &str| {
            std::fs::write(&p, body).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        exe(path("bin/op"), &fake_op(dir.path()));
        for stub in ["gh", "ghx", "ghb"] {
            exe(path("bin").join(stub), STUB);
        }
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 10.into());
        for name in ["github_token", "approve_token", "broken_token"] {
            table(&mut t, "secrets")
                .insert(name.into(), format!("op://Test/{name}/credential").into());
        }
        let broker: toml::Table = r#"
            [programs.gh]
            env = { GH_TOKEN = "github_token" }
            [programs.ghx]
            env = { GHX_TOKEN = "approve_token" }
            [programs.ghb]
            env = { GHB_TOKEN = "broken_token" }
            [secrets.approve_token]
            posture = "approve"
        "#
        .parse()
        .unwrap();
        t.insert("broker".into(), broker.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        let log = std::fs::File::create(path("theseusd.log")).unwrap();
        let daemon = Daemon::spawn(
            std::process::Command::new(&theseusd)
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
        let r = Self {
            dir,
            _daemon: daemon,
            script,
            _model: model,
        };
        r.wait("the first round of secrets", || {
            r.call("health", Value::Null)
                .ok()
                .filter(|h| h["secrets"]["state"] != "resolving")
        });
        r
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn asks(&self, prompt: &str, argv: &[&str]) {
        self.script.lock().unwrap().push((
            prompt.into(),
            vec![("proc_run", json!({"argv": argv, "timeout_secs": 30}))],
        ));
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "no {what} in 30 s:\n{}",
                self.log()
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

    /// A turn in a new session: its result, once it parks or ends.
    fn turn(&self, prompt: &str) -> Value {
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        self.call(
            "turn.submit",
            json!({"session_id": s["session_id"], "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    /// The session's tool results, their text joined.
    fn results(&self, session: &Value) -> String {
        let h = self
            .call(
                "node.list",
                json!({"session_id": session, "kind": "tool_result"}),
            )
            .unwrap();
        h["nodes"]
            .as_array()
            .map(|n| {
                n.iter()
                    .map(|n| n["text"].as_str().unwrap_or("").to_string())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
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

    /// The `op` runs that named `secret`. The retries fetch only the one
    /// that failed.
    fn op_calls(&self, secret: &str) -> usize {
        std::fs::read_to_string(self.path("op.calls"))
            .map(|s| s.lines().filter(|l| l.contains(secret)).count())
            .unwrap_or(0)
    }

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

/// Every file at or under `at` whose bytes hold `needle`.
fn holding(at: &Path, needle: &[u8]) -> Vec<PathBuf> {
    let mut found = vec![];
    let holds =
        |p: &Path| std::fs::read(p).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle));
    if at.is_file() {
        return if holds(at) {
            vec![at.to_path_buf()]
        } else {
            vec![]
        };
    }
    let mut stack = vec![at.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if holds(&p) {
                found.push(p);
            }
        }
    }
    found
}

#[test]
fn a_program_run_by_its_own_argv_gets_its_secret_and_nothing_else_does() {
    let r = Rig::start();
    let ops = r.op_calls("github_token");
    assert!(ops >= 1, "the secrets came from op at startup");

    // Direct argv: GH_TOKEN, and no other value of the vault.
    r.asks("direct", &["gh", "api", "user"]);
    let direct = r.turn("direct");
    let out = r.results(&direct["session_id"]);
    assert!(
        out.contains(&format!(
            "GH_TOKEN set, length {}",
            "tv-github_token-7f3a9c".len()
        )),
        "{out}\n{}",
        r.log()
    );
    assert!(
        out.contains("values in env: 1") && out.contains("aws names: 0"),
        "{out}"
    );
    assert!(out.contains("login zeroaltitude"), "{out}");

    // Through a shell: nothing, and the result says why.
    r.asks("shell", &["sh", "-c", "gh api user"]);
    let shell = r.turn("shell");
    let out = r.results(&shell["session_id"]);
    assert!(
        !out.contains("GH_TOKEN set") && out.contains("values in env: 0"),
        "{out}"
    );
    assert!(
        out.contains("gh got no GH_TOKEN: it is run by `sh`, not by its own argv"),
        "{out}"
    );

    // A secret that did not resolve: withheld, and said so.
    r.asks("broken", &["ghb"]);
    let broken = r.turn("broken");
    let out = r.results(&broken["session_id"]);
    assert!(!out.contains("GHB_TOKEN set"), "{out}");
    assert!(
        out.contains("ghb got no GHB_TOKEN: broken_token did not resolve")
            && out.contains("it ran without it"),
        "{out}"
    );

    // A secret whose posture is approve: the call waits, and gets it once
    // the operator approves.
    r.asks("approve", &["ghx"]);
    let waiting = r.turn("approve");
    let corr = waiting["awaiting_confirm"]
        .as_str()
        .unwrap_or_else(|| panic!("the call waits: {waiting}\n{}", r.log()))
        .to_string();
    let confirms = r.call("confirm.list", Value::Null).unwrap();
    let reason = confirms["confirms"][0]["reason"].as_str().unwrap_or("");
    assert!(
        reason.contains("ghx gets GHX_TOKEN: [broker.secrets.approve_token] posture = approve"),
        "{confirms}"
    );
    r.call(
        "action.confirm",
        json!({"correlation_id": corr, "approve": true}),
    )
    .unwrap();
    let out = r.wait("the approved call's result", || {
        Some(r.results(&waiting["session_id"])).filter(|o| o.contains("GHX_TOKEN set"))
    });
    assert!(out.contains("values in env: 1"), "{out}");

    // The ledger names each grant, never a value; health counts the uses.
    let granted = r.ledger("secret.granted");
    let named: Vec<(&str, &str, &str)> = granted
        .iter()
        .map(|g| {
            (
                g["program"].as_str().unwrap_or(""),
                g["variable"].as_str().unwrap_or(""),
                g["secret"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        named,
        vec![
            ("gh", "GH_TOKEN", "github_token"),
            ("ghx", "GHX_TOKEN", "approve_token")
        ],
        "{granted:?}"
    );
    assert!(granted.iter().all(|g| g["correlation_id"].is_string()));
    let withheld = r.ledger("secret.withheld");
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    assert_eq!(withheld[0]["secret"], "broken_token");
    let h = r.call("health", Value::Null).unwrap();
    let uses: Vec<(String, u64)> = h["broker"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["to"].as_str().unwrap().to_string(),
                g["uses"].as_u64().unwrap(),
            )
        })
        .collect();
    // The wiring's own grant (DD5: web.search's key) is listed too, unused.
    assert_eq!(
        uses,
        vec![
            ("gh".into(), 1),
            ("ghb".into(), 0),
            ("ghx".into(), 1),
            ("web.search".into(), 0)
        ],
        "{h}"
    );

    // No `op` ran for any of it, and no value is anywhere on disk.
    assert_eq!(r.op_calls("github_token"), ops, "a grant never calls op");
    assert_eq!(r.op_calls("approve_token"), ops);
    let state = holding(&r.path("state"), MARK.as_bytes());
    assert!(state.is_empty(), "values in the store or spool: {state:?}");
    let log = holding(&r.path("theseusd.log"), MARK.as_bytes());
    assert!(log.is_empty(), "a value in the log");
    assert!(
        !holding(&r.path("state"), b"GH_TOKEN").is_empty(),
        "the scan reads the store: the variable's name is there"
    );
}
