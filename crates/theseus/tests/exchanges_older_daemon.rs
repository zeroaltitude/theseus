//! `theseus ledger exchanges` against a daemon older than its answers (issue 44): the real binary on a
//! scripted daemon's socket. Such a daemon's groups can carry a task's id, and it never read `--since`,
//! so the command prints neither its groups nor anything that looks like a filtered report: one line
//! saying to update it, and a failing exit, in text and in `--json`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// An `exchange.report` result exactly as the daemon before the answers wrote it: no `answers`, and
/// the judge's task answer by its id.
fn older_result() -> Value {
    json!({
        "rows": 3, "from_ms": 1_000, "to_ms": 3_000,
        "counts": {
            "judge.addressed_task": {"task_PRIVATE_Q": 1, "none": 2},
            "prev": {"missed": 1, "transcript": 2}
        },
        "tokens": {"header": 300},
        "places_bound": {"dm": 1}
    })
}

/// A daemon on `sock` that answers one connection, recording what `exchange.report` was asked.
fn daemon(sock: &Path) -> (std::thread::JoinHandle<()>, Arc<Mutex<Vec<Value>>>) {
    let listener = UnixListener::bind(sock).unwrap();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let h = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let req: Value = serde_json::from_str(&line).unwrap();
            line.clear();
            let result = match req["method"].as_str().unwrap() {
                "exchange.report" => {
                    seen.lock().unwrap().push(req["params"].clone());
                    older_result()
                }
                _ => json!({}),
            };
            let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
            if (&s).write_all(format!("{answer}\n").as_bytes()).is_err() {
                break;
            }
        }
    });
    (h, asked)
}

#[test]
fn an_older_daemon_gets_one_line_to_update_it_and_a_failing_exit() {
    for json in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("sock");
        let (d, asked) = daemon(&sock);
        let mut c = Command::new(THESEUS);
        c.arg("--socket").arg(&sock);
        if json {
            c.arg("--json");
        }
        let out = c
            .args(["ledger", "exchanges", "--since", "2026-11-01"])
            .env_remove("THESEUS_SOCKET")
            .env_remove("THESEUS_SESSION")
            .env_remove("TMUX")
            .env("XDG_STATE_HOME", dir.path().join("state"))
            .env("TZ", "America/Phoenix")
            .output()
            .unwrap();
        d.join().unwrap();
        let (stdout, stderr) = (
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        assert!(!out.status.success(), "json {json}: {:?}", out.status);
        assert_eq!(stdout, "", "json {json}");
        assert_eq!(
            stderr, "theseus: this daemon does not report the answers; update it\n",
            "json {json}"
        );
        // It was asked for the window, and said nothing of the rows it was not asked about.
        let asked = asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "json {json}: {asked:?}");
        assert!(asked[0]["since_ms"].as_u64().is_some(), "{asked:?}");
    }
}
