//! `theseus --spawn ask` follows what its turn left for later (theseus-mqxk):
//! a job's late result, and a wake due within `--follow-for`. Its daemon ends
//! with the run, so before this the late result's turn, and the wake's, never
//! ran: the stop began first. Past the bound, the run says what it left, and
//! `wake.at` tells the model a wake past it will not fire here.
//!
//! Each test drives the real CLI, which spawns the real `theseusd --stdio` on
//! a config with no vault (every secret `env:`) and the stand-in model
//! (`common::model`), with `[tools] proc_sync_secs = 1`, so a 3 s command is
//! answered `background` and its result comes back later.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::model::FakeModel;
use serde_json::{json, Value};

/// Every secret's value: an invented one, from the daemon's environment.
const KEY_VAR: &str = "THESEUS_FOLLOW_TEST_KEY";

/// The stand-in's tool calls by prompt: `run the slow job` runs a command
/// that marks its start, sleeps `JOB_SECS` (or `LONG_SECS` for `run the long
/// job`), marks its end and prints a line; `wake me in 2s` sets a wake 2 s
/// ahead. Anything else gets text, and a request carrying a tool result
/// (the background answer) gets `Done.`; a late result's turn reads it as
/// text, and gets `Nothing to do.`.
fn calls(prompt: &str) -> Vec<(&'static str, Value)> {
    let job = |secs: u64| {
        // The line's last word is the shell's, so only the job's output
        // holds the whole line, never its command.
        let script = format!(
            "touch started; sleep {secs}; touch finished; w=lit; echo the lighthouse is $w"
        );
        vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
    };
    if prompt.contains("run the slow job") {
        job(3)
    } else if prompt.contains("run the long job") {
        job(60)
    } else if prompt.contains("wake me in 2s") {
        vec![(
            "wake_at",
            json!({"after": "2s", "note": "check the lighthouse"}),
        )]
    } else {
        vec![]
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
    took: Duration,
}

impl Run {
    /// stdout as the one JSON object it must be: the whole of it parses.
    fn json(&self) -> Value {
        serde_json::from_str(self.stdout.trim())
            .unwrap_or_else(|e| panic!("stdout is not one JSON object ({e}):\n{}", self.stdout))
    }
}

/// A temp dir with `config.toml` (the safe template, every secret from
/// `KEY_VAR`, every provider at the stand-in, `proc_sync_secs = 1`) and
/// `projects/`.
fn rig(model: &FakeModel) -> tempfile::TempDir {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    let mut t: toml::Table = common::safe_note(&theseusd, &projects, 100.0)
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
    for (_, v) in table(&mut t, "secrets").iter_mut() {
        *v = format!("env:{KEY_VAR}").into();
    }
    table(&mut t, "tools").insert("proc_sync_secs".into(), 1.into());
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

/// `theseus --spawn <theseusd> ARGS…` in `dir`, with no vault.
fn cli(dir: &Path, args: &[&str]) -> Command {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let mut c = Command::new(theseusd.with_file_name("theseus"));
    c.arg("--spawn")
        .arg(&theseusd)
        .args(args)
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
        .env(KEY_VAR, "tv-follow-41c2")
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET");
    c
}

/// `theseus --spawn <theseusd> ARGS…`, run to its end.
fn run(dir: &Path, args: &[&str]) -> Run {
    let t0 = Instant::now();
    let out = cli(dir, args).output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        took: t0.elapsed(),
    }
}

/// The store the spawned daemon used, opened again: what it replayed and
/// whether it repaired the index.
fn reopened(dir: &Path) -> (u64, bool) {
    let store = theseus_core::store::Store::open(&dir.join("state/store-stdio")).unwrap();
    let st = store.stats().unwrap();
    (st.replayed_into_index, st.index_repaired)
}

/// Whether the stand-in was asked about the job's late result.
fn asked_the_late_result(model: &FakeModel) -> bool {
    model
        .requests()
        .iter()
        .any(|r| r["messages"].to_string().contains("the lighthouse is lit"))
}

/// The text of the `wake.at` result the model read, from its requests.
fn wake_answer(model: &FakeModel) -> String {
    model
        .requests()
        .iter()
        .map(|r| r["messages"].to_string())
        .find(|m| m.contains("Set wake"))
        .unwrap_or_default()
}

/// The late result is followed: under `--json`, one object whose `output`
/// is the late result's turn's and whose spend is both turns', the ask's own
/// in `asked` and the follow's in `continuations`; the model read the job's
/// output. Before the follow, the run ended with its turn and the result's
/// turn never ran (the model was asked twice, not three times).
#[test]
fn a_late_result_is_followed_and_its_spend_is_in_the_one_json_object() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(dir.path(), &["--json", "ask", "run the slow job"]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    let v = r.json();
    assert!(
        asked_the_late_result(&model),
        "the late result's turn never ran: {} requests\n{}",
        model.requests().len(),
        r.stderr
    );
    assert_eq!(model.requests().len(), 3, "{}", r.stderr);
    assert_eq!(
        v["continuation"], true,
        "the last turn is the late result's: {v}"
    );
    let asked = &v["asked"];
    let follow = v["continuations"].as_array().expect("continuations");
    assert_eq!(follow.len(), 1, "{v}");
    assert_eq!(follow[0]["turn_id"], v["turn_id"]);
    assert_ne!(asked["turn_id"], v["turn_id"]);
    let cost = |x: &Value| x["cost_usd"].as_f64().unwrap_or(0.0);
    assert!(cost(asked) > 0.0 && cost(&follow[0]) > 0.0, "{v}");
    assert!(
        (cost(&v) - cost(asked) - cost(&follow[0])).abs() < 1e-9,
        "the spend is both turns': {v}"
    );
    let out = |x: &Value| x["usage"]["output_tokens"].as_u64().unwrap_or(0);
    assert_eq!(out(&v), out(asked) + out(&follow[0]), "{v}");
    assert_eq!(
        v["output"], "Nothing to do.",
        "the late result's turn's: {v}"
    );
    assert_eq!(asked["output"], "Done.", "{v}");
    assert!(v.get("later").is_none(), "nothing was left: {v}");
    assert_eq!(
        reopened(dir.path()),
        (0, false),
        "the daemon did not stop cleanly"
    );
}

/// The same run in text: each turn's reply on stdout, the follow named on
/// stderr between them, and the last status line names nothing left.
#[test]
fn a_late_result_is_followed_and_printed() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(dir.path(), &["ask", "run the slow job"]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert_eq!(r.stdout, "Done.\nNothing to do.\n", "{}", r.stderr);
    assert!(
        r.stderr.contains("[following: 1 job(s) running"),
        "{}",
        r.stderr
    );
    assert!(r.stderr.contains("· continuation ·"), "{}", r.stderr);
    assert!(!r.stderr.contains("the run ended"), "{}", r.stderr);
}

/// `--follow-for 0` ends the run with its turn, as before: the late result
/// is lost, and the status line and the JSON say the job was left running.
#[test]
fn follow_for_0_leaves_the_job_and_says_so() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(
        dir.path(),
        &["--json", "ask", "--follow-for", "0", "run the slow job"],
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    let v = r.json();
    assert_eq!(v["later"]["jobs"], 1, "{v}");
    assert!(v.get("continuations").is_none(), "{v}");
    assert_eq!(model.requests().len(), 2, "a later turn ran");
    let r = run(
        dir.path(),
        &["ask", "--follow-for", "0", "run the slow job"],
    );
    assert!(
        r.stderr.contains("· 1 job still running: the run ended]"),
        "{}",
        r.stderr
    );
}

/// The bound comes while the job runs: the run says what still runs, exits
/// 0 at the bound, and its daemon stops cleanly.
#[test]
fn the_bound_reached_says_what_still_runs_and_stops_the_daemon_cleanly() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(
        dir.path(),
        &["--json", "ask", "--follow-for", "2s", "run the long job"],
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    assert!(
        r.stderr
            .contains("[still running after --follow-for 2 s: 1 job still running"),
        "{}",
        r.stderr
    );
    assert!(r.took < Duration::from_secs(30), "took {:?}", r.took);
    assert_eq!(r.json()["later"]["jobs"], 1);
    assert_eq!(
        reopened(dir.path()),
        (0, false),
        "the daemon did not stop cleanly"
    );
}

/// A wake within the bound: its turn runs and prints, and the `wake.at`
/// result the model read is today's.
#[test]
fn a_wake_within_the_bound_is_followed() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(dir.path(), &["--json", "ask", "wake me in 2s"]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    let v = r.json();
    assert_eq!(v["continuations"].as_array().map(Vec::len), Some(1), "{v}");
    assert!(
        model.requests().iter().any(
            |q| q["messages"].to_string().contains("check the lighthouse")
                && q["messages"].to_string().contains("wake (set")
        ),
        "the wake's turn never ran"
    );
    let words = wake_answer(&model);
    assert!(
        words.contains("This conversation gets a turn then"),
        "{words}"
    );
    assert!(v.get("later").is_none(), "{v}");
}

/// A wake past the bound: `wake.at` tells the model it will not fire in this
/// run, the run ends with its turn, and the status line and the JSON name
/// the wake left.
#[test]
fn a_wake_past_the_bound_is_said_not_to_fire() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(
        dir.path(),
        &["--json", "ask", "--follow-for", "1s", "wake me in 2s"],
    );
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    let v = r.json();
    assert_eq!(v["later"]["wakes"].as_array().map(Vec::len), Some(1), "{v}");
    assert_eq!(v["later"]["wakes"][0]["note"], "check the lighthouse");
    let words = wake_answer(&model);
    assert!(words.contains("will not fire in this run"), "{words}");
    assert_eq!(model.requests().len(), 2, "the wake's turn ran");
    let r = run(dir.path(), &["ask", "--follow-for", "1s", "wake me in 2s"]);
    assert!(
        r.stderr.contains("· 1 wake not fired: the run ended]"),
        "{}",
        r.stderr
    );
}

