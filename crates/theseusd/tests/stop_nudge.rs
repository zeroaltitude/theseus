//! The stop nudge in a one-shot `ask` (theseus-fh0u; the owner's D8): the
//! real CLI spawns the real `theseusd --stdio --one-shot` on the stand-in
//! model and the fake Jev, scripted to say the last message announced work
//! still to come. The ask's first answer stops ("Nothing to do.", the
//! stand-in's text, which Jev reads as unfinished), the harness nudges it
//! once, and the second answer runs the tests; the run ends after it with
//! one more model call and no third. With `[judge] stop_nudge = false`, the
//! same ask ends at its first answer.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use common::model::FakeModel;
use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, Scripted as Jev};

const KEY_VAR: &str = "THESEUS_NUDGE_TEST_KEY";

/// The harness's line the model reads (`turn::nudge_step::NUDGE_TEXT`).
const NUDGE: &str = "[The harness: your last message said more work follows";

/// The stand-in's calls: after the harness's line, it runs the tests.
fn calls(prompt: &str) -> Vec<(&'static str, Value)> {
    if prompt.contains(NUDGE) {
        vec![("proc_run", json!({"argv": ["echo", "41 passed"]}))]
    } else {
        vec![]
    }
}

/// The safe template on the stand-in model, every secret from `KEY_VAR`,
/// the judge on at `jev` with `loop.v1` judging alone, and `stop_nudge`.
fn rig(model: &FakeModel, jev: &FakeJev, stop_nudge: bool) -> tempfile::TempDir {
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
    let j = table(&mut t, "judge");
    j.insert("enabled".into(), true.into());
    j.insert("api_base".into(), jev.base().into());
    j.insert("connect_secs".into(), 1.into());
    j.insert("total_secs".into(), 5.into());
    j.insert("stop_nudge".into(), stop_nudge.into());
    let packs = table(j, "packs");
    for p in ["classify.v4", "role.v1", "route.v9", "directive.v1"] {
        let mut off = toml::Table::new();
        off.insert("rung".into(), "off".into());
        packs.insert(p.into(), off.into());
    }
    std::fs::write(dir.path().join("config.toml"), toml::to_string(&t).unwrap()).unwrap();
    dir
}

/// `theseus --spawn <theseusd> --json ask PROMPT` in `dir`, to its end.
fn ask(dir: &Path, prompt: &str) -> (Value, String) {
    let theseusd = PathBuf::from(env!("CARGO_BIN_EXE_theseusd"));
    let out = Command::new(theseusd.with_file_name("theseus"))
        .arg("--spawn")
        .arg(&theseusd)
        .args(["--json", "ask", prompt])
        .current_dir(dir.join("projects"))
        .env("THESEUS_CONFIG", dir.join("config.toml"))
        .env("THESEUS_STATE_DIR", dir.join("state"))
        .env(KEY_VAR, "tv-nudge-5e1a")
        .env_remove("OP_SERVICE_ACCOUNT_TOKEN")
        .env_remove("THESEUS_OP_TOKEN_FILE")
        .env_remove("THESEUS_SOCKET")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    let v = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("stdout is not one JSON object ({e}): {stderr}"));
    (v, stderr)
}

fn unfinished() -> FakeJev {
    let jev = FakeJev::start().unwrap();
    jev.script("announced_unfinished", Jev::Noul(0.97));
    jev
}

/// The ask is nudged once and its second answer runs the tests: three model
/// calls (the stop, the tool call after the nudge, the answer to its
/// result), the result's `nudged` with `acted`, and the harness's line read
/// by the model as the last thing before its second answer.
#[test]
fn a_one_shot_ask_that_stops_early_is_nudged_once_and_runs_the_tests() {
    let model = FakeModel::start(calls);
    let jev = unfinished();
    let dir = rig(&model, &jev, true);
    let (v, stderr) = ask(dir.path(), "Check the parser and run its tests.");
    let reqs = model.requests();
    assert_eq!(reqs.len(), 3, "{stderr}\n{v}");
    assert!(
        reqs[1]["messages"].to_string().contains(NUDGE),
        "{}",
        reqs[1]["messages"]
    );
    assert!(reqs[2]["messages"].to_string().contains("41 passed"));
    assert_eq!(v["nudged"]["by"], "announced_unfinished", "{v}");
    assert_eq!(v["nudged"]["acted"], true, "{v}");
    assert_eq!(
        (v["loops"].as_u64(), v["tool_calls"].as_u64()),
        (Some(3), Some(1))
    );
}

/// The same ask with the switch off ends at its first answer.
#[test]
fn with_the_switch_off_the_ask_ends_at_its_first_answer() {
    let model = FakeModel::start(calls);
    let jev = unfinished();
    let dir = rig(&model, &jev, false);
    let (v, stderr) = ask(dir.path(), "Check the parser and run its tests.");
    assert_eq!(model.requests().len(), 1, "{stderr}\n{v}");
    assert!(v.get("nudged").is_none(), "{v}");
}
