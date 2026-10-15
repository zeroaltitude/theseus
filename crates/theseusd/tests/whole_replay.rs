//! A whole-turn replay on a real daemon (own-benchmark part (c),
//! theseus-pw1q.20): a stand-in coding turn (an edit, then a check that
//! reads the edit) is replayed with `turn.replay {scope: "whole_turn"}` on a
//! second profile. The real directory is the same bytes after; the row names
//! each call, the paths changed and the test result.
//!
//! The program runs in L1, which refuses a root daemon's job
//! (theseus-pv6i): as root the test checks that the call says so and that
//! nothing ran outside L1 instead; unprivileged it checks the check ran, in
//! the copy, and passed.

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use common::model::FakeModel;
use common::Served;
use serde_json::{json, Value};

const ASK: &str = "Make the greeting say beta and run the check.";

/// Every file under `dir`, by relative path.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, at: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for e in std::fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.insert(
                    p.strip_prefix(base).unwrap().display().to_string(),
                    std::fs::read(&p).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[test]
fn a_whole_turn_replay_on_a_real_daemon_leaves_the_directory_as_it_was() {
    let root = unsafe { libc::geteuid() } == 0;
    let work_cell = std::sync::Arc::new(std::sync::OnceLock::<std::path::PathBuf>::new());
    let cell = work_cell.clone();
    let model = FakeModel::start(move |prompt| {
        if !prompt.starts_with("Make the greeting") {
            return Vec::new();
        }
        let work = cell.get().expect("the directory").clone();
        vec![
            (
                "fs_edit",
                json!({"path": work.join("lib.txt"), "old_string": "alpha", "new_string": "beta"}),
            ),
            (
                "proc_run",
                json!({"argv": ["sh", "check.sh", "test"], "cwd": work}),
            ),
        ]
    });
    let base = model.base.clone();
    let opbin = std::sync::OnceLock::<std::path::PathBuf>::new();
    let s = Served::start_with(
        |dir| {
            let work = dir.join("projects/proj");
            std::fs::create_dir_all(&work).unwrap();
            std::fs::create_dir_all(dir.join("opbin")).unwrap();
            std::fs::write(work.join("lib.txt"), "greeting: alpha\n").unwrap();
            std::fs::write(work.join("check.sh"), "grep -q beta lib.txt\n").unwrap();
            work_cell.set(work.canonicalize().unwrap()).unwrap();
            // A vault that answers every reference with a stand-in key.
            let op = dir.join("opbin/op");
            std::fs::write(
                &op,
                "#!/bin/sh\ncase \"$1\" in\n inject) sed -e 's/{{ [^}]* }}/test-secret-value-0000/g' ;;\n read) printf '%s' test-secret-value-0000 ;;\n *) exit 1 ;;\nesac\n",
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&op, std::fs::Permissions::from_mode(0o755)).unwrap();
            opbin.set(dir.join("opbin")).unwrap();
        },
        |t| {
            t.entry("model")
                .or_insert_with(|| toml::Value::Table(Default::default()))
                .as_table_mut()
                .unwrap()
                .insert("api_base".into(), base.clone().into());
            for (_, p) in t["providers"].as_table_mut().unwrap().iter_mut() {
                p.as_table_mut()
                    .unwrap()
                    .insert("api_base".into(), base.clone().into());
            }
            t["policy"]
                .as_table_mut()
                .unwrap()
                .insert("enforcement".into(), "notify".into());
        },
        |cmd: &mut Command| {
            // The stand-in `op` first, in front of the rig's refusing one.
            let path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{}:{path}", opbin.get().unwrap().display()));
        },
    );
    let work = work_cell.get().unwrap().clone();
    let sid = s
        .call(
            "session.open",
            json!({"label": ASK, "dir": work.display().to_string()}),
        )
        .unwrap()["session_id"]
        .clone();
    let res = s
        .call(
            "turn.submit",
            json!({"session_id": sid, "input": ASK, "author": "test", "attachments": []}),
        )
        .unwrap_or_else(|e| panic!("{e}\n{}", s.log()));
    assert_eq!(
        std::fs::read_to_string(work.join("lib.txt")).unwrap(),
        "greeting: beta\n"
    );
    let turn = res["turn_id"].as_str().unwrap().to_string();
    let before = snapshot(&work);

    let r = s
        .call(
            "turn.replay",
            json!({"session": sid, "turn": turn, "profile": "haiku", "scope": "whole_turn", "compare": false}),
        )
        .unwrap_or_else(|e| panic!("{e}\n{}", s.log()));
    assert_eq!(snapshot(&work), before, "the real directory is untouched");
    let whole = &r["whole"];
    let calls: Vec<(&str, &str)> = whole["calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["tool"].as_str().unwrap(), c["how"].as_str().unwrap()))
        .collect();
    assert_eq!(calls[0], ("fs.edit", "ran"), "{r}");
    assert_eq!(whole["changed"], json!(["lib.txt"]), "{r}");
    assert_eq!(whole["original_changed"], json!(["lib.txt"]));
    assert_eq!(whole["same"]["files"], true);
    if root {
        // L1 refuses a root daemon's job: the call says so, and the check did
        // not run anywhere else.
        assert_eq!(calls[1], ("proc.run", "refused"), "{r}");
        assert_eq!(whole["tests"][0]["exit"], Value::Null);
    } else {
        assert_eq!(calls[1], ("proc.run", "ran"), "{r}");
        assert_eq!(whole["tests"][0]["exit"], 0, "{r}");
        assert_eq!(whole["same"]["tests"], true);
    }
    // The row is in the ledger, with the same words.
    let tail = s.call("ledger.tail", json!({"n": 500})).unwrap();
    let row = tail["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "turn.replayed")
        .expect("a turn.replayed row");
    assert_eq!(row["data"]["scope"], "whole_turn");
    assert_eq!(row["data"]["whole"]["changed"], json!(["lib.txt"]));
    drop(model);
}
