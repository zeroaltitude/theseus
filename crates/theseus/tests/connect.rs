//! How the CLI reaches a daemon (theseus-7yx, step 10a), through the
//! library's `client::Conn`: `--spawn BIN` runs `BIN --stdio` and speaks the
//! protocol over its pipes, and a daemon that cannot be reached either way
//! exits 3. `main` reads that from the error's text, which the library
//! writes, so these tests hold the two together.

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::Value;

const THESEUS: &str = env!("CARGO_BIN_EXE_theseus");

/// A shell script stands in for theseusd: it records its arguments and the
/// request it reads, answers it, and exits.
#[test]
fn spawn_runs_the_daemon_on_stdio_and_reads_its_answer() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().display();
    let script = dir.path().join("harbour-theseusd");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             echo \"$@\" > '{d}/args'\n\
             IFS= read -r line\n\
             printf '%s\\n' \"$line\" > '{d}/request'\n\
             printf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{\"session_id\":\"ses_harbour1\"}}}}'\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new(THESEUS)
        .arg("--spawn")
        .arg(&script)
        .args(["sessions", "open", "--label", "harbour"])
        .env_remove("THESEUS_SOCKET")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "ses_harbour1\n");
    let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
    assert_eq!(args, "--stdio\n");
    let request = std::fs::read_to_string(dir.path().join("request")).unwrap();
    let request: Value = serde_json::from_str(&request).unwrap();
    assert_eq!(
        (
            &request["method"],
            &request["id"],
            &request["params"]["label"]
        ),
        (
            &Value::from("session.open"),
            &Value::from(1),
            &Value::from("harbour")
        )
    );
}

/// A daemon that cannot be spawned, or a socket with no daemon behind it,
/// exits 3 with what it tried.
#[test]
fn a_daemon_that_cannot_be_reached_exits_3() {
    let dir = tempfile::tempdir().unwrap();
    let spawned = Command::new(THESEUS)
        .args(["--spawn", "/nonexistent/harbour-theseusd", "health"])
        .output()
        .unwrap();
    assert_eq!(spawned.status.code(), Some(3));
    let err = String::from_utf8_lossy(&spawned.stderr);
    assert!(
        err.starts_with("theseus: spawning /nonexistent/harbour-theseusd"),
        "{err}"
    );
    let sock = dir.path().join("harbour.sock");
    let connected = Command::new(THESEUS)
        .arg("--socket")
        .arg(&sock)
        .arg("health")
        .output()
        .unwrap();
    assert_eq!(connected.status.code(), Some(3));
    let err = String::from_utf8_lossy(&connected.stderr);
    assert!(
        err.starts_with(&format!(
            "theseus: connecting to theseusd at {} (is it running? try --spawn)",
            sock.display()
        )),
        "{err}"
    );
}
