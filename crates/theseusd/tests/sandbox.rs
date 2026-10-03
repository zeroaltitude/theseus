//! L1 for `proc.run` (M4 17b), with the real `theseusd`, its real job
//! wrappers and L1 init, and a stand-in for the Messages API that asks for
//! the calls. Every job here runs in a real L1 sandbox, unprivileged:
//! - the probe after serving finds L1 working, and health and the ledger
//!   say so;
//! - a probe script with `sandbox: true` shows the contract (the operator's
//!   uid, no capabilities, an empty HOME, no route out, gh not logged in,
//!   the daemon's socket and store not there), and the same script at L0
//!   shows the contrast;
//! - `[sandbox] l1_argv` routes a call to L1;
//! - a job that cannot start in L1 fails with the reason, and never runs at
//!   L0;
//! - a program the broker grants a secret, asked to run in L1, gets none,
//!   and its result and the ledger say so.
//!
//! The rig's state directory and socket are inside its workspace root, so
//! the view hides them only because 17b hides them.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Daemon;
use serde_json::{json, Value};

/// The tool calls the stand-in model makes for each prompt, by its start.
type Script = Arc<Mutex<Vec<(String, Vec<(&'static str, Value)>)>>>;

struct Rig {
    dir: tempfile::TempDir,
    daemon: Daemon,
    script: Script,
    _model: FakeModel,
}

/// What a job reports, one `key=value` a line: `$1` is the daemon's socket,
/// `$2` its store (or nothing).
const PROBE: &str = r#"echo "uid=$(id -u)"
echo "cap=$(sed -n 's/^CapEff:[[:space:]]*//p' /proc/self/status)"
echo "home=$(ls -A "$HOME" | wc -l)"
echo "host=$(cat /proc/sys/kernel/hostname)"
echo "routes=$(($(wc -l < /proc/net/route) - 1))"
if /usr/bin/gh auth status >/dev/null 2>&1; then echo gh=in; else echo gh=out; fi
if [ -S "$1" ]; then echo sock=yes; else echo sock=no; fi
echo "store=$(ls -A "${2:-/nonexistent}" 2>/dev/null | wc -l)"
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
        for d in ["bin", "projects/bin", "home", "outside"] {
            std::fs::create_dir_all(path(d)).unwrap();
        }
        // A HOME of the rig's own, with something in it.
        std::fs::write(path("home/.planted"), "x").unwrap();
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
        // A stand-in `gh` that the broker grants GH_TOKEN, and that says
        // whether it got it. In the workspace, so an L1 job sees it.
        exe(
            path("projects/bin/gh"),
            "#!/bin/sh\nif [ -n \"$GH_TOKEN\" ]; then echo token=yes; else echo token=no; fi\n",
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
        tbl(&mut t, "tools").insert("proc_sync_secs".into(), 30.into());
        tbl(&mut t, "secrets").insert(
            "github_token".into(),
            "op://Test/github_token/credential".into(),
        );
        let gh: toml::Table = toml::from_str("env = { GH_TOKEN = \"github_token\" }").unwrap();
        let programs = tbl(tbl(&mut t, "broker"), "programs");
        programs.insert("gh".into(), toml::Value::Table(gh));
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
                        "{}:{}:{}",
                        path("projects/bin").display(),
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
        let s =
            UnixStream::connect(self.path("projects/sock")).map_err(|e| json!(e.to_string()))?;
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

    /// A turn in a new session whose model makes `calls`, and the text of
    /// each tool result, once the turn ends.
    fn turn(&self, prompt: &str, calls: Vec<(&'static str, Value)>) -> Vec<String> {
        self.asks(prompt, calls);
        let s = self.call("session.open", json!({"label": prompt})).unwrap();
        let sid = s["session_id"].clone();
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

    fn log(&self) -> String {
        let s = std::fs::read_to_string(self.path("theseusd.log")).unwrap_or_default();
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(30)..].join("\n")
    }
}

/// `key=value` lines into their values.
fn said(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}=")))
        .unwrap_or_else(|| panic!("no {key}= in:\n{text}"))
        .to_string()
}

fn probe_call(r: &Rig, sandbox: bool, store: bool) -> (&'static str, Value) {
    let mut argv = vec![
        "sh".to_string(),
        "-c".into(),
        PROBE.into(),
        "probe".into(),
        r.path("projects/sock").display().to_string(),
    ];
    if store {
        argv.push(r.path("projects/state/store").display().to_string());
    }
    ("proc_run", json!({"argv": argv, "sandbox": sandbox}))
}

/// The probe after serving runs `/bin/true` in L1 and finds it working;
/// health's `sandbox` block and the ledger's `sandbox.probe` row say so, with
/// the template's `ro_paths` this HOME lacks skipped (design §2.2, §2.10).
#[test]
fn the_probe_after_serving_finds_l1_working() {
    let mut r = Rig::start(|_| {});
    let h = r.until("the L1 probe", |h| !h["sandbox"]["probe"].is_null());
    let s = &h["sandbox"];
    assert_eq!(s["probe"]["ok"], true, "{s}\n{}", r.log());
    assert_eq!(s["default"], "l0");
    assert!(s["probe"]["start_ms"].as_f64().unwrap() > 0.0, "{s}");
    let skipped = s["probe"]["skipped"].to_string();
    assert!(
        skipped.contains(".cargo") && skipped.contains(".rustup"),
        "{s}"
    );
    assert!(
        s["cgroup"].is_string(),
        "the probe found the cgroup's mode: {s}"
    );
    // Its row follows health's answer by a frame.
    let t0 = Instant::now();
    let rows = loop {
        let rows = r.ledger("sandbox.probe");
        if !rows.is_empty() || t0.elapsed() > Duration::from_secs(10) {
            break rows;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["ok"], true);
}

/// The contract, by a probe script in L1 (design §3, 17b's tests), and the
/// same script at L0 beside it. The store is named only in L1: at L0 its
/// path is on the floor, which asks.
#[test]
fn a_probe_script_in_l1_shows_the_contract_and_l0_the_contrast() {
    let r = Rig::start(|_| {});
    let l1 = r.turn("in L1", vec![probe_call(&r, true, true)]);
    let l0 = r.turn("at L0", vec![probe_call(&r, false, false)]);
    let (l1, l0) = (&l1[0], &l0[0]);
    assert!(
        l1.starts_with("[ran in L1, the sandbox: no network, no secret; "),
        "{l1}"
    );
    let uid = unsafe { libc::getuid() }.to_string();
    assert_eq!(said(l1, "uid"), uid, "the operator's own uid");
    assert_eq!(said(l1, "cap"), "0000000000000000", "no capability");
    assert_eq!(said(l1, "home"), "0", "an empty HOME");
    assert_eq!(said(l1, "host"), "theseus-l1");
    assert_eq!(said(l1, "routes"), "0", "no route out");
    assert_eq!(said(l1, "gh"), "out", "gh is not logged in");
    assert_eq!(said(l1, "sock"), "no", "the daemon's socket is not there");
    assert_eq!(said(l1, "store"), "0", "the store is not there");
    // At L0, the same uid, but the operator's HOME, host, and socket.
    assert_eq!(said(l0, "uid"), uid);
    assert_eq!(said(l0, "home"), "1");
    assert_ne!(said(l0, "host"), "theseus-l1");
    assert_eq!(said(l0, "sock"), "yes");
    assert!(!l0.contains("L1"), "{l0}");
    // The surfaces: the gate's record, the job's start, the ledger.
    let started = r.ledger("sandbox.started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(started[0]["limits"]["pids"], 512);
    let jobs = r.ledger("tool.job_started");
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0]["class"], "l1");
    assert!(jobs[1].get("class").is_none(), "{jobs:?}");
}

/// `[sandbox] l1_argv` routes a call to L1 that never asked (decision 2).
#[test]
fn l1_argv_routes_a_call_to_l1() {
    let r = Rig::start(|t| {
        let s: toml::Table = toml::from_str("l1_argv = [[\"uname\"]]").unwrap();
        t.insert("sandbox".into(), toml::Value::Table(s));
    });
    let out = r.turn(
        "uname",
        vec![("proc_run", json!({"argv": ["uname", "-n"]}))],
    );
    assert!(out[0].contains("theseus-l1"), "{}", out[0]);
}

/// No fallback (design §2.2): a job whose working directory its view lacks
/// cannot start in L1, and the call fails with the stage and the error; it
/// never runs at L0, where the directory is there and its command would
/// leave a mark.
#[test]
fn a_job_that_cannot_start_in_l1_fails_and_never_runs_at_l0() {
    let r = Rig::start(|_| {});
    let mark = r.path("outside/ran");
    let out = r.turn(
        "outside",
        vec![(
            "proc_run",
            json!({"argv": ["touch", mark.display().to_string()],
                "cwd": r.path("outside").display().to_string(), "sandbox": true}),
        )],
    );
    assert!(out[0].contains("could not start in L1"), "{}", out[0]);
    assert!(
        out[0].contains("It did not run, in L1 or at L0"),
        "{}",
        out[0]
    );
    std::thread::sleep(Duration::from_millis(200));
    assert!(!mark.exists(), "the job ran at L0");
}

/// Decision 4: a program the broker grants a secret, asked to run in L1,
/// gets none (18d brings them); its result says so, and so does a
/// `secret.withheld` row. At L0 the same program gets it.
#[test]
fn a_granted_program_asked_to_run_in_l1_gets_no_secret() {
    let r = Rig::start(|_| {});
    let gh = |sandbox| {
        (
            "proc_run",
            json!({"argv": ["gh", "api", "user"], "sandbox": sandbox}),
        )
    };
    let l1 = r.turn("gh in L1", vec![gh(true)]);
    assert!(l1[0].contains("token=no"), "{}", l1[0]);
    assert!(
        l1[0].contains("gh got no GH_TOKEN: no secret reaches a job in L1"),
        "{}",
        l1[0]
    );
    let withheld = r.ledger("secret.withheld");
    assert_eq!(withheld.len(), 1, "{withheld:?}");
    let l0 = r.turn("gh at L0", vec![gh(false)]);
    assert!(l0[0].contains("token=yes"), "{}", l0[0]);
}

/// Every live process whose command line holds `marker` as an argument, from
/// the host's `/proc`, which sees into an L1 job's pid namespace.
fn with_marker(marker: &str) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| c.split(|&b| b == 0).any(|a| a == marker.as_bytes()))
        })
        .collect()
}

