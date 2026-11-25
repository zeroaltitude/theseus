//! The conversation that attached a server is told when it did not come up
//! (issue 54): a server that refuses every connection after the yes ends
//! `failed`, and that session's next turn reads one line saying so, with the
//! HTTP status and the standard code and none of the server's own words. A
//! server that comes up tells nothing; a server no session attached tells no
//! session; a failure is told once, not once per try.

use std::time::Duration;

use serde_json::json;

use super::tests::{rig, Rig};
use crate::node::Body;

/// The words the note says the server is in.
const TOLD: &str = "is attached but not connected";

/// Attach the HTTP server `name` from a new session and say yes; the session.
async fn attach(r: &Rig, name: &str) -> String {
    r.core.proofs.open_stand_in_window("cli");
    let sid = r.session();
    r.turn(
        &sid,
        "mcp_attach",
        json!({"name": name, "url": "https://docs.example.invalid:8443/mcp", "auth_secret": "wc_key"}),
    )
    .await;
    let q = r.asked().remove(0);
    assert!(
        r.core
            .confirm_action(&q.correlation_id, true, None, "cli")
            .unwrap()
            .approved
    );
    sid
}

/// The notes in `sid` that say a server is not connected.
fn told(r: &Rig, sid: &str) -> Vec<String> {
    r.core
        .store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .filter_map(|(_, n)| match n.body {
            Body::UserMessage { text, .. } if text.contains(TOLD) => Some(text),
            _ => None,
        })
        .collect()
}

fn state(r: &Rig, name: &str) -> String {
    r.core
        .mcp
        .status()
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.state)
        .unwrap_or_default()
}

/// Wait, on tokio's clock (paused or not), for `f`; at most 20 s of real time.
async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = std::time::Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(20), "no {what} in 20 s");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// The owner's case: the server answers 401 after the yes, and after three
/// tries the board holds it `failed`. The session that attached it reads one
/// line in its next turn: the server, that it is not connected, the status
/// and the code, and what the owner can do; never the body's own words. A
/// server from the file that fails too tells no session.
#[tokio::test]
async fn a_server_refused_after_the_yes_is_told_to_the_session_that_attached_it() {
    let r = rig();
    let sid = attach(&r, "docs").await;
    r.refuse("docs");
    r.refuse("base");
    tokio::time::pause();
    r.core.mcp.start();
    until("a failed server", || state(&r, "docs") == "failed").await;
    until("a failed file server", || state(&r, "base") == "failed").await;
    until("the note", || !told(&r, &sid).is_empty()).await;
    tokio::time::resume();
    let notes = told(&r, &sid);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let note = &notes[0];
    assert!(note.contains("`docs`"), "{note}");
    assert!(note.contains("HTTP 401 (invalid_token)"), "{note}");
    assert!(note.contains("`theseus mcp restart docs`"), "{note}");
    assert!(note.contains("sign-in in a browser"), "{note}");
    assert!(
        !note.contains("harbour portal"),
        "the server's words: {note}"
    );
    assert!(
        !note.contains("base"),
        "a server no session attached: {note}"
    );
    let given = r.says(&sid, "is it up?").await;
    assert_eq!(given.matches(TOLD).count(), 1, "{given}");
    assert!(given.contains("HTTP 401 (invalid_token)"), "{given}");
    assert!(!given.contains("harbour portal"), "{given}");
}

/// Of a refusal's body only a standard code or a JSON-RPC number is said: a
/// code of the server's own, its other words, and any other failure's reason
/// are not.
#[test]
fn only_the_status_and_a_standard_code_are_said() {
    use crate::mcp::attached::failed_note;
    let said = |why: &str| failed_note("docs", why);
    let own = said(r#"HTTP 401: {"error":"visit_the_harbour_portal"}"#);
    assert!(own.contains("with HTTP 401, and"), "{own}");
    assert!(!own.contains("harbour"), "{own}");
    let plain = said("HTTP 503: the harbour is closed today");
    assert!(plain.contains("with HTTP 503, and"), "{plain}");
    assert!(
        !plain.contains("harbour") && !plain.contains("sign-in"),
        "{plain}"
    );
    let rpc = said("the server answered error -32001: the harbour wants a pass");
    assert!(rpc.contains("with JSON-RPC error -32001, and"), "{rpc}");
    assert!(!rpc.contains("harbour"), "{rpc}");
    let other = said("the server exited with status 2: HARBOUR_KEY is not set");
    assert!(other.contains("its connection did not come up"), "{other}");
    assert!(!other.contains("HARBOUR"), "{other}");
}

/// A server that comes up tells nothing.
#[tokio::test]
async fn a_server_that_comes_up_tells_nothing() {
    let r = rig();
    let sid = attach(&r, "docs").await;
    r.core.mcp.start();
    until("a ready server", || state(&r, "docs") == "ready").await;
    let given = r.says(&sid, "is it up?").await;
    assert!(!given.contains(TOLD), "{given}");
    assert!(told(&r, &sid).is_empty());
}

/// Three tries make one failure and one line; a restart that fails again
/// makes one line more, not three.
#[tokio::test]
async fn a_second_failure_after_a_restart_is_told_once_more() {
    let r = rig();
    let sid = attach(&r, "docs").await;
    r.refuse("docs");
    tokio::time::pause();
    r.core.mcp.start();
    until("the first note", || told(&r, &sid).len() == 1).await;
    tokio::time::sleep(Duration::from_secs(3_600)).await;
    assert_eq!(told(&r, &sid).len(), 1, "once for three tries");
    assert!(r.core.mcp.restart("docs").is_some());
    until("the second note", || told(&r, &sid).len() == 2).await;
    tokio::time::sleep(Duration::from_secs(3_600)).await;
    assert_eq!(told(&r, &sid).len(), 2, "once more, not per try");
    tokio::time::resume();
    let given = r.says(&sid, "is it up now?").await;
    assert_eq!(given.matches(TOLD).count(), 2, "{given}");
}
