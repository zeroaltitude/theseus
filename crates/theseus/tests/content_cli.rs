//! `theseus content` against a scripted daemon (theseus-9ymz0): the list of a
//! session's labels, the removal of one or all, and the refusal of a removal
//! inside a job. Each runs the real binary over a Unix socket that answers its
//! requests in order. Kept apart from golden.rs, which is at its line ceiling.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
/// The zone the times are written in: UTC-7, all year.
const GOLDEN_TZ: &str = "<-07>7";
const S: &str = "ses_q7f3k2";

/// Run `theseus --socket <sock> ARGS` against a daemon that answers each
/// request with the next of `steps` (its method, and its result), inside the
/// job `job` names when it does. What it printed, with the requests it saw.
fn run_in(args: &[&str], steps: Vec<(&'static str, Value)>, job: Option<&str>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen = Vec::new();
        for (method, result) in steps {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let req: Value = serde_json::from_str(&line).unwrap();
            seen.push(req["method"].as_str().unwrap_or("?").to_string());
            let a = if req["method"] == method {
                json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
            } else {
                json!({"jsonrpc": "2.0", "id": req["id"], "error": {"code": -32601,
                    "message": format!("the script expected {method}")}})
            };
            (&s).write_all(format!("{a}\n").as_bytes()).unwrap();
        }
        seen
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
    let seen = daemon.join().unwrap();
    format!(
        "$ theseus {}\n--- requests\n{}\n--- stdout\n{}--- stderr\n{}--- exit {}\n",
        args.join(" "),
        seen.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
        out.status.code().unwrap_or(-1)
    )
}

fn run(args: &[&str], steps: Vec<(&'static str, Value)>) -> String {
    run_in(args, steps, None)
}

/// A session as `session.list` shows it, carrying `labels` (theseus-9ymz0).
fn labelled_list(labels: &[(&str, &str, &str)]) -> Value {
    let labels: Vec<Value> = labels
        .iter()
        .map(|(flag, kind, source)| {
            json!({"flag": flag, "node": "nod_e1", "kind": kind, "source": source,
                   "confidence": 0.93, "band": "act", "at_ms": 1_759_300_400_000u64})
        })
        .collect();
    json!({"sessions": [{"session_id": S, "kind": "conversation", "label": null,
        "created_at_unix_ms": 1_759_300_000_000u64, "turns": 2, "labels": labels}]})
}

/// What `content.unlabel` answers for `flag`.
fn unlabelled(flag: &str, kind: &str, source: &str) -> Value {
    json!({"session_id": S, "flag": flag, "by": "the CLI", "who": "sock#3", "via": "cli",
        "how": "content.unlabel", "at_ms": 1_759_300_500_000u64,
        "label": {"flag": flag, "node": "nod_e1", "kind": kind, "source": source,
                  "confidence": 0.93, "band": "act", "at_ms": 1_759_300_400_000u64}})
}

/// `theseus content SESSION` lists a session's labels, one line each with what
/// raised it and from where, and the command that clears it; a session with
/// none says so (theseus-9ymz0).
#[test]
fn content_lists_a_sessions_labels() {
    let out = run(
        &["content", S],
        vec![(
            "session.list",
            labelled_list(&[
                (
                    "injection",
                    "fetched_page",
                    "http.fetch notes.example/today",
                ),
                ("secret", "tool_result", "fs.read /srv/open/.env"),
            ]),
        )],
    );
    assert!(out.contains("--- requests\nsession.list\n"), "{out}");
    assert!(out.contains("ses_q7f3k2: 2 content labels\n"), "{out}");
    assert!(
        out.contains(
            "  injection\t2025-09-30 23:33\tfetched_page (node nod_e1)\tfrom http.fetch \
             notes.example/today\tact 0.93\n"
        ),
        "{out}"
    );
    assert!(
        out.contains("theseus content remove ses_q7f3k2 secret"),
        "{out}"
    );
    assert!(out.ends_with("--- exit 0\n"), "{out}");
    let none = run(&["content", S], vec![("session.list", labelled_list(&[]))]);
    assert!(none.contains("ses_q7f3k2: no content labels\n"), "{none}");
}

/// `theseus content remove SESSION FLAG` is one `content.unlabel`; `--all`
/// removes each label the session carries, one call each.
#[test]
fn content_remove_takes_one_label_or_all_of_them() {
    let one = run(
        &["content", "remove", S, "injection"],
        vec![
            (
                "session.list",
                labelled_list(&[(
                    "injection",
                    "fetched_page",
                    "http.fetch notes.example/today",
                )]),
            ),
            (
                "content.unlabel",
                unlabelled(
                    "injection",
                    "fetched_page",
                    "http.fetch notes.example/today",
                ),
            ),
        ],
    );
    assert!(
        one.contains("--- requests\nsession.list content.unlabel\n"),
        "{one}"
    );
    assert!(
        one.contains(
            "removed injection from session ses_q7f3k2 (by the CLI) · it was raised by \
             fetched_page from http.fetch notes.example/today · its calls that act run at \
             their postures again\n"
        ),
        "{one}"
    );
    let both = run(
        &["content", "remove", S, "--all"],
        vec![
            (
                "session.list",
                labelled_list(&[
                    (
                        "injection",
                        "fetched_page",
                        "http.fetch notes.example/today",
                    ),
                    ("secret", "tool_result", "fs.read /srv/open/.env"),
                ]),
            ),
            (
                "content.unlabel",
                unlabelled(
                    "injection",
                    "fetched_page",
                    "http.fetch notes.example/today",
                ),
            ),
            (
                "content.unlabel",
                unlabelled("secret", "tool_result", "fs.read /srv/open/.env"),
            ),
        ],
    );
    assert!(
        both.contains("--- requests\nsession.list content.unlabel content.unlabel\n"),
        "{both}"
    );
    assert!(both.contains("removed secret from session"), "{both}");
    assert!(both.ends_with("--- exit 0\n"), "{both}");
}

/// A removal loosens, so inside a Theseus job the CLI refuses it before it
/// sends anything, as it refuses `policy untighten` (theseus-zmgb), and a word
/// that is not a flag is a usage error.
#[test]
fn content_remove_is_refused_inside_a_job_and_takes_only_a_flag() {
    let out = run_in(
        &["content", "remove", S, "injection"],
        vec![(
            "session.list",
            labelled_list(&[("injection", "fetched_page", "x")]),
        )],
        Some("ses_job01"),
    );
    assert!(
        out.contains("theseus content remove refused: it is the operator's to run"),
        "{out}"
    );
    assert!(
        !out.contains("content.unlabel"),
        "nothing was removed: {out}"
    );
    assert!(out.contains("--- exit 1\n"), "{out}");
    // A usage error is the CLI's own, before it connects to anything.
    let bad = Command::new(THESEUS)
        .args([
            "--socket",
            "/nonexistent/sock",
            "content",
            "remove",
            S,
            "open",
        ])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2), "{bad:?}");
    assert!(String::from_utf8_lossy(&bad.stderr).contains("injection"));
}
