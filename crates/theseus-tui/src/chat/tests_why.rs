//! `/why` (theseus-vjzt, phase 2): the four reads at once, worded when all
//! have answered; a side read that fails leaves its section out, and only
//! `context.explain` failing is told.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::{json, Value};

use super::tests::{script, Rig, SID};
use crate::tests::{Answer, Script};

fn explain() -> Value {
    json!({
        "session_id": SID, "kind": "conversation", "class": "private", "place": "CLI",
        "turns": [], "turn_id": "trn_2",
        "compiled": {"session_id": SID, "turn_id": "trn_2", "loop": 0, "decision": "append",
            "messages": 9},
        "profile": "f10", "model": "claude-sonnet-5-5", "window": 200000,
        "parts": [
            {"block": "header", "name": "persona", "bytes": 4800, "tokens": 1200},
            {"block": "conversation", "name": "6 nodes", "bytes": 20000, "tokens": 7550}],
        "digest_now": "d1", "recalls": [], "sources": {}, "ms": 4.0
    })
}

/// A daemon that answers the four reads; `fail` names methods that error.
fn why_script(fail: &'static [&'static str]) -> Script {
    let base = script();
    Arc::new(move |m, p| -> Answer {
        if fail.contains(&m) {
            return Err((-32000, "the store is busy".into()));
        }
        match m {
            "context.explain" => Ok(explain()),
            "policy.explain" => Ok(json!({"places": [], "roots": []})),
            "ledger.tail" => Ok(json!({"rows": [], "total": 0})),
            "node.list" => Ok(json!({"nodes": [
                {"node_id": "n1", "kind": "tool_call", "session_id": SID, "position": 5,
                 "at_unix_ms": 1, "turn_id": "trn_2", "text": "",
                 "detail": {"tool": "proc.run"}, "bytes": 0}], "total": 1})),
            _ => base(m, p),
        }
    })
}

async fn ask_why(r: &mut Rig) {
    r.ready().await;
    r.type_text("/why").await;
    r.press(KeyCode::Enter).await;
}

#[tokio::test(start_paused = true)]
async fn why_words_the_turn_from_four_reads_asked_once_each() {
    let mut r = Rig::with(100, 40, why_script(&[])).await;
    ask_why(&mut r).await;
    r.shows("ran on: f10 (claude-sonnet-5-5)").await;
    let t = r.text();
    assert!(t.contains("turn trn_2") && t.contains(SID), "{t}");
    assert!(t.contains("proc.run"), "a called tool is named:\n{t}");
    for m in [
        "context.explain",
        "policy.explain",
        "ledger.tail",
        "node.list",
    ] {
        assert_eq!(r.daemon.asked(m).len(), 1, "{m} asked once");
    }
    assert_eq!(r.daemon.asked("context.explain")[0]["session_id"], SID);
}

#[tokio::test(start_paused = true)]
async fn a_failed_side_read_leaves_its_section_out_and_the_rest_shows() {
    let mut r = Rig::with(100, 40, why_script(&["ledger.tail"])).await;
    ask_why(&mut r).await;
    r.shows("ran on: f10 (claude-sonnet-5-5)").await;
    let t = r.text();
    assert!(t.contains("proc.run"), "{t}");
    assert!(!t.contains("could not read it"), "{t}");
}

#[tokio::test(start_paused = true)]
async fn a_failed_context_read_is_told_and_nothing_else_shows() {
    let mut r = Rig::with(100, 40, why_script(&["context.explain"])).await;
    ask_why(&mut r).await;
    r.shows("could not read it: the store is busy").await;
    assert!(!r.text().contains("ran on:"), "{}", r.text());
}

// ------------------------------------------------ a lost connection

fn why_calls(effects: Vec<super::state::Effect>) -> usize {
    use super::state::{Effect, Purpose};
    effects
        .into_iter()
        .filter(|e| matches!(e, Effect::Call(c) if matches!(c.purpose, Purpose::Why(_))))
        .count()
}

fn notice(c: &super::state::Chat) -> String {
    c.notice.as_ref().map(|(t, _)| t.clone()).unwrap_or_default()
}

#[test]
fn a_lost_connection_does_not_leave_why_stuck_on_still_reading_it() {
    use super::state::Chat;
    use super::tests::{cfg, T0};
    let mut c = Chat::new(cfg(std::path::PathBuf::from("/nonexistent/history")), T0);
    c.session = Some(SID.to_string());
    assert_eq!(why_calls(super::why::why(&mut c, "")), 4);
    // Unanswered and still connected: a second /why refuses and sends nothing.
    assert_eq!(why_calls(super::why::why(&mut c, "")), 0);
    assert!(notice(&c).contains("still reading it"), "{}", notice(&c));
    c.notice = None;
    // The four answers die with the connection.
    c.disconnected();
    assert_eq!(
        why_calls(super::why::why(&mut c, "")),
        4,
        "the next /why asks its four reads again"
    );
    assert!(!notice(&c).contains("still reading it"), "{}", notice(&c));
}
