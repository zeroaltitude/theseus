//! Golden output for `theseus profile unpin [--all]` and `theseus profile pins` (theseus-h66v4.2): the
//! counts before and after come off the wire as numbers, and the lines say what was let go; the pin and the
//! hold on one name (`quick`) still read as cleared; a hold that could not be let go exits 1. Each runs the
//! real binary against a scripted daemon on a Unix socket and compares its request, stdout, stderr and exit
//! code with `tests/golden/profile_*.txt` (`THESEUS_GOLDEN=write` rewrites them). Kept apart from
//! golden.rs, which is at its line ceiling.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
/// The zone every golden's times are written in: UTC−7, all year.
const GOLDEN_TZ: &str = "<-07>7";

/// Run `theseus --socket <sock> ARGS` against a daemon that answers `result` to its one request.
fn run(args: &[&str], result: Value) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        req["method"].as_str().unwrap_or("?").to_string()
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
        "$ theseus {}\n--- requests\n{seen}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
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

/// A `profile.pins` answer over 5 sessions and 3 tasks, holding `pinned`.
fn pins(pinned: &[Value]) -> Value {
    let of = |kind: &str| pinned.iter().filter(|p| p["kind"] == kind).count();
    json!({"live": "quick", "live_source": "config", "sessions": 5, "tasks": 3,
           "pinned_sessions": of("session"), "pinned_tasks": of("task"), "pins": pinned})
}

fn held() -> Vec<Value> {
    vec![
        json!({"session_id": "ses_q7f3k2", "kind": "session", "profile": "quick", "configured": true}),
        json!({"session_id": "ses_tasktasktask01", "kind": "task", "profile": "gone", "configured": false}),
    ]
}

/// A `profile.unpin` answer: the live pin and the hold on one name (`quick`), both released.
fn unpinned(all: bool, cleared: &[Value], failed: Vec<Value>, after: &[Value]) -> Value {
    json!({"released": true, "previous": "quick", "live": "quick", "by": "the CLI", "all": all,
           "before": pins(&held()), "cleared": cleared, "failed": failed, "after": pins(after)})
}

#[test]
fn pins_counts_what_is_held_beside_the_totals() {
    golden("profile_pins", &run(&["profile", "pins"], pins(&held())));
}

#[test]
fn unpin_all_says_what_it_let_go_and_the_counts_after() {
    let answer = unpinned(true, &held(), vec![], &[]);
    golden(
        "profile_unpin_all",
        &run(&["profile", "unpin", "--all"], answer.clone()),
    );
    golden(
        "profile_unpin_all_json",
        &run(&["--json", "profile", "unpin", "--all"], answer),
    );
}

#[test]
fn unpin_without_all_says_how_many_holds_remain() {
    golden(
        "profile_unpin_holds_remain",
        &run(&["profile", "unpin"], unpinned(false, &[], vec![], &held())),
    );
}

#[test]
fn a_hold_that_was_not_let_go_is_named_and_the_exit_is_one() {
    let failed = vec![json!({"session_id": "ses_tasktasktask01", "error": "refused"})];
    golden(
        "profile_unpin_all_failed",
        &run(
            &["profile", "unpin", "--all"],
            unpinned(true, &held()[..1], failed, &held()[1..]),
        ),
    );
}
