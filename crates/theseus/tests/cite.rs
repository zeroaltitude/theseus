//! A reply's citations in the CLI (theseus-ejdk), the real binary against a
//! scripted daemon: `ask` streams the reply with its handles as numbered
//! marks and a footnote block under it (each source's header and the command
//! that opens it); `--no-stream` does the same from the result; `history`
//! marks the replies it shows from the page's `cites`; a handle with no
//! source stays text; and `history m:a1b2c3` opens the cited message whole.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");
const S: &str = "ses_q7f3k2";
const T: &str = "turn_m4p8z1";
const HEADER: &str = "a message from ana in #harbor, 2026-09-30 14:34 UTC (as of @18231)";

/// Runs `theseus ARGS` against a daemon that answers each request of `script`
/// (method, result, notifications before it) in order, and `session.wait`
/// (what `history` records after its page) with nothing. Returns stdout,
/// stderr, and the methods seen.
fn run(
    args: &[&str],
    script: Vec<(&'static str, Value, Vec<Value>)>,
) -> (String, String, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(&s);
        let mut seen = Vec::new();
        let mut script = script.into_iter();
        s.set_read_timeout(Some(std::time::Duration::from_millis(400)))
            .unwrap();
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let req: Value = serde_json::from_str(&line).unwrap();
            line.clear();
            let method = req["method"].as_str().unwrap_or("?").to_string();
            seen.push(method.clone());
            let mut out = String::new();
            let result = if method == "session.wait" {
                json!({"reached": "timeout", "already": false, "confirms": []})
            } else {
                let (want, result, before) = script.next().expect("a request past the script");
                assert_eq!(method, want);
                for n in before {
                    out.push_str(&format!("{n}\n"));
                }
                result
            };
            out.push_str(&format!(
                "{}\n",
                json!({"jsonrpc": "2.0", "id": req["id"], "result": result})
            ));
            (&s).write_all(out.as_bytes()).unwrap();
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
        .env("TZ", "<-07>7")
        .output()
        .unwrap();
    let seen = daemon.join().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        seen,
    )
}

fn cite(handle: &str) -> Value {
    json!({"handle": handle, "node_id": "msg_0198a0b1c2d3e4f5a6b7c8d9e0f1a2b3", "session_id": "ses_harbor",
           "position": 18231, "header": HEADER, "at_unix_ms": 1_759_300_000_000u64,
           "excerpt": "We chose oak for the new pier."})
}

fn result(output: &str, cites: Vec<Value>) -> Value {
    json!({"session_id": S, "turn_id": T, "loops": 1, "output": output, "stop_reason": "end_turn",
           "provider_stop_reason": "end_turn", "model": "glm-x", "provider": "zed", "profile": "quick",
           "usage": {"input_tokens": 10, "output_tokens": 5}, "elapsed_ms": 900, "cost_usd": 0.001,
           "tool_calls": 0, "continuation": false, "cites": cites})
}

fn delta(text: &str) -> Value {
    json!({"jsonrpc": "2.0", "method": "model.delta", "params": {"turn_id": T, "loop_index": 0, "text": text}})
}

/// The streamed reply's handles are marks as they complete, even split across
/// pieces, and the block under it says each source; a handle that was no
/// source says so.
#[test]
fn ask_streams_marks_and_footnotes() {
    let pieces = [
        "You chose oak [m:a1",
        "b2c3] for the pier; and [m:",
        "ffffff].\n",
    ];
    let (out, _, seen) = run(
        &["ask", "What did we choose?"],
        vec![(
            "turn.submit",
            result(
                "You chose oak [m:a1b2c3] for the pier; and [m:ffffff].\n",
                vec![cite("m:a1b2c3")],
            ),
            pieces.iter().map(|p| delta(p)).collect(),
        )],
    );
    assert_eq!(
        out,
        format!(
            "You chose oak [1] for the pier; and [2].\n\
             \x20 [1] {HEADER}\n      theseus history m:a1b2c3\n\
             \x20 [2] m:ffffff: not a source this reply was given\n"
        )
    );
    assert_eq!(seen, ["turn.submit"]);
}

