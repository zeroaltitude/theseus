//! A headless run (theseus-n88g.2): `theseus --spawn ask` exits with a code
//! that says how its turn ended, and the daemon it spawned gets a clean stop,
//! so the next open of its store replays nothing.
//!
//! Each test drives the real CLI, which spawns the real `theseusd --stdio` on
//! a config with no vault (every secret `env:`, theseus-n88g.1) and the
//! stand-in model (`common::model`).

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use serde_json::{json, Value};

/// Every secret's value: an invented one, from the daemon's environment.
const KEY_VAR: &str = "THESEUS_HEADLESS_TEST_KEY";

struct Run {
    code: i32,
    turn: Value,
    stderr: String,
}

/// A temp dir with `config.toml` (the safe template, every secret from
/// `KEY_VAR`, both providers at the stand-in) and `projects/`.
fn rig(model: &FakeModel, tweak: impl FnOnce(&mut toml::Table)) -> tempfile::TempDir {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let dir = tempfile::tempdir().unwrap();
    let projects = dir.path().join("projects");
    std::fs::create_dir_all(&projects).unwrap();
    std::fs::write(projects.join("notes.txt"), "low water at six\n").unwrap();
    let mut t: toml::Table = common::safe_note(&theseusd, &projects, 100.0)
        .parse()
        .unwrap();
    set(&mut t, &["model", "api_base"], model.base.clone().into());
    let keys = |t: &toml::Table, key: &str| -> Vec<String> {
        t.get(key)
            .and_then(toml::Value::as_table)
            .map(|v| v.keys().cloned().collect())
            .unwrap_or_default()
    };
    for name in keys(&t, "providers") {
        set(
            &mut t,
            &["providers", name.as_str(), "api_base"],
            model.base.clone().into(),
        );
    }
    for name in keys(&t, "secrets") {
        set(
            &mut t,
            &["secrets", name.as_str()],
            format!("env:{KEY_VAR}").into(),
        );
    }
    tweak(&mut t);
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

/// `theseus --spawn <theseusd> --json ARGS…` in `dir`, with no vault.
fn spawn_cli(dir: &Path, args: &[&str]) -> Command {
    let mut c = spawn_bare(dir);
    c.arg("--json").args(args);
    c
}

/// `theseus --spawn <theseusd>` in `dir`, with no vault: its arguments to come.
fn spawn_bare(dir: &Path) -> Command {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let mut c = Command::new(theseusd.with_file_name("theseus"));
    c.arg("--spawn")
        .arg(&theseusd)
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
        .env(KEY_VAR, "tv-headless-7f3a9c")
        // The stop's phases are logged at info (theseus-vjn7), the level a
        // daemon logs at with nothing set: the spawned daemon inherits it.
        .env("THESEUS_LOG", "info")
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET");
    c
}

/// `theseus --spawn <theseusd> --json ask PROMPT` in `dir`, run to its end.
fn ask(dir: &Path, prompt: &str) -> Run {
    let out = spawn_cli(dir, &["ask", prompt]).output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    Run {
        code: out.status.code().unwrap_or(-1),
        turn: serde_json::from_str(stdout.trim()).unwrap_or(Value::Null),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The store `--spawn`'s daemon used, opened again: what it replayed and
/// whether it repaired the index.
fn reopened(dir: &Path) -> (u64, bool) {
    let store = theseus_core::store::Store::open(&dir.join("state/store-stdio")).unwrap();
    let st = store.stats().unwrap();
    (st.replayed_into_index, st.index_repaired)
}

/// The model reads `notes.txt` when the prompt asks it to.
fn reads_notes(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.contains("read the notes") {
        vec![("fs_read", json!({"path": "notes.txt"}))]
    } else {
        vec![]
    }
}

fn set(t: &mut toml::Table, path: &[&str], value: toml::Value) {
    let (last, tables) = path.split_last().unwrap();
    let mut at = t;
    for key in tables {
        at = at
            .entry(*key)
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .unwrap();
    }
    at.insert((*last).into(), value);
}

/// The model ended its turn: 0. The daemon `--spawn` started stopped
/// cleanly: its store, opened again, replays nothing and repairs nothing,
/// where a killed daemon's next open replayed the run's tail. Its log names
/// the stop's phases and the index's close at the default level, so a slow
/// stop says where it went (theseus-vjn7).
#[test]
fn a_turn_the_model_ends_exits_0_and_its_daemon_stops_cleanly() {
    let model = FakeModel::start(reads_notes);
    let dir = rig(&model, |_| {});
    let run = ask(dir.path(), "read the notes, then say what they hold");
    assert_eq!(run.code, 0, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "no_tool_calls", "{}", run.turn);
    assert_eq!(run.turn["tool_calls"], 1);
    assert_eq!(
        reopened(dir.path()),
        (0, false),
        "the next open replayed the spawned daemon's tail"
    );
    assert!(
        run.stderr.contains("\"row and checkpoint\""),
        "no clean stop in the daemon's log:\n{}",
        run.stderr
    );
    assert!(
        run.stderr.contains("store: index closed"),
        "no index close in the daemon's log:\n{}",
        run.stderr
    );
    assert!(
        !run.stderr.contains("tv-headless-7f3a9c"),
        "the key's value was printed"
    );
}

/// A provider's fault fails the turn: 1, the code of every daemon error.
#[test]
fn a_failed_turn_exits_1() {
    let model = FakeModel::start(reads_notes);
    model.fail_always(400);
    let dir = rig(&model, |_| {});
    let run = ask(dir.path(), "say hello");
    assert_eq!(run.code, 1, "{}", run.stderr);
    assert!(run.stderr.contains("invalid_request"), "{}", run.stderr);
}

/// The session's spend limit, which asks, is below the first call's
/// reservation: the turn waits for the operator's reset, 5.
#[test]
fn a_turn_at_the_spend_limit_exits_5() {
    let model = FakeModel::start(reads_notes);
    let dir = rig(&model, |t| {
        set(t, &["kernel", "spend_limit_usd"], 0.002.into());
        set(t, &["kernel", "spend_limit_mode"], "ask".into());
    });
    let run = ask(dir.path(), "say hello");
    assert_eq!(run.code, 5, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "budget", "{}", run.turn);
    assert!(run.stderr.contains("spend limit"), "{}", run.stderr);
    assert!(model.requests().is_empty(), "a call ran past the limit");
}

/// A call waits for the operator's approval, which a headless run cannot
/// give: 6.
#[test]
fn a_turn_parked_on_an_approval_exits_6() {
    let model = FakeModel::start(reads_notes);
    let dir = rig(&model, |t| {
        set(t, &["policy", "tools", "fs.read"], "approve".into());
    });
    let run = ask(dir.path(), "read the notes");
    assert_eq!(run.code, 6, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "awaiting_confirm", "{}", run.turn);
    assert!(run.turn["awaiting_confirm"].is_string(), "{}", run.turn);
}

/// The model declines, and so does its fallback (theseus-7gir.18): 7.
#[test]
fn a_refused_turn_exits_7() {
    let model = FakeModel::start(reads_notes);
    model.decline_next(2);
    let dir = rig(&model, |_| {});
    let run = ask(dir.path(), "say hello");
    assert_eq!(run.code, 7, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "refusal", "{}", run.turn);
    assert_eq!(model.requests().len(), 2, "one fallback, never a loop");
}

/// The model declines and its fallback answers (theseus-7gir.18): 0, and the
/// plain output says so in a line of its own, above the status line.
#[test]
fn a_refusal_its_fallback_answers_exits_0_and_says_so() {
    let model = FakeModel::start(reads_notes);
    model.decline_next(1);
    let dir = rig(&model, |_| {});
    let out = spawn_bare(dir.path())
        .args(["ask", "say hello"])
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    let line = "Sonnet 5.5 declined (cyber); Sonnet 5 answered.";
    let at = stderr.find(line).expect(&stderr);
    assert!(
        stderr[at..].contains("[sonnet → anthropic/claude-sonnet-5 "),
        "{stderr}"
    );
    assert!(
        !stdout.contains(line),
        "the reply's text is the model's alone: {stdout}"
    );
    let models: Vec<Value> = model
        .requests()
        .iter()
        .map(|r| r["model"].clone())
        .collect();
    assert_eq!(
        models,
        [json!("claude-sonnet-5-5"), json!("claude-sonnet-5")]
    );
}

/// The loop cap, which ends, ends the turn before the model does: 8.
#[test]
fn a_turn_cut_by_the_loop_cap_exits_8() {
    let model = FakeModel::start(reads_notes);
    let dir = rig(&model, |t| {
        set(t, &["model", "max_loops"], 1.into());
        set(t, &["model", "max_loops_mode"], "end".into());
        set(t, &["profiles", "sonnet", "max_loops"], 1.into());
        set(t, &["profiles", "sonnet", "max_loops_mode"], "end".into());
    });
    let run = ask(dir.path(), "read the notes");
    assert_eq!(run.code, 8, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "max_loops", "{}", run.turn);
}

/// The same limit and cap that notify, the defaults (theseus-usei): the
/// turn goes past both and ends as the model ends it, 0.
#[test]
fn a_turn_past_a_limit_and_a_cap_that_notify_goes_on_and_exits_0() {
    let model = FakeModel::start(reads_notes);
    let dir = rig(&model, |t| {
        set(t, &["kernel", "spend_limit_usd"], 0.002.into());
        set(t, &["model", "max_loops"], 1.into());
        set(t, &["profiles", "sonnet", "max_loops"], 1.into());
    });
    let run = ask(dir.path(), "read the notes");
    assert_eq!(run.code, 0, "{}\n{}", run.turn, run.stderr);
    assert_eq!(run.turn["stop_reason"], "no_tool_calls", "{}", run.turn);
    assert_eq!(model.requests().len(), 2, "both calls went out");
}

/// A SIGTERM to a `--spawn` run (a harness's timeout) stops the turn as
/// `/stop` does, here while it waits on a job: the job is stopped, the turn
/// answers `stopped` with what it spent, 9, and its daemon stops cleanly. So
/// the store's next open replays nothing, and the execution waits on input,
/// with nothing for a later spawn to resume.
#[test]
fn a_sigterm_stops_the_turn_with_its_spend_and_exits_9() {
    let model = FakeModel::start(|prompt| {
        if prompt.contains("run the job") {
            let script = "touch started; exec sleep 60";
            vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
        } else {
            vec![]
        }
    });
    let dir = rig(&model, |_| {});
    let file = |name: &str| std::fs::File::create(dir.path().join(name)).unwrap();
    let mut cli = spawn_cli(dir.path(), &["ask", "run the job"])
        .stdout(file("turn.json"))
        .stderr(file("stderr.log"))
        .spawn()
        .unwrap();
    let stderr = || std::fs::read_to_string(dir.path().join("stderr.log")).unwrap_or_default();
    let waited = |what: &str, done: &mut dyn FnMut() -> bool| {
        let t0 = std::time::Instant::now();
        while !done() {
            assert!(t0.elapsed().as_secs() < 30, "{what}:\n{}", stderr());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    };
    waited("the job never started", &mut || {
        dir.path().join("projects/started").exists()
    });
    let pid = cli.id().to_string();
    assert!(Command::new("kill")
        .args(["-TERM", &pid])
        .status()
        .unwrap()
        .success());
    let mut status = None;
    waited("the turn did not stop", &mut || {
        status = cli.try_wait().unwrap();
        status.is_some()
    });
    let turn: Value =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join("turn.json")).unwrap())
            .unwrap_or(Value::Null);
    let stderr = stderr();
    assert_eq!(status.unwrap().code(), Some(9), "{turn}\n{stderr}");
    assert_eq!(turn["stop_reason"], "stopped", "{turn}");
    assert!(
        turn["cost_usd"].as_f64().unwrap_or(0.0) > 0.0,
        "no spend: {turn}"
    );
    assert!(stderr.contains("SIGTERM: stopping the turn"), "{stderr}");
    assert_eq!(model.requests().len(), 1, "the model was asked again");
    assert_eq!(
        reopened(dir.path()),
        (0, false),
        "the stopped run's daemon did not stop cleanly"
    );
    // A later spawn on the same store finds the execution waiting on its
    // next input, and asks the model nothing.
    let out = spawn_cli(dir.path(), &["history"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let history: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        history["session"]["execution_state"], "waiting",
        "{}",
        history["session"]
    );
    assert_eq!(model.requests().len(), 1, "a later spawn resumed the turn");
}

/// A headless run on a host whose seccomp profile answers `clone3` with
/// ENOSYS, as Docker's default does (theseus-f7tz): `proc.run`'s command
/// starts by `clone` and its output reaches the model, and a command that
/// cannot start reaches it as why. Every `proc.run` of a bench run in Docker
/// failed to start before ("Function not implemented (os error 38)"), and
/// the model read "(no output)". The profile is a filter on the thread that
/// starts the CLI, which its daemon and every wrapper inherit.
#[test]
fn proc_run_runs_where_the_host_refuses_clone3_and_a_failed_start_says_why() {
    let model = FakeModel::start(|prompt| {
        if prompt.contains("run the echo") {
            let script = "echo from-the-container-7f3a; echo err-7f3a >&2";
            vec![("proc_run", json!({"argv": ["sh", "-c", script]}))]
        } else if prompt.contains("run the missing one") {
            vec![(
                "proc_run",
                json!({"argv": ["theseus-no-such-program-7f3a"]}),
            )]
        } else {
            vec![]
        }
    });
    let dir = rig(&model, |t| {
        set(t, &["policy", "enforcement"], "open".into());
    });
    let path = dir.path().to_path_buf();
    let (echo, missing) = std::thread::spawn(move || {
        theseus_sandbox::seccomp::refuse_here(&[(libc::SYS_clone3, libc::ENOSYS)]).unwrap();
        (
            ask(&path, "run the echo"),
            ask(&path, "run the missing one"),
        )
    })
    .join()
    .unwrap();
    // What the model was sent after each call: the call's result.
    let results: Vec<String> = model
        .requests()
        .iter()
        .map(|r| r["messages"].to_string())
        .filter(|m| m.contains("tool_result"))
        .collect();
    assert_eq!(echo.code, 0, "{}\n{}", echo.turn, echo.stderr);
    assert_eq!(echo.turn["tool_calls"], 1, "{}", echo.turn);
    assert_eq!(results.len(), 2, "{results:?}\n{}", echo.stderr);
    assert!(
        results[0].contains("from-the-container-7f3a") && results[0].contains("err-7f3a"),
        "the command's output did not reach the model: {}",
        results[0]
    );
    assert!(!results[0].contains("could not start"), "{}", results[0]);
    assert_eq!(missing.code, 0, "{}\n{}", missing.turn, missing.stderr);
    assert!(
        results[1].contains(
            "the command could not start: No such file or directory (os error 2). It did not run"
        ),
        "a failed start's reason did not reach the model: {}",
        results[1]
    );
    assert!(!results[1].contains("(no output)"), "{}", results[1]);
}
