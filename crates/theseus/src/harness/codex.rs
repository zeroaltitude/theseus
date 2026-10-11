//! Codex's rollout files (JSON Lines, one per session, under
//! `~/.codex/sessions/<year>/<month>/<day>/rollout-<time>-<id>.jsonl`).
//!
//! A line is `{timestamp, type, payload}`. `session_meta` holds the session's
//! `cwd` and git branch. `response_item` is the conversation: its payload's
//! `type` is `message` (a `role`, and `content` blocks of `input_text`,
//! `output_text`), `function_call` (`name`, `arguments`),
//! `function_call_output`, the custom and local-shell forms of both, or
//! `reasoning`, which is skipped like thinking. `event_msg`, `turn_context`
//! and `compacted` repeat or frame what the response items hold: skipped,
//! counted. An older rollout has the items bare on the line, with no
//! envelope; they read the same, at the session's last known time.
//!
//! A user message that is the harness's injected context
//! (`<environment_context>`, the instructions of an AGENTS.md) is skipped,
//! counted as `injected_context`: it is not the person's words, and every
//! session repeats it.

use serde_json::Value;

use super::{bounded, call_line, result_line, time_ms, Counts, Draft, Msg, TEXT_MAX};

/// What a rollout's reader remembers between lines.
#[derive(Default)]
pub(super) struct State {
    /// The newest time a line named, for a line that names none.
    last_ms: Option<i64>,
}

const INJECTED: &[&str] = &[
    "<environment_context>",
    "<user_instructions>",
    "<user_action>",
    "<permissions instructions>",
    "# AGENTS.md instructions",
];

pub(super) fn line(v: &Value, number: u64, d: &mut Draft, counts: &mut Counts, st: &mut State) {
    if let Some(ms) = v.get("timestamp").and_then(time_ms) {
        st.last_ms = Some(ms);
    }
    let kind = v.get("type").and_then(Value::as_str);
    let (kind, item) = match (kind, v.get("payload")) {
        (Some("response_item"), Some(p)) => (p.get("type").and_then(Value::as_str), p),
        (Some("session_meta"), Some(p)) => return meta(p, d),
        (Some(other), Some(_)) => return counts.skip(other),
        // An older rollout: the item is the line, and its first line (no type,
        // an id and instructions) is the session's own.
        (Some(_), None) => (kind, v),
        (None, _) if v.get("id").is_some() => return meta(v, d),
        (None, _) => return counts.skip("(no type)"),
    };
    let Some(kind) = kind else {
        return counts.skip("(no type)");
    };
    let call = |name: &str, input: &Value, counts: &mut Counts| {
        ("agent:codex", "agent", call_line(name, input, counts))
    };
    let made: Vec<(&str, &'static str, String)> = match kind {
        "message" => {
            let role = item.get("role").and_then(Value::as_str).unwrap_or("");
            let (author, integrity) = match role {
                "user" => ("user", "operator"),
                "assistant" => ("agent:codex", "agent"),
                other => return counts.skip(&format!("message:{other}")),
            };
            let mut texts = Vec::new();
            for b in item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                match b.get("type").and_then(Value::as_str).unwrap_or("") {
                    "input_text" | "output_text" | "text" => {
                        let t = b.get("text").and_then(Value::as_str).unwrap_or("");
                        if t.trim().is_empty() {
                            continue;
                        }
                        if role == "user" && INJECTED.iter().any(|p| t.trim_start().starts_with(p))
                        {
                            counts.skip("injected_context");
                            continue;
                        }
                        texts.push((author, integrity, bounded(t, TEXT_MAX, counts)));
                    }
                    "input_image" => counts.skip("image"),
                    other => counts.skip(&format!("block:{other}")),
                }
            }
            texts
        }
        "function_call" | "custom_tool_call" => {
            let name = item.get("name").and_then(Value::as_str).unwrap_or("?");
            let input = match item.get("arguments").or_else(|| item.get("input")) {
                // Arguments are a JSON string: shown as the string.
                Some(a) => a.clone(),
                None => Value::Null,
            };
            vec![call(name, &input, counts)]
        }
        "local_shell_call" => {
            let input = item
                .get("action")
                .and_then(|a| a.get("command"))
                .cloned()
                .unwrap_or(Value::Null);
            vec![call("shell", &input, counts)]
        }
        "web_search_call" => {
            let input = item.get("action").cloned().unwrap_or(Value::Null);
            vec![call("web_search", &input, counts)]
        }
        "function_call_output" | "custom_tool_call_output" => {
            let text = output_text(item.get("output"));
            vec![("tool", "outside", result_line(&text, false, counts))]
        }
        "reasoning" => return counts.skip("thinking"),
        other => return counts.skip(other),
    };
    if made.is_empty() {
        return;
    }
    let Some(at_ms) = v.get("timestamp").and_then(time_ms).or(st.last_ms) else {
        counts
            .rejected
            .push((number, "a message line with no readable timestamp".into()));
        return;
    };
    for (author, integrity, text) in made {
        d.msgs.push(Msg {
            author: author.to_string(),
            integrity,
            text,
            at_ms,
            line: number,
        });
    }
}

/// The session's own line: its directory and branch.
fn meta(p: &Value, d: &mut Draft) {
    if d.cwd.is_none() {
        d.cwd = p
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
    if d.branch.is_none() {
        d.branch = p
            .get("git")
            .and_then(|g| g.get("branch"))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
    }
}

/// A call's output: a string (which may itself be the JSON of
/// `{output, metadata}`), or an object with `content` or `output`.
fn output_text(o: Option<&Value>) -> String {
    match o {
        Some(Value::String(s)) => match serde_json::from_str::<Value>(s) {
            Ok(Value::Object(m)) if m.contains_key("output") => output_text(m.get("output")),
            _ => s.clone(),
        },
        Some(Value::Object(m)) => output_text(m.get("content").or_else(|| m.get("output"))),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
