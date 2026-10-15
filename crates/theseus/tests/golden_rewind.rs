//! Golden output for `theseus rewind` (theseus-23rk, R20): the files put back
//! and the conversation forked, the files alone, the refusal of a first
//! message, and an undo. Each runs the real binary against a
//! scripted daemon on a Unix socket and compares its requests, stdout,
//! stderr and exit code with `tests/golden/rewind_*.txt`
//! (`THESEUS_GOLDEN=write` rewrites them). Kept apart from golden.rs.

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

const NODE: &str = "msg_0198d3a0b1c2d3e4f5a6b7c8d9a1b2c3";
const BEFORE: &str = "asm_0198d3a0b1c2d3e4f5a6b7c8d9a1b2b0";

fn reach() -> Value {
    json!({
        "node_id": NODE, "session_id": SOURCE, "position": 166_201,
        "direct": {"compilations": [], "loops": 0},
        "descendants": [], "totals": {"contexts": 0, "sessions": 1}, "partial": false})
}

fn point(fork_at: Option<&str>) -> Value {
    let mut p = json!({
        "session_id": SOURCE, "note": "n",
        "points": [{
            "node_id": NODE, "position": 166_201, "at_ms": 1_790_000_000_000u64,
            "text": "add a comment line at the top of tides.py",
            "message": "add a comment line at the top of tides.py",
            "files": [{"path": "/work/tides.py", "added": 1, "removed": 1}],
            "commands": 0}]});
    if let Some(f) = fork_at {
        p["points"][0]["fork_at"] = json!(f);
    }
    p
}

fn restored() -> Value {
    json!({
        "session_id": SOURCE, "since_node": NODE, "since_position": 166_201,
        "dry_run": false, "commands": 2, "undo_since": 166_900,
        "files": [
            {"path": "/work/tides.py", "outcome": "restored"},
            {"path": "/work/notes.md", "outcome": "skipped",
             "why": "changed since: it no longer holds what the last edit wrote, so it was not restored"}],
        "words": "Restored 1 file; 1 skipped, with why. 2 commands ran after that message: the files they changed are not restored"
    })
}

fn forked() -> Value {
    json!({
        "session_id": FORK, "title": "refactor the writer (fork)", "source_session": SOURCE,
        "at_node": BEFORE, "at_position": 166_100, "compilation_id": "cmp_f1",
        "bytes": 1_433, "ms": 3,
        "not_restored": "files and what programs did after the fork point (they ran in the original)"
    })
}

#[test]
fn a_rewind_puts_back_the_files_and_forks_before_the_message() {
    golden(
        "rewind",
        &run(
            &["rewind", "--to", "msg·a1b2c3"],
            vec![reach(), point(Some(BEFORE)), restored(), forked()],
        ),
    );
    golden(
        "rewind_files_only",
        &run(
            &["rewind", SOURCE, "--to", NODE, "--files-only"],
            vec![point(Some(BEFORE)), restored()],
        ),
    );
    golden(
        "rewind_json",
        &run(
            &[
                "--json",
                "rewind",
                SOURCE,
                "--to",
                NODE,
                "--conversation-only",
            ],
            vec![point(Some(BEFORE)), forked()],
        ),
    );
}

#[test]
fn the_first_message_has_nothing_before_it_and_nothing_is_restored() {
    golden(
        "rewind_first",
        &run(&["rewind", SOURCE, "--to", NODE], vec![point(None)]),
    );
}

#[test]
fn an_undo_rewinds_to_a_position_and_only_the_files() {
    let mut undone = restored();
    undone["files"] = json!([{"path": "/work/tides.py", "outcome": "restored"}]);
    undone["words"] = json!("Restored 1 file");
    golden(
        "rewind_undo",
        &run(
            &["rewind", SOURCE, "--to", "@166900", "--files-only"],
            vec![undone],
        ),
    );
    golden(
        "rewind_undo_needs_files_only",
        &run(&["rewind", SOURCE, "--to", "@166900"], vec![]),
    );
}
