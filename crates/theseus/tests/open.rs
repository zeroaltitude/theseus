//! `theseus open turn` (theseus-x9swu): the link to a turn in the cockpit's
//! inspector, built by the daemon from `[cockpit] public_url` (so a relay's
//! port is the config's), printed, and opened in a browser unless `--print`.
//! A scripted daemon: the reads it makes are the session list, the turn list
//! and `turn.inspect`, and nothing is written.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};
use theseus_protocol::inspect::TurnInspectResult;

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const S: &str = "ses_reef42";
const T: &str = "trn_k3m9x2";

fn run(args: &[&str], script: Vec<(&'static str, Value)>) -> (String, String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen = Vec::new();
        for (method, result) in script {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap() == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            seen.push(format!(
                "{} {}",
                req["method"].as_str().unwrap(),
                req["params"]
            ));
            let answer = if req["method"] == method {
                json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
            } else {
                json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601, "message": format!("the script expected {method}")}})
            };
            (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        }
        seen
    });
    let out = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .args(args)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env("BROWSER", "true")
        .env("PATH", "")
        .output()
        .unwrap();
    let seen = daemon.join().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        seen,
    )
}

fn inspected(url: &str) -> Value {
    serde_json::to_value(TurnInspectResult {
        session_id: S.into(),
        turn_id: T.into(),
        url: url.into(),
        ..TurnInspectResult::default()
    })
    .unwrap()
}

fn sessions() -> Value {
    json!({"sessions": [{"session_id": S, "kind": "conversation", "label": null, "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1}]})
}

fn explained() -> Value {
    json!({"session_id": S, "kind": "conversation", "class": "private", "place": "p", "turns": [{"turn_id": T, "at_ms": 1, "loops": 2, "est_tokens": 9, "compilation_id": "c"}],
           "profile": "sonnet", "model": "m", "parts": [], "digest_now": "d", "recalls": [], "sources": {}, "ms": 1.0})
}

/// The session's last turn, by default: the link is what the daemon built, on
/// stdout alone, and the reads are the session list, the turn list and the inspect.
#[test]
fn the_last_turn_opens_at_the_link_the_daemon_built() {
    let url = format!("http://localhost:17433/session/{S}/turn/{T}");
    let (out, err, seen) = run(
        &["open", "turn", "--print"],
        vec![
            ("session.list", sessions()),
            ("context.explain", explained()),
            ("turn.inspect", inspected(&url)),
        ],
    );
    assert_eq!(out, format!("{url}\n"), "stderr: {err}");
    assert_eq!(seen.len(), 3, "{seen:?}");
    assert!(seen[2].starts_with("turn.inspect "), "{seen:?}");
    assert!(seen[2].contains(T) && seen[2].contains(S), "{seen:?}");
}

#[test]
fn json_is_the_ids_and_the_link() {
    let url = format!("http://localhost:7433/session/{S}/turn/{T}");
    let (out, _, _) = run(
        &["--json", "open", "turn", "--print"],
        vec![
            ("session.list", sessions()),
            ("context.explain", explained()),
            ("turn.inspect", inspected(&url)),
        ],
    );
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v, json!({"session_id": S, "turn_id": T, "url": url}));
}
