//! A job's handle with the real `theseusd` and real job wrappers
//! (theseus-n8gk): a job `proc_run` started in the background is read with
//! `job_read` and waited for with `job_wait`, whose result carries what it
//! printed; and a run past its window, named by the handle its answer gave,
//! is stopped with `job_stop`, which leaves no process of its tree. The
//! stand-in model ends each turn once it has a tool's result, so each step is
//! a turn of its own.

mod common;

use common::{table, Fixture, FAKE_OP};

use std::path::PathBuf;
use std::process::Stdio;

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// A tree: a shell that writes its pid and its child's, a `sleep` it waits
/// on, gone by itself within `secs`.
fn tree(secs: u32) -> Vec<String> {
    vec![
        "bash".into(),
        "-c".into(),
        format!("echo $$ > tree.pid; sleep {secs} & echo $! > child.pid; wait"),
    ]
}

/// Six digits for a run, so no other process on the machine matches what a
/// test's jobs name.
fn nonce() -> &'static str {
    static N: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    N.get_or_init(|| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap();
        format!("{:06}", (t.subsec_nanos() ^ std::process::id()) % 1_000_000)
    })
}

/// A job whose own `pkill -f` pattern matches its sleeper and, before
/// theseus-53ujg.2, its shell and its wrapper.
fn own_pkill() -> Vec<String> {
    let n = nonce();
    vec![
        "bash".into(),
        "-c".into(),
        format!("sleep 30.{n} & pkill -f 'sleep 30.{n}'; wait; echo survived"),
    ]
}

/// Two commands, so a shell stays; the token is on no command line.
fn token_line() -> String {
    format!("echo $$ > tree.pid; sleep 5; true # token-{}", nonce())
}

struct Rig {
    dir: tempfile::TempDir,
    model: FakeModel,
}

impl Fixture for Rig {
    fn dir(&self) -> &std::path::Path {
        self.dir.path()
    }
}

