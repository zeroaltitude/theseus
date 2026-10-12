//! Golden output for `theseus fork` (theseus-2qzx, R19): what a fork prints,
//! the fork's history with its inherited prefix marked as the source's, and
//! `theseus reach` listing a fork. Each runs the real binary against a
//! scripted daemon on a Unix socket and compares its requests, stdout,
//! stderr and exit code with `tests/golden/fork_*.txt`
//! (`THESEUS_GOLDEN=write` rewrites them). Kept apart from golden.rs, which
//! is at its line ceiling.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
/// The zone every golden's times are written in: UTC−7, all year.
const GOLDEN_TZ: &str = "<-07>7";

const SOURCE: &str = "ses_0198d3a0b1c2d3e4f5a6b7c8d9e06664";
const FORK: &str = "ses_0198d3a0b1c2d3e4f5a6b7c8d9e0f00d";
const AT: &str = "msg_0198d3a0b1c2d3e4f5a6b7c8d9a1b2c3";

/// Run `theseus --socket <sock> ARGS` against a daemon that answers each
/// request in order with the next of `answers`, and closes after the last.
fn run(args: &[&str], answers: Vec<Value>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        let mut reader = BufReader::new(&s);
        for answer in answers {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            seen.push(format!(
                "{} {}",
                req["method"].as_str().unwrap_or("?"),
                req["params"]
            ));
            let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": answer});
            (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
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
        .env("TZ", GOLDEN_TZ)
        .output()
        .unwrap();
    let seen = daemon.join().unwrap();
    format!(
        "$ theseus {}\n--- requests\n{}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
        seen.join("\n"),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
        out.status.code().unwrap_or(-1)
    )
}

fn golden(name: &str, got: &str) {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "golden",
        &format!("{name}.txt"),
    ]
    .iter()
    .collect();
    if std::env::var("THESEUS_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e} (write it with THESEUS_GOLDEN=write)",
            path.display()
        )
    });
    assert_eq!(
        got,
        want,
        "{name}: the output differs from {}",
        path.display()
    );
}

fn forked() -> Value {
    json!({
        "session_id": FORK, "title": "refactor the writer (fork)", "source_session": SOURCE,
        "at_node": AT, "at_position": 166_201, "compilation_id": "cmp_f1",
        "derived_from": "cmp_a9", "bytes": 1_433, "ms": 3,
        "not_restored": "files and what programs did after the fork point (they ran in the original)"
    })
}

/// A fork of the session that holds the node: the node's session is read
/// first (its reach at generation 0), then the fork is made.
#[test]
fn a_fork_prints_where_it_forked_and_what_it_does_not_restore() {
    let reach = json!({
        "node_id": AT, "session_id": SOURCE, "position": 166_201,
        "direct": {"compilations": [], "loops": 0},
        "descendants": [], "totals": {"contexts": 0, "sessions": 1}, "partial": false});
    golden("fork", &run(&["fork", "msg·a1b2c3"], vec![reach, forked()]));
    golden(
        "fork_flags",
        &run(
            &[
                "fork",
                "msg·a1b2c3",
                "--session",
                SOURCE,
                "--title",
                "refactor the writer (fork)",
                "--budget",
                "2.5",
            ],
            vec![forked()],
        ),
    );
    golden(
        "fork_json",
        &run(
            &["--json", "fork", "msg·a1b2c3", "--session", SOURCE],
            vec![forked()],
        ),
    );
}

fn node(session: &str, kind: &str, at: u64, text: &str) -> Value {
    json!({"node_id": format!("msg_{at:032x}"), "kind": kind, "session_id": session,
           "position": at, "at_unix_ms": 1_759_300_000_000u64 + at * 1_000,
           "turn_id": "turn_m4p8z1", "text": text, "detail": Value::Null, "bytes": text.len()})
}

/// A fork's history: the source's nodes through the fork point, marked as
/// from the source, then the fork's own turns.
#[test]
fn a_forks_history_marks_the_inherited_prefix() {
    let h = json!({
        "session": {"session_id": FORK, "kind": "conversation", "label": null,
                    "created_at_unix_ms": 1_759_300_900_000u64, "turns": 1,
                    "usage": {"input_tokens": 100, "output_tokens": 20},
                    "execution_id": "exe_f00d", "execution_state": "waiting",
                    "last_active_ms": 1_759_300_950_000u64, "cost_usd": 0.001,
                    "tool_calls": 0, "model": "glm-x", "title": "refactor the writer (fork)"},
        "nodes": [
            node(SOURCE, "user_message", 10, "Refactor the writer."),
            node(SOURCE, "assistant_message", 11, "Done: it now streams."),
            node(FORK, "user_message", 20, "Take the other road: batch it."),
            node(FORK, "assistant_message", 21, "Batched."),
        ],
        "pending_confirms": []
    });
    // What it showed is then recorded: one `session.wait`, answered at once.
    let settled = json!({"reached": "settled", "already": true, "confirms": []});
    golden("fork_history", &run(&["history", FORK], vec![h, settled]));
}

/// `theseus reach` of a source node lists the fork, generation by
/// generation, beside its copies.
#[test]
fn reach_lists_a_fork_and_a_fork_of_it() {
    let reach = json!({
        "node_id": AT, "session_id": SOURCE, "position": 166_201,
        "direct": {"compilations": [], "loops": 1, "first_ms": 1_759_300_000_001u64,
                   "last_ms": 1_759_300_000_001u64},
        "descendants": [],
        "forks": [
            {"session_id": FORK, "compilation_id": "cmp_f1", "generation": 1, "from": SOURCE,
             "at_position": 166_201,
             "compilations": [{"compilation_id": "cmp_f1", "strategy": "transcript",
                               "created_at_ms": 1_759_300_100_000u64}],
             "loops": 2, "first_ms": 1_759_300_100_000u64, "last_ms": 1_759_300_160_000u64},
            {"session_id": "ses_0198d3a0b1c2d3e4f5a6b7c8d9e0beef", "compilation_id": "cmp_f2",
             "generation": 2, "from": FORK, "at_position": 166_300,
             "compilations": [], "loops": 0}],
        "totals": {"contexts": 4, "sessions": 3}, "partial": false});
    golden("fork_reach", &run(&["reach", AT], vec![reach]));
}
