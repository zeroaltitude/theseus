//! A held turn's status line names no model (theseus-2rvwv).

use super::status_line;
use theseus_protocol::TurnSubmitResult;

fn result(stop_reason: &str) -> TurnSubmitResult {
    serde_json::from_value(serde_json::json!({"session_id": "ses_a", "turn_id": "turn_a",
        "loops": 1, "output": "", "stop_reason": stop_reason, "model": "claude-sonnet-5",
        "provider": "anthropic", "profile": "sonnet", "usage": {"input_tokens": 0, "output_tokens": 0},
        "elapsed_ms": 900}))
    .unwrap()
}

#[test]
fn a_held_turns_line_says_no_model_ran() {
    let held = status_line(&result("held"));
    assert!(
        held.starts_with("[sonnet → held, no model ran · 1 loop(s)"),
        "{held}"
    );
    assert!(!held.contains("claude-sonnet-5"), "{held}");
    let done = status_line(&result("no_tool_calls"));
    assert!(
        done.starts_with("[sonnet → anthropic/claude-sonnet-5 · 1 loop(s)"),
        "{done}"
    );
}
