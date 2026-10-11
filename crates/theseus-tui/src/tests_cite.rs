//! A reply's citations in the terminal UI's pane (theseus-ejdk): a history
//! page's replies show their cited handles as numbered marks with the
//! footnote block under them, as `theseus history` does, and a turn this TUI
//! sent that answers with its sources marks the reply that streamed in and
//! adds the block. The pane alone, as `tests_order.rs` drives it.

use serde_json::{json, Value};
use theseus_client::render::Tag;
use theseus_protocol::cite::Citation;
use theseus_protocol::{Event, SessionHistoryResult};

use crate::detail::Detail;

const HEADER: &str = "a message from ana in #harbor, 2026-09-30 14:34 UTC (as of @18231)";

fn cite(handle: &str) -> Citation {
    Citation {
        handle: handle.into(),
        node_id: "msg_0198a0b1c2d3e4f5a6b7c8d9e0f1a2b3".into(),
        session_id: "ses_harbor".into(),
        position: 18231,
        header: HEADER.into(),
        at_unix_ms: 0,
        excerpt: "We chose oak.".into(),
        discord: None,
    }
}

fn history(cites: &[Citation]) -> SessionHistoryResult {
    let h: Value = json!({
        "session": {"session_id": "ses_dm0001", "kind": "conversation", "label": null,
            "created_at_unix_ms": 1_790_000_000_000u64, "turns": 1,
            "usage": {"input_tokens": 1, "output_tokens": 1}, "execution_id": null,
            "execution_state": null, "last_active_ms": 1_790_000_000_000u64,
            "cost_usd": 0.0, "tool_calls": 0, "model": "glm-x", "title": "Pier"},
        "nodes": [{"node_id": "msg_aaa111", "kind": "assistant_message", "session_id": "ses_dm0001",
            "position": 9, "at_unix_ms": 1_790_000_100_000u64, "turn_id": "turn_a",
            "text": "We chose oak [m:a1b2c3], not [m:ffffff].", "detail": {"model": "glm-x",
            "stop_reason": "end_turn", "usage": {"input_tokens": 3, "output_tokens": 2},
            "tool_calls": []}, "bytes": 40}],
        "pending_confirms": [],
        "cites": cites,
    });
    serde_json::from_value(h).unwrap()
}

fn texts(d: &Detail) -> Vec<String> {
    d.lines().iter().map(|(_, t)| t.clone()).collect()
}

/// The history's goldens: the reply's text with its cited handle a mark, the
/// one with no source left as it was written, and the footnote block, each
/// dim, under the reply's own lines.
#[test]
fn a_history_page_marks_its_cited_replies() {
    let mut d = Detail::new("ses_dm0001");
    d.history(&history(&[cite("m:a1b2c3")]));
    let t = texts(&d);
    assert!(
        t[0].ends_with("glm-x: We chose oak [1], not [m:ffffff]."),
        "{t:?}"
    );
    let notes: Vec<_> = d
        .lines()
        .iter()
        .filter(|(tag, _)| *tag == Tag::Dim)
        .collect();
    assert!(
        t.contains(&format!("  [1] {HEADER}"))
            && t.contains(&"      theseus history m:a1b2c3".to_string()),
        "{t:?}"
    );
    assert!(notes.len() >= 3, "the footnote lines are dim: {notes:?}");
    // With no citations the page is as it was.
    let mut plain = Detail::new("ses_dm0001");
    plain.history(&history(&[]));
    assert!(texts(&plain)[0].ends_with("glm-x: We chose oak [m:a1b2c3], not [m:ffffff]."));
}

/// A turn this TUI sent: its reply streams in raw, and the answer's sources
/// turn the handles into marks and add the block under the reply, never above
/// the operator's own line.
#[test]
fn a_submitted_turns_sources_mark_the_reply_that_streamed() {
    let mut d = Detail::new("ses_dm0001");
    d.loaded = true;
    d.sent("What did we choose?");
    for piece in ["We chose oak [m:a1", "b2c3].\nNot [m:ffffff].\n"] {
        let e = Event::from_notification(
            "model.delta",
            &json!({"session_id": "ses_dm0001", "turn_id": "turn_a", "loop_index": 0, "text": piece}),
        )
        .unwrap()
        .unwrap();
        d.event(&e);
    }
    d.cited(&[cite("m:a1b2c3")]);
    assert_eq!(
        texts(&d),
        [
            "you: What did we choose?".to_string(),
            "We chose oak [1].".into(),
            "Not [m:ffffff].".into(),
            format!("  [1] {HEADER}"),
            "      theseus history m:a1b2c3".into(),
        ]
    );
    // Nothing cited, nothing changed.
    let before = texts(&d);
    d.cited(&[]);
    assert_eq!(texts(&d), before);
}
