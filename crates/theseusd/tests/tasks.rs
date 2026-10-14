//! Task sessions with the real `theseusd` (DD7, theseus-qn2): its Discord
//! binding talks to a stand-in for Discord's REST API
//! (`theseus_sim::fake_discord`), its gateway to a port nothing listens on,
//! and its model to a stand-in for the Messages API. Nothing reaches Discord.
//! - `kill -9` while a task's job runs, then a restart: the job's result
//!   arrives, the task finishes, and its report posts once;
//! - `kill -9` while a task is parked on its own wake, then a restart: the
//!   wake fires once, and the task reports once (37b, theseus-7kg);
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
    model: FakeModel,
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
                "guild_id = \"314159265358979323\"\n[[dm]]\nuser = \"{USER}\"\nname = \"zeroaltitude\"\n"
            ),
        )
        .unwrap();
        Self { dir, fake, model }
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
                // These tests submit to the DM place's session by hand, before
                // any message: a debug build's plant opens it at the bind, as
                // before theseus-emqx.
                .env("THESEUS_TEST_OPEN_AT_BIND", "1")
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
            "Run the probe job",
            vec![(
                "proc_run",
                json!({"argv": ["sh", "-c", "sleep 3; echo probe-done"], "timeout_secs": 30}),
            )],
        ),
        (
            "Start the probe",
            vec![(
                "task_create",
                json!({"brief": "Run the probe job and report.", "arrangement":
                       {"pieces": [{"quote": "Start the probe as a task", "role": "objective"}]}}),
            )],
        ),
    ]);
    let daemon = r.spawn();
    let sid = r.session();
    let first = r.ask(&sid, "Start the probe as a task");
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

/// `kill -9` while a task is parked on its own one-shot wake (37b,
/// theseus-7kg): the restarted daemon fires the wake once, its turn continues
/// the task, and the task reports once, after the wake's turn. While it was
/// parked, `wake.list` named the task, and `task.list` said it waits on a
/// wake.
#[test]
fn a_kill_while_a_task_is_parked_on_its_wake_then_a_restart_reports_once() {
    let r = Rig::new(vec![
        (
            "Check the build now and again",
            vec![(
                "wake_at",
                json!({"after": "4s", "note": "Look at the build once more, then report."}),
            )],
        ),
        (
            "Watch the build",
            vec![(
                "task_create",
                json!({"brief": "Check the build now and again shortly, then report.", "arrangement":
                       {"pieces": [{"quote": "Watch the build as a task", "role": "objective"}]}}),
            )],
        ),
    ]);
    let daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Watch the build as a task");
    let task = r.wait("the task parked on its wake", || {
        r.tasks().into_iter().find(|t| t["waiting_on"] == "a wake")
    });
    let short = task["short"].as_str().unwrap().to_string();
    let wakes = r.call("wake.list", json!({})).unwrap()["wakes"].clone();
    assert_eq!(wakes.as_array().unwrap().len(), 1, "{wakes}");
    assert_eq!(wakes[0]["task"], short.as_str(), "{wakes}");
    assert!(r.reports().is_empty(), "a parked task has not reported");
    // SIGKILL, with the wake pending.
    drop(daemon);
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
    std::thread::sleep(Duration::from_millis(1_000));
    assert_eq!(r.reports().len(), 1, "one report: {:?}", r.reports());
    assert!(
        got[0]
            .content
            .starts_with(&format!("📋 **Task `{short}` finished**")),
        "{}",
        got[0].content
    );
    assert!(got[0].content.contains("-# 2 turns"), "{}", got[0].content);
    let wakes = r.call("wake.list", json!({})).unwrap()["wakes"].clone();
    assert!(wakes.as_array().unwrap().is_empty(), "{wakes}");
    // The wake's turn ran once: one request ended on its line (the set
    // call's result quotes the line too, as a tool result).
    let fired = r
        .model
        .requests()
        .iter()
        .filter_map(|q| q["messages"].as_array().and_then(|m| m.last()).cloned())
        .map(|m| m.to_string())
        .filter(|m| m.contains("⏰ wake") && !m.contains("tool_result"))
        .count();
    assert_eq!(fired, 1, "the wake's turn ran once");
}

