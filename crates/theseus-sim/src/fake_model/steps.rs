//! A scripted turn of several steps (theseus-7gir.13): a rule is matched on
//! the turn's opening user text, the last user message that carries a plain
//! text block (not a harness's `<system-reminder>`, nor the task graph's
//! view), and its step is how many user messages with tool results came
//! after it. So one rule scripts a whole turn's calls and its answer for any
//! harness, whatever else its requests carry: Claude Code merges a new
//! prompt into the message that holds the last turn's results, after them,
//! and sends reminders as text blocks beside its results.

use serde_json::Value;

/// A text block a harness adds, not the person's words.
fn harness_text(t: &str) -> bool {
    t.trim_start().starts_with("<system-reminder>")
        || t.starts_with(theseus_core::task_graph::view::HEAD)
}

/// A message's plain text: a string, or its text blocks but the harness's,
/// joined.
fn plain(m: &Value) -> String {
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .filter(|t| !harness_text(t))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// The turn's opening user text, and its step.
pub fn opening_text(req: &Value) -> (String, usize) {
    let Some(msgs) = req["messages"].as_array() else {
        return (String::new(), 0);
    };
    let has_result = |m: &Value| {
        m["content"]
            .as_array()
            .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
    };
    let Some(at) = msgs
        .iter()
        .rposition(|m| m["role"] == "user" && !plain(m).trim().is_empty())
    else {
        return (String::new(), 0);
    };
    let steps = msgs[at + 1..]
        .iter()
        .filter(|m| m["role"] == "user" && has_result(m))
        .count();
    (plain(&msgs[at]), steps)
}

/// The word after `marker=` in `text`, the run's unique marker; empty when
/// there is none.
pub fn marker(text: &str) -> &str {
    let Some(i) = text.find("marker=") else {
        return "";
    };
    let rest = &text[i + "marker=".len()..];
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `{marker}` in every string of `v` made the run's marker.
pub fn fill(v: &Value, marker: &str) -> Value {
    match v {
        Value::String(s) => Value::String(s.replace("{marker}", marker)),
        Value::Array(a) => Value::Array(a.iter().map(|x| fill(x, marker)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), fill(x, marker)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The step counts the tool results since the opening text, and a
    /// reminder riding beside a result does not open a new turn.
    #[test]
    fn the_step_is_the_tool_results_since_the_opening_text() {
        let req = json!({"messages": [
            {"role": "user", "content": "an earlier turn"},
            {"role": "assistant", "content": "ok"},
            {"role": "user", "content": [{"type": "text", "text": "fix it marker=Q7XK2P please"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "a", "name": "Read", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "a", "content": "x"},
                                          {"type": "text", "text": "<system-reminder>r</system-reminder>"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "b", "name": "Bash", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "b", "content": "y"}]},
        ]});
        let (text, step) = opening_text(&req);
        assert_eq!(text, "fix it marker=Q7XK2P please");
        assert_eq!(step, 2);
        assert_eq!(marker(&text), "Q7XK2P");
        assert_eq!(marker("no marker here"), "");
        let opening_only = json!({"messages": [{"role": "user", "content": "hi marker=a-1."}]});
        assert_eq!(
            opening_text(&opening_only),
            ("hi marker=a-1.".to_string(), 0)
        );
        assert_eq!(marker("hi marker=a-1."), "a-1");
        assert_eq!(opening_text(&json!({})), (String::new(), 0));
    }

    /// Claude Code's shape: a new prompt merged after the last turn's
    /// result, a reminder before it, and reminders beside later results.
    #[test]
    fn a_prompt_merged_after_a_result_opens_the_turn() {
        let req = json!({"messages": [
            {"role": "user", "content": [{"type": "text", "text": "<system-reminder>ctx</system-reminder>"},
                                          {"type": "text", "text": "first marker=A1"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "a", "name": "Read", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "a", "content": "x"},
                                          {"type": "text", "text": "second marker=B2"}]},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "b", "name": "Bash", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "b", "content": "y"},
                                          {"type": "text", "text": "<system-reminder>r</system-reminder>"}]},
        ]});
        assert_eq!(opening_text(&req), ("second marker=B2".to_string(), 1));
    }

    #[test]
    fn a_marker_fills_every_string() {
        let v = json!({"text": "ok-{marker}", "input": {"command": "echo {marker}", "n": 3}});
        let f = fill(&v, "M1");
        assert_eq!(f["text"], "ok-M1");
        assert_eq!(f["input"]["command"], "echo M1");
        assert_eq!(f["input"]["n"], 3);
    }
}
