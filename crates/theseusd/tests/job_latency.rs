//! Tier 7.1, with the real `theseusd`: a synchronous `proc.run` hears of its
//! job's end as an event. The wrapper waits on its command's pidfd, spools
//! the completion, and pokes the notify socket; the harness's drain leaves
//! the completion to the turn that waits on the job, and wakes it; the turn
//! takes it with its result in one frame. So a quick command's result comes
//! within milliseconds of its end: not at a 50 ms poll's next look, and
//! never at the turn's 1 s backstop.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

struct Rig {
    dir: tempfile::TempDir,
    _model: FakeModel,
}

impl Rig {
    fn new() -> Self {
        let model = FakeModel::start(|prompt| {
            if prompt.contains("Run the quick one") {
                vec![("proc_run", json!({"argv": ["true"], "timeout_secs": 60}))]
            } else {
                vec![]
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let path = |p: &str| dir.path().join(p);
        let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
        for d in ["bin", "projects", "state"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path("bin/op"), FAKE_OP).unwrap();
        std::fs::set_permissions(path("bin/op"), std::fs::Permissions::from_mode(0o755)).unwrap();
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
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, _model: model }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn spawn(&self) -> Daemon {
        let log = std::fs::File::create(self.path("theseusd.log")).unwrap();
        let d = Daemon::spawn(
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
        let t0 = Instant::now();
        while self.call("health", Value::Null).is_err() {
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no socket in 40 s; the daemon's log:\n{}",
                std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        d
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
}

/// Five turns that each run `true`: every result's wait, from the launch to
/// the result the turn took, is under the turn's 1 s backstop, so each came
/// by the drain's word. The turn looks at the launch and then waits for the
/// word, so a result the backstop brought comes a second or more after the
/// launch: each came well before. A word is quick (tens of ms alone), but
/// one wait in a loaded suite once took 642 ms (the gate, 2026-10-03 18:26),
/// so the bound on each is the backstop's, and the bound on the median is
/// the word's.
#[test]
fn a_quick_jobs_result_comes_by_the_drains_word_not_a_poll() {
    let r = Rig::new();
    let _daemon = r.spawn();
    let mut session = Value::Null;
    for i in 0..5 {
        let res = r
            .call(
                "turn.submit",
                json!({"input": format!("Run the quick one, {i}"), "author": "test",
                    "attachments": [], "session_id": session}),
            )
            .unwrap();
        session = res["session_id"].clone();
    }
    let h = r
        .call("session.history", json!({"session_id": session}))
        .unwrap();
    let nodes = h["nodes"].as_array().unwrap();
    let waits: Vec<u64> = nodes
        .iter()
        .filter(|n| n["kind"] == "tool_result" && n["detail"]["tool"] == "proc.run")
        .map(|n| n["detail"]["duration_ms"].as_u64().unwrap())
        .collect();
    eprintln!("each job's wait, launch to result: {waits:?} ms");
    assert_eq!(waits.len(), 5, "of {} nodes", nodes.len());
    assert!(
        waits.iter().all(|w| *w < 900),
        "a result waited for the backstop, not the drain's word: {waits:?} ms"
    );
    let mut sorted = waits.clone();
    sorted.sort_unstable();
    assert!(
        sorted[2] < 200,
        "the drain's word was slow: median {} ms of {waits:?}",
        sorted[2]
    );
}