/// A cancel stops the task and its job: the job's wrapper is terminated, its
/// action settles cancelled with termination verified, and the place hears it
/// once; a second cancel says nothing more.
#[test]
fn a_cancel_stops_the_task_and_its_job_and_reports_once() {
    let r = Rig::new(vec![
        (
            "Run the long job",
            vec![(
                "proc_run",
                json!({"argv": ["sleep", "30"], "timeout_secs": 60}),
            )],
        ),
        (
            "Start the long job",
            vec![(
                "task_create",
                json!({"brief": "Run the long job.", "arrangement":
                       {"pieces": [{"quote": "Start the long job as a task", "role": "objective"}]}}),
            )],
        ),
    ]);
    let _daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Start the long job as a task");
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
    let r = Rig::new(vec![
        // The task's own prompt quotes its parent's: its phrase comes first.
        ("Say one word", vec![]),
        (
            "Start the chain",
            vec![(
                "task_create",
                json!({"brief": "Say one word.", "wake_parent": true, "arrangement":
                       {"pieces": [{"quote": "Start the chain of tasks", "role": "objective"}]}}),
            )],
        ),
    ]);
    let _daemon = r.spawn();
    let sid = r.session();
    r.ask(&sid, "Start the chain of tasks");
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
        nodes.iter().any(|n| n["text"].as_str().is_some_and(
            |t| t.contains("[cancelled: stopped by test; verified: process tree, 1 process]")
        )),
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
            late["detail"]["meta"]["stopped_by"].as_str(),
            late["detail"]["meta"]["verified"].as_str()
        ),
        (
            Some("cancelled"),
            Some("test"),
            Some("verified: process tree, 1 process")
        ),
        "{late}"
    );
    // The stop answered with the job's verdict (M4 18a).
    assert_eq!(res["verdicts"][0]["verified_by"], "tree", "{res}");
    assert_eq!(execution_of(&r, &sid)["turns"], 2);
}

/// theseus-ewev: a job stopped before its completion has no `result_ref`, so
/// its raw output waited for the spool's sweep after its cancelled result
/// was written. Now the turn that writes that result deletes the file, with
/// no restart, and the result shows what the job printed before the stop.
/// The job printed past its cap's head (theseus-gsn9). Since 18a the stop
/// ends the job's tree and not its wrapper, which writes the end its ring
/// held once the last writer is gone: the file keeps both ends, and the
/// result says how the stop was verified, and loses nothing.
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
    // Both ends of a 65,536-byte cap: the head, the marker, and the end.
    let head = 65_536 - 32_768 - 128;
    let kept = std::fs::read(&raw).unwrap();
    assert!(
        kept.len() as u64 > head,
        "{}: {}",
        raw.display(),
        kept.len()
    );
    assert!(kept.ends_with(b"still building\n"), "the end was kept");

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
        text.starts_with("[cancelled: stopped by test; verified: process tree, "),
        "{text}"
    );
    assert!(!text.contains("its end was lost"), "{text}");
}