/// A SIGTERM while the follow waits on the job stops the session's work, as
/// `/stop` does: the job is stopped, the run exits 9 with one JSON object
/// carrying the ask's spend, and its daemon stops cleanly.
#[test]
fn a_signal_during_the_follow_stops_it_with_the_spend() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let file = |name: &str| std::fs::File::create(dir.path().join(name)).unwrap();
    let mut child = cli(dir.path(), &["--json", "ask", "run the long job"])
        .stdout(file("turn.json"))
        .stderr(file("stderr.log"))
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let stderr = || std::fs::read_to_string(dir.path().join("stderr.log")).unwrap_or_default();
    let t0 = Instant::now();
    // Under --json no line says the follow began: the ask's turn has ended
    // once the model has answered the job's background answer.
    while model.requests().len() < 2 || !dir.path().join("projects/started").exists() {
        assert!(t0.elapsed() < Duration::from_secs(30), "{}", stderr());
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(500));
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(45),
            "no exit:\n{}",
            stderr()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = std::fs::read_to_string(dir.path().join("turn.json")).unwrap();
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("not one JSON object ({e}):\n{stdout}"));
    assert_eq!(status.code(), Some(9), "{v}\n{}", stderr());
    assert!(
        stderr().contains("SIGTERM: stopping the session's later work"),
        "{}",
        stderr()
    );
    assert!(v["cost_usd"].as_f64().unwrap_or(0.0) > 0.0, "no spend: {v}");
    assert!(
        !dir.path().join("projects/finished").exists(),
        "the job was not stopped"
    );
    assert_eq!(
        reopened(dir.path()),
        (0, false),
        "the daemon did not stop cleanly"
    );
}

