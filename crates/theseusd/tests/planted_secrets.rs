//! The planted-secret corpus end to end (theseus-oyrt): a scratch daemon's
//! model asks for `cat` of a file holding one invented secret of every shape
//! the scrubber knows, each in every encoding it claims, and of a file of
//! ordinary output. No byte of any secret reaches the request the stand-in
//! model received, any file under the state dir (the WAL, the index, the
//! spool, so the ledger and the spans too), the daemon's log, or the
//! session's history; the ordinary output arrives as it was printed.

mod common;
#[path = "../../theseus-core/src/scrub/corpus.rs"]
mod corpus;

use std::path::Path;
use std::time::{Duration, Instant};

use common::model::FakeModel;
use common::Served;
use serde_json::{json, Value};

const SEED: u64 = 0x0E2E_5EED;

/// Every file at or under `at`, its bytes joined, read as text.
fn everything_under(at: &Path) -> String {
    let mut all = Vec::new();
    let mut stack = vec![at.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(b) = std::fs::read(&p) {
                all.extend_from_slice(&b);
                all.push(b'\n');
            }
        }
    }
    String::from_utf8_lossy(&all).into_owned()
}

#[test]
fn no_planted_secret_reaches_any_sink() {
    let planted = corpus::planted(SEED);
    let clean = corpus::clean(SEED);
    let planted_text: String = planted.iter().map(|p| p.text.as_str()).collect();
    let clean_text: String = clean.iter().map(|(_, t)| t.as_str()).collect();
    let shapes: Vec<&str> = corpus::secrets(SEED).iter().map(|s| s.shape).collect();

    let (mut d, model) = daemon(&planted_text, &clean_text);
    let (results, history, ledger) = turn(&d, &model);
    assert_eq!(d.call("shutdown", Value::Null), Ok(json!({"ok": true})));
    let t0 = Instant::now();
    while d.daemon.try_wait().is_none() {
        assert!(
            t0.elapsed() < Duration::from_secs(15),
            "the daemon never stopped"
        );
        std::thread::sleep(Duration::from_millis(20));
    }

    let requests: String = model
        .raw_requests()
        .iter()
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let state = everything_under(&d.path("state"));
    let log = d.log();
    let sinks = [
        ("the model's requests", &requests),
        ("the state dir", &state),
        ("the log", &log),
        ("the history", &history),
        ("the ledger", &ledger),
        ("the results", &results),
    ];
    // What every sink must show: the stand-ins reached the model and the
    // store, which is read as text (so a grep of it finds what it holds).
    for shape in &shapes {
        let marker = format!("[redacted:{shape}]");
        assert!(
            requests.contains(&marker),
            "{marker} never reached the model"
        );
        assert!(
            state.contains(&marker),
            "{marker} is not in the store as text"
        );
    }
    let looked = assert_no_leak(&planted, &sinks);
    // The ordinary output came through as printed.
    for (what, text) in &clean {
        assert!(
            results.contains(text.trim_end()),
            "{what} was changed:\n{results}"
        );
    }
    eprintln!(
        "planted: {} texts, {} pieces looked for in {} sinks ({} bytes of state), none found",
        planted.len(),
        looked,
        sinks.len(),
        state.len()
    );
}