/// A deferred task (theseus-rcgph) with the real daemon: opened asleep, it is
/// listed with its start and takes no turn; a `kill -9` and a restart keep it,
/// and it starts once, with the note that says so, and reports once.
#[test]
fn a_deferred_task_survives_a_kill_and_a_restart_and_starts_once() {
    let r = Rig::new(vec![(
        "Defer the notes",
        vec![(
            "task_create",
            json!({"brief": "Draft the release notes and report.", "start_after": "6s",
                   "arrangement": {"pieces": [{"quote": "Defer the notes as a task", "role": "objective"}]}}),
        )],
    )]);
    let daemon = r.spawn();
    let sid = r.session();
    let first = r.ask(&sid, "Defer the notes as a task");
    assert_eq!(first["output"], "Done.", "{first}");
    let task = r.wait("the sleeping task", || {
        r.tasks().into_iter().find(|t| t["starts_at_ms"].is_u64())
    });
    assert_eq!(task["state"], "waiting", "{task}");
    assert_eq!(task["turns"], 0, "no turn before it is due: {task}");
    assert!(
        task["attention"]["label"]
            .as_str()
            .unwrap()
            .starts_with("▷ starts "),
        "{task}"
    );
    // The daemon goes while it sleeps, and comes back.
    drop(daemon);
    assert!(r.reports().is_empty(), "no report before it started");
    let _daemon = r.spawn();
    let kept = r
        .tasks()
        .into_iter()
        .find(|t| t["task_id"] == task["task_id"]);
    assert_eq!(
        kept.as_ref().map(|t| &t["starts_at_ms"]),
        Some(&task["starts_at_ms"]),
        "the restart kept its start"
    );
    r.wait("the task complete", || {
        r.tasks()
            .into_iter()
            .find(|t| t["task_id"] == task["task_id"] && t["state"] == "complete")
    });
    r.wait("the report delivered", || {
        let got = r.reports();
        (!got.is_empty()).then_some(got)
    });
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(r.reports().len(), 1, "one report");
    let h = r
        .call(
            "session.history",
            json!({"session_id": task["task_id"].as_str().unwrap()}),
        )
        .unwrap();
    let notes: Vec<&Value> = h["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|n| n["author"] == "harness:deferred")
        .collect();
    assert_eq!(notes.len(), 1, "the start's note, once: {h}");
    assert!(
        notes[0].to_string().contains("Deferred task, set "),
        "{}",
        notes[0]
    );
}

/// The stand-in's rules for the forecast scenario: thirty counting tasks,
/// a slow one, and the rule that starts them.
fn forecast_script() -> Vec<(String, Vec<(&'static str, Value)>)> {
    let mut script: Vec<(String, Vec<(&'static str, Value)>)> = (0..30)
        .map(|i| {
            (
                format!("counting task number {i} please"),
                vec![(
                    "task_create",
                    json!({"brief": format!("Count the buoys, batch {i}."), "arrangement":
                           {"pieces": [{"quote": format!("Start the counting task number {i} please"),
                                        "role": "objective"}]}}),
                )],
            )
        })
        .collect();
    // The slow rule comes first and keys on a word only the task's brief has:
    // the task reads its parent's message whole too, and that starts a task.
    script.insert(
        0,
        (
            "then sleep a while".into(),
            vec![(
                "proc_run",
                json!({"argv": ["sh", "-c", "sleep 5; echo slow-done"], "timeout_secs": 60}),
            )],
        ),
    );
    script.push((
        "Kick off the task".into(),
        vec![(
            "task_create",
            json!({"brief": "Count the buoys, then sleep a while.", "arrangement":
                   {"pieces": [{"quote": "wait it out, please, then report", "role": "objective"}]}}),
        )],
    ));
    script
}

/// A task's forecast, live (theseus-zz752): thirty finished tasks, a restart
/// (the table is read from the store after serving), then a new task whose
/// first line carries the forecast, and a slow one that runs past the p90 of
/// minutes and is told once to a `work.watch` connection.
#[test]
fn a_new_task_has_a_forecast_and_a_slow_one_is_told_once() {
    let script = forecast_script();
    // A command waits in the turn for up to 30 s, so the slow task is slow.
    let r = Rig::with(
        script
            .iter()
            .map(|(p, c)| (p.as_str(), c.clone()))
            .collect(),
        |t| {
            t.entry("tools")
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
                .insert("proc_sync_secs".into(), 30.into());
        },
    );
    let daemon = r.spawn();
    let sid = r.session();
    for i in 0..30 {
        r.ask(&sid, &format!("Start the counting task number {i} please"));
        r.wait("the task complete", || {
            let done = r
                .tasks()
                .iter()
                .filter(|t| t["state"] == "complete")
                .count();
            (done == i + 1).then_some(())
        });
    }
    // A restart: the table of finished tasks is read from the store.
    drop(daemon);
    let _daemon = r.spawn();
    let sid = r.session();
    r.wait("the finished tasks read", || {
        r.log()
            .contains("forecast: finished tasks read")
            .then_some(())
    });
    // Listen for notices first, as a terminal does.
    let sock = UnixStream::connect(r.path("sock")).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    (&sock)
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"work.watch\",\"params\":{\"interactive\":true}}\n")
        .unwrap();
    let mut reader = BufReader::new(&sock);
    r.ask(&sid, "Kick off the task: wait it out, please, then report");
    let h = r
        .call("session.history", json!({"session_id": sid}))
        .unwrap();
    let text = h.to_string();
    let at = text
        .rfind("⏳ task")
        .expect("the task's first line is in the conversation");
    let first_line = &text[at..text.len().min(at + 200)];
    assert!(
        first_line.contains("forecast ") && first_line.contains("from 25 like it"),
        "{first_line}"
    );
    eprintln!("first line: {}", first_line.split("\\n").next().unwrap());
    // The slow task runs past its p90 of minutes: one notice, however many
    // frames follow.
    let mut notices = Vec::new();
    let t0 = Instant::now();
    let mut line = String::new();
    let mut ended_at: Option<Instant> = None;
    while t0.elapsed() < Duration::from_secs(40) {
        // A read that times out leaves its partial line in `line`: it is
        // kept, and the next read finishes it.
        let _ = reader.read_line(&mut line);
        if line.ends_with('\n') {
            if line.contains("\"work.notice\"") {
                eprintln!("notice: {}", line.chars().take(300).collect::<String>());
            }
            if line.contains("\"work.notice\"") && line.contains("past_forecast") {
                notices.push(line.clone());
            }
            line.clear();
        }
        if ended_at.is_none()
            && !notices.is_empty()
            && r.tasks().iter().all(|t| t["state"] == "complete")
        {
            ended_at = Some(Instant::now());
        }
        // Two seconds past the task's end, for a second notice, were there one.
        if ended_at.is_some_and(|t| t.elapsed() > Duration::from_secs(2)) {
            break;
        }
    }
    if notices.len() != 1 {
        for q in r.model.requests().iter().rev().take(4) {
            let msgs = q["messages"].as_array().unwrap();
            let last = msgs.last().unwrap()["content"].to_string();
            eprintln!(
                "DEBUG request: {} messages, last: {}",
                msgs.len(),
                last.chars().take(300).collect::<String>()
            );
        }
        eprintln!("DEBUG tasks: {:?}", r.tasks().last());
    }
    assert_eq!(notices.len(), 1, "told once: {notices:?}");
    assert!(
        notices[0].contains("is past its forecast"),
        "{}",
        notices[0]
    );
}
