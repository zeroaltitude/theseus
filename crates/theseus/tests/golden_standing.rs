//! Golden output for `theseus standing` (theseus-a7vi): the list with its
//! manifests' tally, an add, a revoke, and the refusal of a write inside a
//! job. Each runs the real binary against a scripted daemon on a Unix socket
//! and compares its requests, stdout, stderr and exit code with
//! `tests/golden/standing_*.txt` (`THESEUS_GOLDEN=write` rewrites them). Kept
//! apart from golden.rs, which is at its line ceiling.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
/// The zone every golden's times are written in: UTC−7, all year.
const GOLDEN_TZ: &str = "<-07>7";

/// Run `theseus --socket <sock> ARGS` against a daemon that answers `answer`
/// to its one request (none when `answer` is none), inside the job `job`
/// names when it does.
fn run(args: &[&str], answer: Option<Value>, job: Option<&str>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        let mut reader = BufReader::new(&s);
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return (seen, Value::Null);
        }
        let req: Value = serde_json::from_str(&line).unwrap();
        seen.push(req["method"].as_str().unwrap_or("?").to_string());
        let a = json!({"jsonrpc": "2.0", "id": req["id"], "result": answer.unwrap_or(Value::Null)});
        (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        (seen, req["params"].clone())
    });
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket")
        .arg(&sock)
        .args(args)
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .env_remove("THESEUS_SOCKET")
        .env_remove("THESEUS_SESSION")
        .env("TZ", GOLDEN_TZ);
    if let Some(job) = job {
        cmd.env("THESEUS_SESSION", job);
    }
    let out = cmd.output().unwrap();
    let (seen, params) = daemon.join().unwrap();
    format!(
        "$ theseus {}\n--- requests\n{} {}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
        seen.join(" "),
        params,
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

fn record(id: &str, kind: &str, scope: Value, text: &str) -> Value {
    json!({"id": id, "kind": kind, "text": text, "scope": scope, "since_ms": 1_790_000_000_000u64,
           "source": "the CLI", "state": "live", "compiles": 12, "missing": 0})
}

#[test]
fn the_list_has_a_row_a_record_and_the_manifests_tally() {
    let list = json!({"records": [
        record("std_3f9a1c2d", "order", json!({"kind": "everywhere"}), "never push to main"),
        record("std_77aa01b2", "fact", json!({"kind": "place", "of": "discord:dm:u1"}), "keeps a vegetable garden"),
    ], "compiles": 12, "missing": 0});
    golden("standing_list", &run(&["standing"], Some(list), None));
    let empty = json!({"records": [], "compiles": 0, "missing": 0});
    golden(
        "standing_none",
        &run(&["standing", "list"], Some(empty), None),
    );
}

#[test]
fn an_add_and_a_revoke_say_what_they_did() {
    let r = record(
        "std_0000aaaa",
        "preference",
        json!({"kind": "everywhere"}),
        "answer in two sentences",
    );
    let added = json!({"record": r, "ended": null});
    golden(
        "standing_add",
        &run(
            &[
                "standing",
                "add",
                "--kind",
                "preference",
                "answer",
                "in",
                "two",
                "sentences",
            ],
            Some(added),
            None,
        ),
    );
    let revoked = json!({"record": r, "ended": "std_3f9a1c2d"});
    golden(
        "standing_revoke",
        &run(&["standing", "revoke", "std_3f9a"], Some(revoked), None),
    );
}

/// A write inside a job is refused before anything is sent.
#[test]
fn a_write_inside_a_job_is_refused_and_a_read_is_not() {
    golden(
        "standing_refused",
        &run(
            &["standing", "add", "never", "push"],
            None,
            Some("ses_0000aa1b2c3"),
        ),
    );
}