/// M4 18a's L1 row (design §2.3): an L1 job whose command leaves a `setsid`
/// sleeper behind runs on in the background; `execution.cancel` stops it
/// through its init (this rig's cgroup is not delegated), and answers that its
/// pid namespace is gone, with the count: the init and the two sleepers. The
/// action, the ledger, health, and a `/proc` scan agree.
#[test]
fn a_cancel_of_an_l1_job_is_verified_by_its_pid_namespace() {
    let marker = "300.1806";
    let r = Rig::start(|t| {
        let tools = t.get_mut("tools").and_then(|v| v.as_table_mut()).unwrap();
        tools.insert("proc_sync_secs".into(), 1.into());
    });
    let script = format!("setsid sleep {marker} & exec sleep {marker}");
    let out = r.turn(
        "sleepers in L1",
        vec![(
            "proc_run",
            json!({"argv": ["sh", "-c", script], "sandbox": true}),
        )],
    );
    assert!(out[0].contains("background job"), "{}", out[0]);
    let t0 = Instant::now();
    while with_marker(marker).len() < 2 {
        assert!(
            t0.elapsed() < Duration::from_secs(20),
            "the sleepers never ran"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let actions = r.call("action.list", json!({})).unwrap();
    let job = actions["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["tool"] == "proc.run" && a["state"] == "dispatched")
        .cloned()
        .expect("the job, running");
    let res = r
        .call(
            "execution.cancel",
            json!({"execution_id": job["execution_id"], "author": "test"}),
        )
        .unwrap();
    let left = with_marker(marker);
    for pid in &left {
        unsafe { libc::kill(*pid as i32, libc::SIGKILL) };
    }
    assert!(
        left.is_empty(),
        "a /proc scan found the job still running: {left:?}"
    );
    let v = &res["verdicts"][0];
    assert_eq!(v["verified_by"], "pidns", "{res}");
    assert_eq!(v["state"], "termination_verified", "{res}");
    assert_eq!(
        (v["killed"].as_u64(), v["survivors"].as_u64()),
        (Some(3), Some(0)),
        "{res}"
    );
    let after = r.call("action.list", json!({})).unwrap();
    let a = after["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["correlation_id"] == job["correlation_id"])
        .unwrap();
    assert_eq!(a["verdict"]["verified_by"], "pidns", "{a}");
    let rows = r.ledger("action.cancel_verified");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["killed"], 3);
    let h = r.call("health", Value::Null).unwrap();
    assert_eq!(
        h["cancels"],
        json!([{"backend": "l1", "state": "verified", "n": 1}])
    );
}

/// The jobs bench's L1 row (design §2.10; `theseus-sim bench jobs`): `/bin/true`
/// through the real job wrapper, detached as the daemon starts a job, in L1,
/// 20 times. Each start, from the wrapper's spawn to the command's exec, is
/// in its completion (`detail.sandbox.start_us`). The target is a p95 under
/// 25 ms; this fails only past ten times it, so a loaded machine never fails
/// the gate, and the report quotes the bench itself.
#[test]
fn the_jobs_bench_l1_row() {
    use theseus_kernel::job::{self, WrapperArgs, L1};
    let dir = tempfile::tempdir().unwrap();
    let spool = theseus_kernel::Spool::open(&dir.path().join("spool")).unwrap();
    let ws = dir.path().join("work");
    std::fs::create_dir_all(&ws).unwrap();
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let mut starts = Vec::new();
    for i in 0..20 {
        let args = WrapperArgs {
            spool_dir: spool.dir().to_path_buf(),
            correlation_id: format!("bench-{i}"),
            deadline_ms: 30_000,
            notify_socket: None,
            argv: vec!["/bin/true".into()],
            cwd: Some(ws.clone()),
            env: vec![("PATH".into(), "/usr/bin:/bin".into())],
            umask: None,
            redact: vec![],
            output_max_bytes: job::DEFAULT_OUTPUT_MAX_BYTES,
            sandbox: Some(L1 {
                workspace: vec![ws.clone()],
                limits: job::SandboxLimits::default(),
                memory_mb: 2048,
                ..Default::default()
            }),
        };
        job::spawn_detached(&theseusd, &[job::WRAPPER_MODE], &spool, &args).unwrap();
        let t0 = Instant::now();
        let c = loop {
            if let Some(c) = spool.read_completion(&args.correlation_id).unwrap() {
                break c;
            }
            assert!(
                t0.elapsed() < Duration::from_secs(30),
                "job {i}: no completion"
            );
            std::thread::sleep(Duration::from_millis(1));
        };
        let d = c.detail.unwrap_or_default();
        assert_eq!(d["exit_code"], 0, "{d}");
        let us = d
            .pointer("/sandbox/start_us")
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("job {i} did not start in L1: {d}"));
        if i >= 2 {
            starts.push(us);
        }
    }
    starts.sort_unstable();
    let p95 = starts[(starts.len() * 95).div_ceil(100) - 1];
    eprintln!(
        "L1 start: p50 {} us, p95 {p95} us",
        starts[starts.len() / 2]
    );
    assert!(
        p95 < 250_000,
        "an L1 start's p95 is {p95} us, ten times 25 ms"
    );
}