#[test]
fn ask_without_streaming_marks_what_has_a_source_and_leaves_the_rest() {
    let (out, _, _) = run(
        &["--no-stream", "ask", "What did we choose?"],
        vec![(
            "turn.submit",
            result("Oak [m:a1b2c3], and [m:ffffff].", vec![cite("m:a1b2c3")]),
            vec![],
        )],
    );
    assert_eq!(
        out,
        format!("Oak [1], and [m:ffffff].\n  [1] {HEADER}\n      theseus history m:a1b2c3\n")
    );
}

fn history(cites: Vec<Value>) -> Value {
    json!({
        "session": {"session_id": S, "kind": "conversation", "label": null,
                    "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1,
                    "usage": {"input_tokens": 1, "output_tokens": 1}, "execution_id": null,
                    "execution_state": null, "last_active_ms": 1_759_300_900_000u64,
                    "cost_usd": 0.0, "tool_calls": 0, "model": "glm-x", "title": "Pier"},
        "nodes": [{"node_id": "msg_aaa111", "kind": "assistant_message", "session_id": S,
                   "position": 9, "at_unix_ms": 1_759_300_100_000u64, "turn_id": T,
                   "text": "We chose oak [m:a1b2c3].", "detail": {"model": "glm-x", "stop_reason": "end_turn",
                   "usage": {"input_tokens": 3, "output_tokens": 2}, "tool_calls": []}, "bytes": 24}],
        "pending_confirms": [],
        "cites": cites,
    })
}

#[test]
fn history_marks_the_replies_it_shows() {
    let (out, _, _) = run(
        &["history", S],
        vec![("session.history", history(vec![cite("m:a1b2c3")]), vec![])],
    );
    assert!(out.contains("Theseus (glm-x): We chose oak [1]."), "{out}");
    assert!(
        out.contains(&format!("  [1] {HEADER}\n      theseus history m:a1b2c3\n")),
        "{out}"
    );
    // No cites, no marks: the text is the model's own.
    let (plain, _, _) = run(
        &["history", S],
        vec![("session.history", history(vec![]), vec![])],
    );
    assert!(
        plain.contains("Theseus (glm-x): We chose oak [m:a1b2c3]."),
        "{plain}"
    );
    assert!(!plain.contains("theseus history m:"), "{plain}");
}

/// The footnote's command: `theseus history m:a1b2c3` names the node by its
/// id's end, reads the node's own page of its session, and prints it whole.
#[test]
fn history_opens_a_handle() {
    let reach = json!({"node_id": "msg_0198a0b1c2d3e4f5a6b7c8d9e0f1a2b3", "session_id": "ses_harbor",
        "position": 18231, "direct": {"compilations": [], "loops": 0},
        "descendants": [], "totals": {"contexts": 0, "sessions": 0}, "partial": false});
    let mut page = history(vec![]);
    page["nodes"] = json!([{"node_id": "msg_0198a0b1c2d3e4f5a6b7c8d9e0f1a2b3", "kind": "user_message",
        "session_id": "ses_harbor", "position": 18231, "at_unix_ms": 1_759_300_100_000u64,
        "author": "ana", "text": "We chose oak for the new pier.\nIt is dry.", "detail": null, "bytes": 40}]);
    let (out, err, seen) = run(
        &["history", "m:a1b2c3"],
        vec![
            ("node.reach", reach, vec![]),
            ("session.history", page, vec![]),
        ],
    );
    assert!(
        out.contains("operator (ana):\n    We chose oak for the new pier.\n    It is dry."),
        "{out}{err}"
    );
    assert!(
        out.contains(
            "── in ses_harbor at position 18231: theseus history ses_harbor --before 18232"
        ),
        "{out}"
    );
    assert_eq!(&seen[..2], ["node.reach", "session.history"]);
}
