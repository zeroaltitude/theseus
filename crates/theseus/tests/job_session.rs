//! Inside a job, the CLI names its job's session (theseus-b5cl): `ask` and
//! `sessions open` send the `THESEUS_SESSION` every job carries as
//! `opened_from`, so the session they reach takes that session's hold of
//! external text. Outside a job they send none.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::Command;

use serde_json::{json, Value};

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// The one request the CLI sends with `args`, to a daemon that answers it
/// with `result`, run with `THESEUS_SESSION` set to `session`, or unset.
fn sent(args: &[&str], session: Option<&str>, result: Value) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let daemon = std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&s).read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(&line).unwrap();
        let answer = json!({"jsonrpc": "2.0", "id": req["id"], "result": result});
        (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        req
    });
    let mut cmd = Command::new(THESEUS);
    cmd.arg("--socket").arg(&sock).args(args);
    match session {
        Some(s) => cmd.env("THESEUS_SESSION", s),
        None => cmd.env_remove("THESEUS_SESSION"),
    };
    let out = cmd.output().unwrap();
    let req = daemon.join().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    req
}

/// A turn's answer, as the daemon sends it.
fn turn_result() -> Value {
    json!({"session_id": "ses_0000ff9e8d7", "turn_id": "trn_1", "loops": 1, "output": "Done.",
        "stop_reason": "end_turn", "provider_stop_reason": null, "model": "invented-model",
        "usage": {"input_tokens": 1, "output_tokens": 1}, "elapsed_ms": 5})
}

#[test]
fn ask_and_sessions_open_send_the_jobs_session_as_opened_from() {
    let open = |session| {
        sent(
            &["sessions", "open"],
            session,
            json!({"session_id": "ses_0000ff9e8d7"}),
        )
    };
    let req = open(Some("ses_0000aa1b2c3"));
    assert_eq!(req["method"], "session.open");
    assert_eq!(req["params"]["opened_from"], "ses_0000aa1b2c3");
    let req = open(None);
    assert!(req["params"].get("opened_from").is_none(), "{req}");

    let ask = |session| sent(&["ask", "--no-stream", "hello"], session, turn_result());
    let req = ask(Some("ses_0000aa1b2c3"));
    assert_eq!(req["method"], "turn.submit");
    assert_eq!(req["params"]["opened_from"], "ses_0000aa1b2c3");
    let req = ask(None);
    assert!(req["params"].get("opened_from").is_none(), "{req}");
}