/// Nothing left for later: no follow, no wait after the turn, and the result
/// carries no `later`.
#[test]
fn a_turn_that_leaves_nothing_is_not_followed() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let r = run(dir.path(), &["--json", "ask", "say hello"]);
    assert_eq!(r.code, 0, "{}\n{}", r.stdout, r.stderr);
    let v = r.json();
    assert!(v.get("later").is_none(), "{v}");
    assert!(v.get("continuations").is_none(), "{v}");
    assert!(!r.stderr.contains("[following"), "{}", r.stderr);
}

/// FAST (theseus-mqxk): an `ask` with nothing left waits for nothing after
/// its turn. Times 20 runs of `say hello`: the whole run, and from its
/// turn's end (the trace's start plus the turn's elapsed time) to the
/// process's exit. A measure, not a check: `--ignored --nocapture`.
#[test]
#[ignore]
fn measure_the_wait_after_a_turn_that_leaves_nothing() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let now_ms = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    };
    let (mut wall, mut after) = (Vec::new(), Vec::new());
    for _ in 0..20 {
        let r = run(dir.path(), &["--json", "ask", "say hello"]);
        let exited = now_ms();
        assert_eq!(r.code, 0, "{}", r.stderr);
        let v = r.json();
        let ended = v["trace"]["attrs"]["started_unix_ms"].as_u64().unwrap()
            + v["elapsed_ms"].as_u64().unwrap();
        wall.push(r.took.as_millis() as u64);
        after.push(exited.saturating_sub(ended));
    }
    let median = |v: &mut Vec<u64>| {
        v.sort_unstable();
        v[v.len() / 2]
    };
    eprintln!(
        "spawn ask, 20 runs: wall median {} ms (max {}), turn end to exit median {} ms (max {})",
        median(&mut wall),
        wall[wall.len() - 1],
        median(&mut after),
        after[after.len() - 1]
    );
}