/// A scratch daemon whose stand-in model asks for `cat` of the two files,
/// laid out in its projects dir.
fn daemon(planted_text: &str, clean_text: &str) -> (Served, FakeModel) {
    let model = FakeModel::start(|prompt| {
        if prompt.contains("show me the files") {
            vec![
                (
                    "proc_run",
                    json!({"argv": ["cat", "planted.txt"], "timeout_secs": 30}),
                ),
                (
                    "proc_run",
                    json!({"argv": ["cat", "clean.txt"], "timeout_secs": 30}),
                ),
            ]
        } else {
            vec![]
        }
    });
    let base = model.base.clone();
    // Every secret the config names, from a file only its owner reads: the
    // stand-in `op` answers nothing.
    let key_file = std::cell::RefCell::new(std::path::PathBuf::new());
    let d = Served::start(
        |dir| {
            use std::os::unix::fs::PermissionsExt;
            let key = dir.join("provider-key");
            std::fs::write(&key, "tv-provider-key-invented").unwrap();
            std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
            *key_file.borrow_mut() = key;
            std::fs::create_dir_all(dir.join("projects")).unwrap();
            std::fs::write(dir.join("projects/planted.txt"), planted_text).unwrap();
            std::fs::write(dir.join("projects/clean.txt"), clean_text).unwrap();
        },
        |t| {
            let table = |t: &mut toml::Table, key: &str| -> toml::Table {
                t.get(key)
                    .and_then(|v| v.as_table())
                    .cloned()
                    .unwrap_or_default()
            };
            let mut m = table(t, "model");
            m.insert("api_base".into(), base.clone().into());
            t.insert("model".into(), m.into());
            let mut providers = table(t, "providers");
            for (_, p) in providers.iter_mut() {
                p.as_table_mut()
                    .unwrap()
                    .insert("api_base".into(), base.clone().into());
            }
            t.insert("providers".into(), providers.into());
            let mut secrets = table(t, "secrets");
            for (_, v) in secrets.iter_mut() {
                *v = format!("file:{}", key_file.borrow().display()).into();
            }
            t.insert("secrets".into(), secrets.into());
            let mut policy = table(t, "policy");
            policy.insert("enforcement".into(), "notify".into());
            t.insert("policy".into(), policy.into());
            // The whole output reaches the model, so the test reads it all.
            let mut tools = table(t, "tools");
            tools.insert("result_max_chars".into(), 4_000_000.into());
            t.insert("tools".into(), tools.into());
        },
    );

    (d, model)
}

/// No piece of any planted secret in any sink; how many pieces were looked for.
fn assert_no_leak(planted: &[corpus::Planted], sinks: &[(&str, &String)]) -> usize {
    let mut leaks = Vec::new();
    let mut looked = 0;
    for p in planted {
        for piece in &p.carries {
            looked += 1;
            for (sink, text) in sinks {
                if text.contains(piece.as_str()) {
                    leaks.push(format!("{} / {} in {sink}: {piece}", p.shape, p.encoding));
                }
            }
        }
    }
    assert!(
        leaks.is_empty(),
        "{} leaks:\n{}",
        leaks.len(),
        leaks.join("\n")
    );
    looked
}

/// The turn: its two results once both are in and the turn has ended, the
/// session's history, and the ledger's tail.
fn turn(d: &Served, model: &FakeModel) -> (String, String, String) {
    let s = d.call("session.open", json!({"label": "corpus"})).unwrap();
    let session = s["session_id"].clone();
    d.call(
        "turn.submit",
        json!({"session_id": session, "input": "show me the files", "author": "test", "attachments": []}),
    )
    .unwrap();
    let t0 = Instant::now();
    let results = loop {
        let nodes = d
            .call(
                "node.list",
                json!({"session_id": session, "kind": "tool_result"}),
            )
            .unwrap();
        let texts: Vec<String> = nodes["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n["text"].as_str().map(str::to_string))
            .collect();
        if texts.len() == 2 && model.requests().len() >= 2 {
            break texts.join("\n");
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "no results in 60 s:\n{}",
            d.log()
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    // Let the turn end, so its last frames (the trace's spans among them)
    // are written before the store is read.
    let t0 = Instant::now();
    while d
        .call("session.list", Value::Null)
        .unwrap()
        .to_string()
        .contains("\"busy\":true")
    {
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "the turn never ended"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let history = d
        .call("session.history", json!({"session_id": session}))
        .unwrap()
        .to_string();
    let ledger = d
        .call("ledger.tail", json!({"n": 5000}))
        .unwrap()
        .to_string();
    (results, history, ledger)
}
