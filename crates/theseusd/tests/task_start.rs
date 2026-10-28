//! `theseus task start` on a real daemon (theseus-7oq0w): the real CLI against a
//! socket daemon and the stand-in model. A task started with no model turn runs
//! its first turn and finishes, its report waiting on the owner's Home; one
//! started with `--in-time` is asleep until then and starts then; quotes hold
//! its session as external text; and a second start finds the same Home.

mod common;

use std::process::Command;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Served;
use serde_json::{json, Value};

const KEY_VAR: &str = "THESEUS_TASK_START_TEST_KEY";

/// A daemon on the stand-in model, every secret from `KEY_VAR`.
fn served(model: &FakeModel) -> Served {
    let base = model.base.clone();
    Served::start_with(
        |_| {},
        move |t| {
            fn table<'a>(t: &'a mut toml::Table, k: &str) -> &'a mut toml::Table {
                t.entry(k)
                    .or_insert_with(|| toml::Value::Table(Default::default()))
                    .as_table_mut()
                    .unwrap()
            }
            table(t, "model").insert("api_base".into(), base.clone().into());
            for (_, p) in table(t, "providers").iter_mut() {
                p.as_table_mut()
                    .unwrap()
                    .insert("api_base".into(), base.clone().into());
            }
            for (_, v) in table(t, "secrets").iter_mut() {
                *v = format!("env:{KEY_VAR}").into();
            }
        },
        |cmd| {
            cmd.env(KEY_VAR, "tv-task-start-test-3b7c");
        },
    )
}

/// `theseus --socket SOCK ARGS…`: exit code, stdout, stderr.
fn cli(s: &Served, args: &[&str]) -> (i32, String, String) {
    let theseusd = std::path::PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--socket")
        .arg(s.path("sock"))
        .args(args)
        .env("XDG_STATE_HOME", s.path("xdg-state"))
        .env_remove("THESEUS_SESSION")
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The short id `theseus task start` printed: its first line's second word.
fn short_of(out: &str) -> String {
    out.split_whitespace().nth(1).unwrap().to_string()
}

fn task_state(s: &Served, short: &str) -> Value {
    let list = s.call("task.list", json!({})).unwrap();
    list["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["short"] == short)
        .cloned()
        .unwrap_or_else(|| panic!("no task {short} in {list}"))
}

fn until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "no {what} in {secs} s"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn a_task_started_from_the_cli_runs_with_no_model_turn_of_its_own() {
    let model = FakeModel::start(|_| vec![]);
    let s = served(&model);
    let (code, out, err) = cli(&s, &["task", "start", "Count the boats", "--budget", "0.5"]);
    assert_eq!(code, 0, "{err}\n{}", s.log());
    eprintln!("$ theseus task start \"Count the boats\" --budget 0.5\n{out}");
    assert!(out.contains("no model turn was spent"), "{out}");
    let short = short_of(&out);
    until("the task to finish", 30, || {
        task_state(&s, &short)["state"] == "complete"
    });
    let t = task_state(&s, &short);
    assert_eq!(t["limit_usd"], 0.5);
    // The one model request is the task's own turn, brief first.
    let reqs = model.requests();
    assert_eq!(reqs.len(), 1, "{reqs:?}");
    assert!(
        reqs[0].to_string().contains("Count the boats"),
        "the brief reached the model"
    );
    // Its report waits on its parent, the owner's Home.
    let home = t["parent_session_id"].as_str().unwrap().to_string();
    let (_, again, _) = cli(
        &s,
        &["task", "start", "Count the gulls", "--budget", "0.25"],
    );
    let short2 = short_of(&again);
    assert_eq!(task_state(&s, &short2)["parent_session_id"], home.as_str());
}

#[test]
fn a_task_started_for_later_sleeps_until_then() {
    let model = FakeModel::start(|_| vec![]);
    let s = served(&model);
    let t0 = Instant::now();
    let (code, out, err) = cli(
        &s,
        &["task", "start", "Count the boats later", "--in-time", "2s"],
    );
    assert_eq!(code, 0, "{err}\n{}", s.log());
    eprintln!("$ theseus task start \"Count the boats later\" --in-time 2s\n{out}");
    assert!(out.contains("asleep until"), "{out}");
    let short = short_of(&out);
    let t = task_state(&s, &short);
    assert_eq!(
        (t["state"].as_str(), t["turns"].as_u64()),
        (Some("waiting"), Some(0))
    );
    assert!(t["starts_at_ms"].is_u64(), "{t}");
    assert!(model.requests().is_empty(), "no turn before its time");
    until("the task to finish", 30, || {
        task_state(&s, &short)["state"] == "complete"
    });
    assert!(
        t0.elapsed() >= Duration::from_secs(2),
        "it waited for its time"
    );
    assert_eq!(model.requests().len(), 1);
}

/// A quote file reaches the task's model, and holds nothing: the content pack asks of it what it asks of
/// anything (R6, theseus-gyjyo.8; before it, the task's session held a quote an agent's hand had marked).
#[test]
fn a_quote_file_reaches_the_task_and_holds_its_session_nothing() {
    let model = FakeModel::start(|_| vec![]);
    let s = served(&model);
    let quote = s.path("ferry.log");
    std::fs::write(&quote, "ferry 7 was ten minutes late").unwrap();
    let (code, out, err) = cli(
        &s,
        &[
            "task",
            "start",
            "Explain the delay",
            "--quote-file",
            quote.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}\n{}", s.log());
    let short = short_of(&out);
    until("the task to finish", 30, || {
        task_state(&s, &short)["state"] == "complete"
    });
    let reqs = model.requests();
    assert!(reqs[0].to_string().contains("ferry 7 was ten minutes late"));
    let sid = task_state(&s, &short)["task_id"]
        .as_str()
        .unwrap()
        .to_string();
    let list = s.call("session.list", json!({})).unwrap();
    let rec = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["session_id"] == sid.as_str())
        .cloned()
        .unwrap_or_else(|| panic!("no session {sid} in {list}"));
    assert!(rec.get("external_text").is_none(), "{rec}");
}