impl Rig {
    fn new(sync_secs: i64) -> Self {
        let model = FakeModel::start(|prompt| match prompt {
            "Start it" => vec![(
                "proc_run",
                json!({"argv": ["sh", "-c", "sleep 20; printf 'do%s\\n' ne"], "background": true}),
            )],
            "Read j1" => vec![("job_read", json!({"job": "j1"}))],
            "Wait for j1" => vec![("job_wait", json!({"job": "j1"}))],
            "Run the long one" => vec![("proc_run", json!({"argv": tree(90)}))],
            "Stop j1" => vec![("job_stop", json!({"job": "j1"}))],
            "Kill the sleeper" => vec![("proc_run", json!({"argv": own_pkill()}))],
            "Show no text" => vec![(
                "proc_run",
                json!({"argv": ["bash", "-c", token_line()], "background": true}),
            )],
            "Signal the wrapper" => vec![(
                "proc_run",
                json!({"argv": ["bash", "-c", "kill -TERM \"$PPID\"; sleep 30; echo unreachable"]}),
            )],
            _ => vec![],
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
        table(&mut t, "model").insert("api_base".into(), model.base.clone().into());
        for (_, p) in table(&mut t, "providers").iter_mut() {
            p.as_table_mut()
                .unwrap()
                .insert("api_base".into(), model.base.clone().into());
        }
        table(&mut t, "policy").insert("enforcement".into(), "notify".into());
        table(&mut t, "tools").insert("proc_sync_secs".into(), sync_secs.into());
        std::fs::write(path("config.toml"), toml::to_string(&t).unwrap()).unwrap();
        Self { dir, model }
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
        self.wait("the socket", || self.call("health", Value::Null).ok());
        d
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, Value> {
        common::rpc::call_within(&self.sock(), 120, method, params)
    }

    /// A turn with `input`, in `session` (a new one when none): its session.
    fn turn(&self, session: Option<&str>, input: &str) -> String {
        let mut p = json!({"input": input, "author": "test", "attachments": []});
        if let Some(s) = session {
            p["session_id"] = json!(s);
        }
        let res = self.call("turn.submit", p).unwrap();
        res["session_id"].as_str().unwrap().to_string()
    }

    /// The text of the last tool result the stand-in was sent.
    fn last_result(&self) -> String {
        let requests = self.model.requests();
        for r in requests.iter().rev() {
            let Some(last) = r["messages"].as_array().and_then(|m| m.last()) else {
                continue;
            };
            for b in last["content"].as_array().into_iter().flatten() {
                if b["type"] == "tool_result" {
                    return match &b["content"] {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts
                            .iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join(""),
                        other => other.to_string(),
                    };
                }
            }
        }
        panic!("no tool result was sent")
    }

    /// A pid the job wrote to `file` in the projects dir.
    fn pid(&self, file: &str) -> Option<u32> {
        std::fs::read_to_string(self.path("projects").join(file))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

/// Running or sleeping: neither gone nor a zombie.
fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|s| s[s.rfind(')')? + 2..].chars().next())
        .is_some_and(|c| c != 'Z' && c != 'X')
}

/// Every process of a job whose text holds `marker`. The text is on no
/// command line (theseus-53ujg.2): a running wrapper is found by its job's
/// argv file in `spool`, and a job's shell by the runner's variable in its
/// environment.
fn holding(spool: &std::path::Path, marker: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").unwrap().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else {
            continue;
        };
        if !alive(pid) {
            continue;
        }
        let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let words: Vec<String> = cmd
            .split(|&b| b == 0)
            .map(|w| String::from_utf8_lossy(w).into_owned())
            .collect();
        let argv = words
            .iter()
            .position(|w| w == "--correlation-id")
            .and_then(|i| words.get(i + 1))
            .and_then(|id| std::fs::read_to_string(spool.join("argv").join(id)).ok())
            .unwrap_or_default();
        let env = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
        let shell = env
            .split(|&b| b == 0)
            .map(String::from_utf8_lossy)
            .find(|v| v.starts_with("THESEUS_JOB_LINE="))
            .unwrap_or_default()
            .into_owned();
        if argv.contains(marker) || shell.contains(marker) {
            out.push((pid, words.join(" ")));
        }
    }
    out
}

/// `proc_run {background: true}` answers at once (the job still runs at the
/// next turn's read); `job_read` shows the job running; `job_wait` returns
/// its result, which the request after it carries.
#[test]
fn a_background_job_is_read_and_waited_for() {
    let r = Rig::new(60);
    let _daemon = r.spawn();
    let sid = r.turn(None, "Start it");
    let started = r.last_result();
    assert!(started.starts_with("Started job j1 ("), "{started}");
    r.turn(Some(&sid), "Read j1");
    let read = r.last_result();
    assert!(read.starts_with("Job j1 (sh -c sleep 20; "), "{read}");
    assert!(read.contains("), running "), "{read}");
    r.turn(Some(&sid), "Wait for j1");
    let waited = r.last_result();
    // Its first line names where it ran (theseus-9dt2).
    assert_eq!(
        waited
            .split_once("]\n")
            .map(|(head, rest)| (head.starts_with("[exit code 0 · in /"), rest)),
        Some((true, "done\n")),
        "{waited}\n{}",
        r.log()
    );
}

/// A run past its window answers with its handle; `job_stop` by that handle
/// stops its whole tree, verified, and nothing of it is left. A negative
/// assertion: when it fails it prints each process left.
#[test]
fn a_run_past_its_window_is_stopped_by_its_handle_and_nothing_is_left() {
    let r = Rig::new(1);
    let _daemon = r.spawn();
    let sid = r.turn(None, "Run the long one");
    let still = r.last_result();
    assert!(
        still.starts_with("Still running as job j1 after 1 s (timeout 600 s)."),
        "{still}"
    );
    let (shell, child) = r.wait("the tree's pids", || {
        Some((r.pid("tree.pid")?, r.pid("child.pid")?))
    });
    assert!(alive(shell) && alive(child));
    // The helper still finds the job while it runs: its shell by the runner's
    // variable, its wrapper by the argv file in the spool.
    let marker = "echo $$ > tree.pid; sleep 90";
    let running = holding(&r.path("state/spool"), marker);
    assert!(
        running.iter().any(|(p, _)| *p == shell)
            && running.iter().any(|(_, c)| c.contains("job-wrapper")),
        "the job is found while it runs: {running:?}"
    );
    r.turn(Some(&sid), "Stop j1");
    let stopped = r.last_result();
    let left = holding(&r.path("state/spool"), marker);
    assert!(
        stopped.starts_with("Stopped job j1 (bash -c echo $$ > tree.pid; sleep 90")
            && stopped.contains("): nothing of it is left.\n"),
        "{stopped}\nleft: {left:?}"
    );
    assert!(
        stopped.contains("[cancelled: stopped by a job_stop call"),
        "{stopped}"
    );
    assert!(
        !alive(shell) && !alive(child) && left.is_empty(),
        "the job's tree is gone: shell {shell} {}, sleep {child} {}, left {left:?}\n{stopped}",
        alive(shell),
        alive(child)
    );
}

/// theseus-53ujg.2: a command that `pkill -f`s its own sleeper does not kill
/// itself: its text is on no command line of its wrapper or its shell. Planted
/// back on the wrapper's line, or run as `bash -c <line>`, the pattern matches
/// the job's own process, the wrapper stops its tree, and the result is an
/// unknown outcome.
#[test]
fn a_jobs_own_pkill_f_does_not_kill_it() {
    let r = Rig::new(60);
    let _daemon = r.spawn();
    r.turn(None, "Kill the sleeper");
    let result = r.last_result();
    assert!(result.starts_with("[exit code 0"), "{result}");
    assert!(result.contains("survived"), "{result}");
}

/// While a job runs two commands, neither its wrapper's nor its shell's
/// command line shows its text, and the job is found through its spool.
#[test]
fn no_theseus_process_shows_a_jobs_text() {
    let r = Rig::new(60);
    let _daemon = r.spawn();
    r.turn(None, "Show no text");
    let shell = r.wait("the shell's pid", || r.pid("tree.pid"));
    let token = format!("token-{}", nonce());
    assert!(alive(shell));
    let shown: Vec<_> = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(&token))
        })
        .collect();
    assert!(
        shown.is_empty(),
        "command lines that show the text: {shown:?}"
    );
    // The shell runs the fixed runner, and the job is found by its spool.
    let cmd = std::fs::read(format!("/proc/{shell}/cmdline")).unwrap();
    assert!(
        String::from_utf8_lossy(&cmd).contains("unset THESEUS_JOB_LINE"),
        "{}",
        String::from_utf8_lossy(&cmd)
    );
    let found = holding(&r.path("state/spool"), &token);
    assert!(
        found.iter().any(|(p, _)| *p == shell)
            && found.iter().any(|(_, c)| c.contains("job-wrapper")),
        "{found:?}"
    );
}

/// A wrapper sent SIGTERM from outside before it reports leaves a result that
/// names the signal. A job's own `kill -TERM $PPID` is such a signal.
#[test]
fn an_unknown_outcome_names_the_signal() {
    let r = Rig::new(60);
    let _daemon = r.spawn();
    r.turn(None, "Signal the wrapper");
    let result = r.last_result();
    assert!(
        result.starts_with(
            "[outcome unknown: the job's wrapper was ended by SIGTERM before it reported]"
        ),
        "{result}"
    );
    assert!(!result.contains("unreachable"), "{result}");
}
