//! `theseus inspect [SESSION] [TURN]` (theseus-gyjyo R7b): a turn's requests and
//! which vault secrets each carried, by name, and whether each was substituted
//! before it left. A scripted daemon, as `open.rs` has: the reads it makes are
//! the session list, the turn list and `turn.inspect`, and nothing is written.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

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

fn sessions() -> Value {
    json!({"sessions": [{"session_id": S, "kind": "conversation", "label": null, "created_at_unix_ms": 1_759_300_000_000u64, "turns": 1}]})
}

/// The `turn.started` rows of the turns named, oldest first, as `ledger.tail` answers.
fn started(turns: &[&str]) -> Value {
    let rows: Vec<Value> = turns
        .iter()
        .enumerate()
        .map(|(i, t)| json!({"position": 10 + i as u64, "at_unix_ms": 1 + i as u64, "kind": "turn.started", "session_id": S, "turn_id": t, "data": {}}))
        .collect();
    json!({"rows": rows, "total": rows.len()})
}

/// A turn of two loops: the first sent a request that carried two vault
/// secrets, substituted; the second one, sent as written; and a token count,
/// which is no loop's request.
fn inspected(rows: Vec<Value>) -> Value {
    json!({"session_id": S, "turn_id": T, "outcome": "ended", "url": "", "rows": rows,
    "loops": [
        {"loop": 0, "answer": {"provider": "anthropic", "model": "claude-sonnet-5-5",
            "compilation_id": "cmp_a1", "request_digest": "dig0abc", "redacted": [
                {"names": ["anthropic_key", "db_password"], "count": 3, "substituted": true, "correlation_id": "act_p0"}]}},
        {"loop": 1, "answer": {"provider": "anthropic", "model": "claude-sonnet-5-5",
            "compilation_id": "cmp_a1", "request_digest": "dig1abc", "redacted": []}}
    ]})
}

fn row(position: u64, data: Value) -> Value {
    json!({"position": position, "at_ms": 1, "kind": "request.redacted", "data": data})
}

fn script(inspect: Value) -> Vec<(&'static str, Value)> {
    vec![
        ("session.list", sessions()),
        ("ledger.tail", started(&[T])),
        ("turn.inspect", inspect),
    ]
}

/// The operator's question, answered: which vault secret, by name, in which
/// request, to which model, and whether it was substituted.
#[test]
fn it_names_each_requests_vault_secrets_and_whether_they_were_substituted() {
    let rows = vec![
        row(
            1,
            json!({"op": "stream", "provider": "anthropic", "model": "claude-sonnet-5-5", "loop": 0, "names": ["anthropic_key", "db_password"], "count": 3, "substituted": true}),
        ),
        row(
            2,
            json!({"op": "count_tokens", "loop": null, "names": ["x_only"], "count": 1, "substituted": true}),
        ),
        row(
            3,
            json!({"op": "stream", "provider": "anthropic", "model": "claude-sonnet-5-5", "loop": 1, "names": ["db_password"], "count": 1, "substituted": false}),
        ),
    ];
    let (out, err, seen) = run(&["inspect"], script(inspected(rows)));
    assert_eq!(seen.len(), 3, "{seen:?} {err}");
    let want = [
        format!("turn {T} of {S}: ended, 2 loops"),
        "loop 0  anthropic/claude-sonnet-5-5  compilation cmp_a1  digest dig0abc".into(),
        "  carried anthropic_key, db_password (3 met) to anthropic/claude-sonnet-5-5: substituted before it left".into(),
        "loop 1  anthropic/claude-sonnet-5-5  compilation cmp_a1  digest dig1abc".into(),
        "  carried db_password (1 met) to anthropic/claude-sonnet-5-5: SENT AS WRITTEN (redact_outbound is off)".into(),
    ];
    assert_eq!(out.lines().collect::<Vec<_>>(), want, "{err}");
    assert!(!out.contains("x_only"), "a token count is no request");
}

/// A compaction call runs on its own profile, which can be another model than
/// the loop's answer: its line names the model the call went to, from its own
/// row, and says it was a compaction call.
#[test]
fn a_compaction_call_names_its_own_model() {
    let rows = vec![
        row(
            1,
            json!({"op": "stream", "provider": "anthropic", "model": "claude-sonnet-5-5", "loop": 0, "names": ["anthropic_key"], "count": 1, "substituted": true}),
        ),
        row(
            2,
            json!({"op": "compaction", "provider": "openai", "model": "gpt-summary", "loop": 0, "names": ["db_password"], "count": 2, "substituted": true}),
        ),
    ];
    let (out, _, _) = run(&["inspect"], script(inspected(rows)));
    assert!(
        out.contains("  carried db_password (2 met) in a compaction call to openai/gpt-summary: substituted before it left"),
        "{out}"
    );
    assert!(
        out.contains("  carried anthropic_key (1 met) to anthropic/claude-sonnet-5-5: substituted"),
        "{out}"
    );
}

/// A turn whose requests carried nothing says so, rather than printing nothing.
#[test]
fn a_turn_that_carried_no_vault_value_says_so() {
    let (out, _, _) = run(&["inspect"], script(inspected(vec![])));
    assert!(
        out.ends_with("no request carried a value the vault resolved.\n"),
        "{out}"
    );
}

/// A request whose call failed has a row and no answer: it is still named.
#[test]
fn a_request_whose_call_failed_is_still_named() {
    let rows = vec![row(
        1,
        json!({"op": "stream", "loop": 4, "names": ["anthropic_key"], "count": 1, "substituted": true}),
    )];
    let (out, _, _) = run(&["inspect"], script(inspected(rows)));
    assert!(
        out.contains("loop 4  carried anthropic_key (1 met): substituted before it left"),
        "{out}"
    );
}

#[test]
fn json_is_each_loops_requests() {
    let rows = vec![row(
        1,
        json!({"op": "stream", "loop": 0, "names": ["anthropic_key"], "count": 2, "substituted": true, "correlation_id": "act_p0"}),
    )];
    let (out, _, _) = run(&["--json", "inspect"], script(inspected(rows)));
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(
        v["loops"][0]["redacted"][0]["names"],
        json!(["anthropic_key"])
    );
    assert_eq!(v["loops"][0]["redacted"][0]["substituted"], json!(true));
    assert_eq!(v["loops"][1]["redacted"], json!([]));
    assert_eq!(v["turn_id"], T);
}
