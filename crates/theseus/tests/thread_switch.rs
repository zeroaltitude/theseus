//! The current thread in a shell (theseus-j0yh): `thread switch` prints the line that
//! sets `THESEUS_THREAD` and remembers the choice for the terminal in a small file.
//! `ask` sends no thread any more (threads are gone, 2026-11-22), and says so.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

fn thread(slug: &str, title: &str) -> Value {
    json!({"thread": {"slug": slug, "title": title, "state": "active",
        "last_active_ms": 1, "created_at_ms": 1}, "today_usd": 0.0})
}

fn turn_result() -> Value {
    json!({"session_id": "ses_0000ff9e8d7", "turn_id": "trn_1", "loops": 1, "output": "Done.",
        "stop_reason": "end_turn", "provider_stop_reason": null, "model": "invented-model",
        "usage": {"input_tokens": 1, "output_tokens": 1}, "elapsed_ms": 5})
}

/// A daemon that answers one connection until it closes; every request kept.
fn daemon(dir: &Path) -> (PathBuf, Arc<Mutex<Vec<Value>>>) {
    let sock = dir.join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let got = Arc::new(Mutex::new(Vec::new()));
    let g = got.clone();
    std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        for line in BufReader::new(&s).lines() {
            let Ok(line) = line else { break };
            let req: Value = serde_json::from_str(&line).unwrap();
            let result = match req["method"].as_str().unwrap() {
                "thread.list" => json!({"threads": [
                    thread("pr", "ex-ai pr round"),
                    thread("tides-gauge", "Tide gauge four"),
                    thread("tides-log", "Tide log rewrite")], "total": 3}),
                "thread.show" => thread("pr", "ex-ai pr round"),
                "turn.submit" => turn_result(),
                _ => json!({}),
            };
            let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            g.lock().unwrap().push(req);
            if (&s).write_all(format!("{answer}\n").as_bytes()).is_err() {
                break;
            }
        }
    });
    (sock, got)
}

fn run(sock: &Path, state: &Path, env: Option<&str>, args: &[&str]) -> Output {
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket")
        .arg(sock)
        .args(args)
        .env("XDG_STATE_HOME", state)
        .env_remove("THESEUS_SESSION");
    match env {
        Some(e) => cmd.env("THESEUS_THREAD", e),
        None => cmd.env_remove("THESEUS_THREAD"),
    };
    cmd.output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn switch_resolves_a_name_a_number_or_a_start_and_prints_the_export() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let (sock, _) = daemon(dir.path());
    let o = run(&sock, &state, None, &["thread", "switch", "ex-ai"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "export THESEUS_THREAD=pr");
    assert!(
        err(&o).contains("current thread: ex-ai pr round"),
        "{}",
        err(&o)
    );
    // The terminal's memory: a file beside the seen file, not the store.
    let kept: Value =
        serde_json::from_str(&std::fs::read_to_string(state.join("theseus/thread.json")).unwrap())
            .unwrap();
    assert_eq!(kept.as_object().unwrap().values().next().unwrap(), "pr");
}

#[test]
fn switch_by_number_and_home_and_the_refusals() {
    for (given, line) in [
        ("3", "export THESEUS_THREAD=tides-log"),
        ("tides-g", "export THESEUS_THREAD=tides-gauge"),
        ("home", "unset THESEUS_THREAD"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (sock, _) = daemon(dir.path());
        let o = run(
            &sock,
            &dir.path().join("state"),
            None,
            &["thread", "switch", given],
        );
        assert_eq!(out(&o), line, "{given}: {}", err(&o));
    }
    for (given, why) in [
        ("tide", "names 2 threads"),
        ("zzzzzz", "no thread is named"),
        ("pr1", "too short"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (sock, _) = daemon(dir.path());
        let o = run(
            &sock,
            &dir.path().join("state"),
            None,
            &["thread", "switch", given],
        );
        assert_eq!(o.status.code(), Some(1), "{given}");
        assert!(err(&o).contains(why), "{given}: {}", err(&o));
        assert!(out(&o).is_empty(), "nothing is exported on a refusal");
    }
}

/// Threads are gone (2026-11-22): `ask` sends no thread, whether one is named with `--thread` or
/// exported in the shell, and says so in one line; an `ask` that names none says nothing of it.
#[test]
fn ask_sends_no_thread_and_says_threads_are_gone() {
    const GONE: &str = "threads are gone; this ran here";
    let state = tempfile::tempdir().unwrap();
    for (env, args) in [
        (None, vec!["ask", "--no-stream", "--thread", "pr", "hello"]),
        (Some("pr"), vec!["ask", "--no-stream", "hello"]),
        (None, vec!["ask", "--no-stream", "hello"]),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (sock, got) = daemon(dir.path());
        let o = run(&sock, state.path(), env, &args);
        assert!(o.status.success(), "{args:?}: {}", err(&o));
        let sent = got.lock().unwrap().clone();
        assert_eq!(sent[0]["method"], "turn.submit");
        assert!(sent[0]["params"]["thread"].is_null(), "{}", sent[0]);
        let named = env.is_some() || args.contains(&"--thread");
        assert_eq!(err(&o).contains(GONE), named, "{args:?}: {}", err(&o));
    }
}