/// A SIGTERM while a later turn runs (its model call stalls at the
/// stand-in): that turn ends `stopped`, with what it spent, as `/stop` ends
/// one; the run exits 9 with one JSON object whose spend holds both turns.
#[test]
fn a_signal_during_a_later_turn_stops_it_with_its_spend() {
    let model = FakeModel::start(calls);
    let dir = rig(&model);
    let file = |name: &str| std::fs::File::create(dir.path().join(name)).unwrap();
    let mut child = cli(dir.path(), &["--json", "ask", "run the slow job"])
        .stdout(file("turn.json"))
        .stderr(file("stderr.log"))
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let stderr = || std::fs::read_to_string(dir.path().join("stderr.log")).unwrap_or_default();
    let t0 = Instant::now();
    while model.requests().len() < 2 {
        assert!(t0.elapsed() < Duration::from_secs(30), "{}", stderr());
        std::thread::sleep(Duration::from_millis(10));
    }
    // The late result's turn asks next: its call gets no answer.
    model.stall_next(1);
    while model.requests().len() < 3 {
        assert!(t0.elapsed() < Duration::from_secs(30), "{}", stderr());
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap()
        .success());
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(45),
            "no exit:\n{}",
            stderr()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = std::fs::read_to_string(dir.path().join("turn.json")).unwrap();
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("not one JSON object ({e}):\n{stdout}"));
    assert_eq!(status.code(), Some(9), "{v}\n{}", stderr());
    assert_eq!(v["stop_reason"], "stopped", "{v}");
    let later = &v["continuations"][0];
    assert_eq!(later["stop_reason"], "stopped", "{v}");
    let cost = |x: &Value| x["cost_usd"].as_f64().unwrap_or(0.0);
    assert!(cost(later) > 0.0, "the stopped turn's spend: {v}");
    assert!(
        (cost(&v) - cost(&v["asked"]) - cost(later)).abs() < 1e-9,
        "the spend is both turns': {v}"
    );
    assert_eq!(
        reopened(dir.path()),
        (0, false),
        "the daemon did not stop cleanly"
    );
}
