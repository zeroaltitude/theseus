//! Task sessions with the real `theseusd` (DD7, theseus-qn2): its Discord
//! binding talks to a stand-in for Discord's REST API
//! (`theseus_sim::fake_discord`), its gateway to a port nothing listens on,
//! and its model to a stand-in for the Messages API. Nothing reaches Discord.
//! - `kill -9` while a task's job runs, then a restart: the job's result
//!   arrives, the task finishes, and its report posts once;
//! - a cancel stops the task and kills its job's wrapper, and reports once;
//! - a task with `wake_parent` starts its parent's turn, whose reply posts
//!   under the report's line (W1, theseus-lji);
//! - `execution.stop` (`/stop`) kills the session's job and keeps the session,
//!   whose next message continues it (W1).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};
use theseus_sim::fake_discord::{FakeDiscord, Msg};

const USER: u64 = 271_828_182_845_904_523;
/// The fake's DM channel with `USER`.
const DM: u64 = USER + 1;

/// What the stand-in model calls for a prompt that contains a phrase.
type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

/// A fake `op`: every reference's value is `tv-<its item>`.
const FAKE_OP: &str = "#!/bin/sh\n\
    case \"$1\" in\n\
    \x20 inject) sed -E 's#\\{\\{ op://Test/([^/]+)/[^}]* \\}\\}#tv-\\1#g' ;;\n\
    \x20 read) for a; do ref=\"$a\"; done; printf 'tv-%s' \"$(echo \"$ref\" | cut -d/ -f4)\" ;;\n\
    \x20 *) exit 1 ;;\n\
    esac\n";

struct Rig {
    dir: tempfile::TempDir,
    fake: Arc<FakeDiscord>,
    _model: FakeModel,
}

