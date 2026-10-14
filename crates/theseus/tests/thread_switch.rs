//! The current thread in a shell (theseus-j0yh): `thread switch` prints the line that
//! sets `THESEUS_THREAD`, remembers the choice for the terminal in a small file,
//! and `ask` reads only the environment, so a script that never set it asks the
//! home conversation whatever a terminal chose. A reply made in a thread carries
//! its tag.

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
                    thread("pr", "bh-ai pr round"),
                    thread("tides-gauge", "Tide gauge four"),
                    thread("tides-log", "Tide log rewrite")], "total": 3}),
                "thread.show" => thread("pr", "bh-ai pr round"),
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
    let o = run(&sock, &state, None, &["thread", "switch", "bh-ai"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "export THESEUS_THREAD=pr");
    assert!(
        err(&o).contains("current thread: bh-ai pr round"),
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

#[test]
fn ask_uses_the_thread_only_when_the_shell_exported_it() {
    // A terminal chose `pr`; a script without the variable asks home.
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let (sock, got) = daemon(dir.path());
    let switched = run(&sock, &state, None, &["thread", "switch", "pr"]);
    assert!(switched.status.success(), "{}", err(&switched));
    drop(got);
    let dir2 = tempfile::tempdir().unwrap();
    let (sock2, got2) = daemon(dir2.path());
    let o = run(&sock2, &state, None, &["ask", "--no-stream", "hello"]);
    assert!(o.status.success(), "{}", err(&o));
    let sent = got2.lock().unwrap().clone();
    assert_eq!(sent[0]["method"], "turn.submit");
    assert!(sent[0]["params"].get("thread").is_none(), "{}", sent[0]);

    // The shell that exported it asks the thread, and the reply wears its tag.
    let dir3 = tempfile::tempdir().unwrap();
    let (sock3, got3) = daemon(dir3.path());
    let o = run(&sock3, &state, Some("pr"), &["ask", "--no-stream", "hello"]);
    assert!(o.status.success(), "{}", err(&o));
    let sent = got3.lock().unwrap().clone();
    assert_eq!(sent[0]["params"]["thread"], "pr");
    assert!(sent[0]["params"]["session_id"].is_null());
    assert!(err(&o).contains("→ bh-ai pr round"), "{}", err(&o));

    // A named session wins over the variable: it continues that session.
    let dir4 = tempfile::tempdir().unwrap();
    let (sock4, got4) = daemon(dir4.path());
    let o = run(
        &sock4,
        &state,
        Some("pr"),
        &["ask", "--no-stream", "-s", "ses_0000ff9e8d7", "hello"],
    );
    assert!(o.status.success(), "{}", err(&o));
    let sent = got4.lock().unwrap().clone();
    assert!(sent[0]["params"].get("thread").is_none(), "{}", sent[0]);
    assert!(!err(&o).contains("→ bh-ai pr round"));
}