impl Rig {
    /// `script`: a phrase the last user message contains, and the tool calls
    /// the model makes for it. A call that answers a tool call gets `Done.`.
    fn new(script: Vec<(&str, Vec<(&'static str, Value)>)>) -> Self {
        Self::with(script, |_| {})
    }

    /// `new`, with a last change to the config.
    fn with(
        script: Vec<(&str, Vec<(&'static str, Value)>)>,
        tweak: impl FnOnce(&mut toml::Table),
    ) -> Self {
        let script: Script = Arc::new(Mutex::new(
            script
                .into_iter()
                .map(|(p, c)| (p.to_string(), c))
                .collect(),
        ));
        let model = FakeModel::start(move |prompt| {
            script
                .lock()
                .unwrap()
                .iter()
                .find(|(p, _)| prompt.contains(p.as_str()))
                .map(|(_, calls)| calls.clone())
                .unwrap_or_default()
        });
        let fake = FakeDiscord::start();
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
        table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
        let discord = table(&mut t, "discord");
        discord.insert("enabled".into(), true.into());
        discord.insert("rest_proxy".into(), fake.addr.clone().into());
        discord.insert("gateway_proxy".into(), "ws://127.0.0.1:9".into());
        tweak(&mut t);
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        std::fs::write(
            path("state/bindings.toml"),
            format!(
                "guild_id = \"314159265358979323\"\n[[dm]]\nuser = \"{USER}\"\nname = \"eddie\"\n"
            ),
        )
        .unwrap();
        Self {
            dir,
            fake,
            _model: model,
        }
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().join(p)
    }

    fn spawn(&self) -> Daemon {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path("theseusd.log"))
            .unwrap();
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
        self.wait("the socket", || self.call("health", Value::Null).ok());
        d
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default()
    }

    fn wait<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let t0 = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(40),
                "no {what} in 40 s; the daemon's log ends:\n{}",
                tail(&self.log(), 30)
            );
            std::thread::sleep(Duration::from_millis(20));
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

    /// The DM place's session, once the binding has bound it.
    fn session(&self) -> String {
        self.wait("the DM place bound", || {
            let h = self.call("health", Value::Null).ok()?;
            h["bindings"][0]["places"][0]["session_id"]
                .as_str()
                .map(str::to_string)
        })
    }

    fn ask(&self, sid: &str, prompt: &str) -> Value {
        self.call(
            "turn.submit",
            json!({"session_id": sid, "input": prompt, "author": "test", "attachments": []}),
        )
        .unwrap()
    }

    fn tasks(&self) -> Vec<Value> {
        self.call("task.list", json!({})).unwrap()["tasks"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    /// The DM's task reports, as the fake keeps them.
    fn reports(&self) -> Vec<Msg> {
        self.fake
            .messages(DM)
            .into_iter()
            .filter(|m| m.content.contains("**Task `"))
            .collect()
    }

    /// The report nodes in a session.
    fn report_nodes(&self, sid: &str) -> Vec<Value> {
        let h = self
            .call("session.history", json!({"session_id": sid}))
            .unwrap();
        h["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["author"].as_str().is_some_and(|a| a.starts_with("task:")))
            .cloned()
            .collect()
    }
}

fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// `kill -9` while the task's job runs in the background: the job outlives
/// the daemon, the restarted daemon takes its result, the task's next turn
/// finishes it, and its report posts once, on the fake REST. The parent's
/// next turn then reads it, once.
#[test]
fn a_kill_mid_task_then_a_restart_finishes_it_and_reports_once() {
    let r = Rig::new(vec![
        (
            "Start the probe",
            vec![(
                "task_create",
                json!({"brief": "Run the probe job and report."}),
            )],
        ),
        (
            "Run the probe job",
            vec![(
                "proc_run",
                json!({"argv": ["sh", "-c", "sleep 3; echo probe-done"], "timeout_secs": 30}),
            )],
        ),
    ]);
    let daemon = r.spawn();
    let sid = r.session();
    let first = r.ask(&sid, "Start the probe");
    assert_eq!(first["output"], "Done.", "{first}");
    let task = r.wait("the task waiting on its job", || {
        r.tasks().into_iter().find(|t| t["waiting_on"] == "a job")
    });
    // SIGKILL, with the job running under its detached wrapper.
    drop(daemon);
    assert!(r.reports().is_empty(), "no report before the task finished");
    let _daemon = r.spawn();
    r.wait("the task complete", || {
        r.tasks()
            .into_iter()
            .find(|t| t["task_id"] == task["task_id"] && t["state"] == "complete")
    });
    let got = r.wait("the report delivered", || {
        let got = r.reports();
        (!got.is_empty()).then_some(got)
    });
    // Give a second copy, were there one, the time to arrive.
    std::thread::sleep(Duration::from_millis(500));
    let got_again = r.reports();
    assert_eq!(got_again.len(), 1, "one report: {got_again:?}");
    let short = task["short"].as_str().unwrap();
    assert!(
        got[0]
            .content
            .starts_with(&format!("📋 **Task `{short}` finished**")),
        "{}",
        got[0].content
    );
    // Its last message: the job's result came back after the restart as a
    // late result, which the stand-in model answers so.
    assert!(
        got[0].content.contains("\nNothing to do.\n-# 2 turns"),
        "{}",
        got[0].content
    );
    // The parent's next turn reads it, once.
    assert!(r.report_nodes(&sid).is_empty());
    r.ask(&sid, "What did the task report?");
    assert_eq!(r.report_nodes(&sid).len(), 1);
    r.ask(&sid, "Anything else?");
    assert_eq!(r.report_nodes(&sid).len(), 1, "read once");
}

/// A cancel stops the task and its job: the job's wrapper is terminated, its
/// action settles cancelled with termination verified, and the place hears it
/// once; a second cancel says nothing more.
#[test]
fn a_cancel_stops_the_task_and_its_job_and_reports_once() {
    let r = Rig::new(vec![
        (
            "Start the long job",
            vec![("task_create", json!({"brief": "Run the long job."}))],
        ),
        (
            "Run the long job",
            vec![(
                "proc_run",
                json!({"argv": ["sleep", "30"], "timeout_secs": 60}),
            )],
        ),
    ]);
    let _daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Start the long job");
    let task = r.wait("the task waiting on its job", || {
        r.tasks().into_iter().find(|t| t["waiting_on"] == "a job")
    });
    let short = task["short"].as_str().unwrap().to_string();
    let res = r
        .call("task.cancel", json!({"task": short, "author": "test"}))
        .unwrap();
    assert_eq!(res["task"]["state"], "cancelled", "{res}");
    assert_eq!(
        res["cancelled_actions"].as_array().unwrap().len(),
        1,
        "{res}"
    );
    let job = res["cancelled_actions"][0].as_str().unwrap().to_string();
    let actions = r
        .call("action.list", json!({"execution_id": task["execution_id"]}))
        .unwrap();
    let a = actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["correlation_id"] == job.as_str())
        .cloned()
        .unwrap();
    assert_eq!(a["state"], "cancelled", "{a}");
    assert_eq!(a["cancel"], "termination_verified", "{a}");
    let got = r.wait("the report delivered", || {
        let got = r.reports();
        (!got.is_empty()).then_some(got)
    });
    assert!(
        got[0]
            .content
            .starts_with(&format!("⏹️ **Task `{short}` stopped**")),
        "{}",
        got[0].content
    );
    assert!(
        got[0].content.contains("cancelled by test"),
        "{}",
        got[0].content
    );
    let again = r
        .call("task.cancel", json!({"task": short, "author": "test"}))
        .unwrap();
    assert!(again["cancelled_actions"].as_array().unwrap().is_empty());
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(r.reports().len(), 1, "reported once");
}

/// A session's executions, by the list's rows (W1).
fn execution_of(r: &Rig, sid: &str) -> Value {
    r.call("execution.list", Value::Null).unwrap()["executions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["session_id"] == sid)
        .cloned()
        .unwrap()
}

/// A task with `wake_parent` (W1): its report starts the parent's turn by
/// itself, and that turn's reply reaches the fake once, under the report's
/// line, after the report.
#[test]
fn a_task_with_wake_parent_starts_its_parents_turn_and_the_reply_posts_under_its_line() {
    let r = Rig::new(vec![(
        "Start the chain",
        vec![(
            "task_create",
            json!({"brief": "Say one word.", "wake_parent": true}),
        )],
    )]);
    let _daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Start the chain");
    let task = r.wait("the task complete", || {
        r.tasks().into_iter().find(|t| t["state"] == "complete")
    });
    assert_eq!(task["wake_parent"], true, "{task}");
    let short = task["short"].as_str().unwrap().to_string();
    let line = format!("-# 📋 task {short} reported\n");
    let reply = r.wait("the report's turn's reply", || {
        r.fake
            .messages(DM)
            .into_iter()
            .find(|m| m.content.starts_with(&line))
    });
    assert!(
        reply.content.contains("Nothing to do."),
        "{}",
        reply.content
    );
    let exec = r.wait("the parent's second turn ended", || {
        let e = execution_of(&r, &sid);
        (e["turns"] == 2 && e["state"] == "waiting").then_some(e)
    });
    assert_eq!(exec["turns"], 2);
    assert_eq!(r.report_nodes(&sid).len(), 1, "its input was the report");
    // The report came first, then the reply, once each.
    std::thread::sleep(Duration::from_millis(500));
    let msgs = r.fake.messages(DM);
    let at = |f: &dyn Fn(&Msg) -> bool| msgs.iter().position(f).unwrap();
    assert!(
        at(&|m| m.content.contains(&format!("**Task `{short}` finished**")))
            < at(&|m| m.content.starts_with(&line))
    );
    assert_eq!(
        msgs.iter().filter(|m| m.content.starts_with(&line)).count(),
        1
    );
    assert_eq!(r.reports().len(), 1);
}

/// `execution.stop` (`/stop`, W1) kills the job the session waits on, keeps
/// the session, and its next message continues it: the same execution, and
/// the job's result reads as stopped.
#[test]
fn a_stop_kills_the_sessions_job_and_the_next_message_continues_the_session() {
    let r = Rig::new(vec![(
        "Run the long build",
        vec![(
            "proc_run",
            json!({"argv": ["sleep", "30"], "timeout_secs": 60}),
        )],
    )]);
    let _daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Run the long build");
    let exec = r.wait("the session waiting on its job", || {
        let e = execution_of(&r, &sid);
        (e["state"] == "waiting" && e["outstanding"] == 1).then_some(e)
    });
    let eid = exec["execution_id"].as_str().unwrap().to_string();
    let res = r
        .call(
            "execution.stop",
            json!({"execution_id": eid, "author": "test"}),
        )
        .unwrap();
    assert_eq!(res["stopped"], true, "{res}");
    assert_eq!(res["execution"]["state"], "waiting", "{res}");
    let job = res["stopped_actions"][0].as_str().unwrap().to_string();
    let a = r.call("action.list", json!({"execution_id": eid})).unwrap()["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["correlation_id"] == job.as_str())
        .cloned()
        .unwrap();
    assert_eq!(a["state"], "cancelled", "{a}");
    assert_eq!(a["cancel"], "termination_verified", "{a}");
    // The next message continues the same session, and reads the job as stopped.
    let next = r.ask(&sid, "What happened to the build?");
    assert_eq!(next["session_id"], sid.as_str(), "{next}");
    assert_eq!(next["execution_id"], eid.as_str(), "{next}");
    let h = r
        .call("session.history", json!({"session_id": sid}))
        .unwrap();
    let nodes = h["nodes"].as_array().unwrap();
    assert!(
        nodes.iter().any(|n| n["text"]
            .as_str()
            .is_some_and(|t| t.contains("[cancelled: stopped by test]"))),
        "the job's late result says it was stopped: {}",
        serde_json::to_string(&nodes).unwrap()
    );
    // And says who stopped it, for the surfaces' `⏹️ stopped by` line
    // (theseus-4uw): the result, not only its text.
    let late = nodes
        .iter()
        .find(|n| n["detail"]["late"] == true)
        .expect("the job's late result");
    assert_eq!(
        (
            late["detail"]["status"].as_str(),
            late["detail"]["meta"]["stopped_by"].as_str()
        ),
        (Some("cancelled"), Some("test")),
        "{late}"
    );
    assert_eq!(execution_of(&r, &sid)["turns"], 2);
}

/// theseus-ewev: a job stopped before its completion has no `result_ref`, so
/// its raw output waited for the spool's sweep after its cancelled result
/// was written. Now the turn that writes that result deletes the file, with
/// no restart, and the result shows what the job printed before the stop.
/// theseus-gsn9: the job printed past its cap's head, and the stop killed
/// its wrapper with the end in its ring: the file holds the head, and the
/// result says the end was lost.
#[test]
fn a_stopped_jobs_raw_output_goes_with_its_cancelled_result() {
    let r = Rig::with(
        vec![(
            "Run the noisy build",
            vec![(
                "proc_run",
                json!({"argv": ["bash", "-c", "head -c 100000 /dev/zero | tr '\\0' x; echo; echo still building; sleep 30"], "timeout_secs": 60}),
            )],
        )],
        |t| {
            t.get_mut("tools")
                .and_then(toml::Value::as_table_mut)
                .unwrap()
                .insert("job_output_max_bytes".into(), 65_536.into());
        },
    );
    let _daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Run the noisy build");
    let exec = r.wait("the session waiting on its job", || {
        let e = execution_of(&r, &sid);
        (e["state"] == "waiting" && e["outstanding"] == 1).then_some(e)
    });
    let eid = exec["execution_id"].as_str().unwrap().to_string();
    let res = r
        .call(
            "execution.stop",
            json!({"execution_id": eid, "author": "test"}),
        )
        .unwrap();
    assert_eq!(res["stopped"], true, "{res}");
    let job = res["stopped_actions"][0].as_str().unwrap().to_string();
    let raw = r.path(&format!("state/spool/results/{job}.out"));
    // The head of a 65,536-byte cap, and no more: the end was in the ring.
    let head = 65_536 - 32_768 - 128;
    assert_eq!(
        std::fs::metadata(&raw).unwrap().len(),
        head,
        "{}",
        raw.display()
    );

    r.ask(&sid, "What happened to the build?");
    assert!(
        !raw.exists(),
        "the stopped job's raw output went with its cancelled result, with no restart"
    );
    let h = r
        .call("session.history", json!({"session_id": sid}))
        .unwrap();
    let late = h["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["detail"]["late"] == true)
        .cloned()
        .expect("the job's late result");
    let text = late["text"].as_str().unwrap();
    assert!(
        text.starts_with(
            "[cancelled: stopped by test]\n[its output reached the first 32,640 bytes, all the \
             file takes before the end, and its end was lost: the job's wrapper was killed \
             before it could write it]\nxxxx"
        ),
        "{text}"
    );
}
