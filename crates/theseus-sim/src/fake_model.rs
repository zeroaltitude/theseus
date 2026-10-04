//! A stand-in for the Messages API on 127.0.0.1, so a bench turn can start
//! a real job through the product's own path (theseus-qa0): a call whose
//! last message carries no tool result asks for `proc.run` of `argv`, and
//! the call that carries its result ends the turn. One response per
//! connection, streamed as the API streams it.
//!
//! The turn bench's stand-in (`FakeModel::start_mixed`, theseus-goa8) asks for
//! its tool only when the input holds [`TOOL_MARK`]; any other turn is one
//! plain answer, so a bench can time both kinds on one daemon.
//!
//! `theseus-sim fake-model --rules <file>` (`FakeModel::start_rules_on`, 37b)
//! serves a scripted stand-in for a scratch daemon's live check: each turn's
//! last user text is matched against the rules, in order, and the first that
//! it holds asks for its tool calls or answers its text.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

/// The model name the fake answers as: the template's live profile's.
pub const MODEL: &str = "claude-sonnet-5-5";

/// What a mixed stand-in looks for in a turn's input: with it, the model asks
/// for its tool; without it, the model answers in plain text.
pub const TOOL_MARK: &str = "bench-tool";

/// What the stand-in answers a call that carries no tool result.
#[derive(Clone)]
enum Script {
    /// Always ask for `argv` (the lifecycle bench's job).
    Job(Vec<String>),
    /// Ask for `argv` when the input holds [`TOOL_MARK`], else answer in text.
    Mixed(Vec<String>),
    /// The first rule whose `when` the turn's last user text holds.
    Rules(Vec<Rule>),
}

/// One rule of a scripted stand-in (`fake-model --rules`, a JSON array of
/// them): when the turn's last user text holds `when`, ask for `calls`, or,
/// with none, answer `text`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub when: String,
    #[serde(default)]
    pub calls: Vec<Call>,
    #[serde(default)]
    pub text: Option<String>,
}

/// A tool call a rule asks for: the tool's wire name (`task_create`) and its
/// input.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub name: String,
    pub input: Value,
}

pub struct FakeModel {
    pub addr: SocketAddr,
}

impl FakeModel {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start(argv: Vec<String>) -> Result<Self> {
        Self::listen(Script::Job(argv))
    }

    /// As [`FakeModel::start`], but a turn asks for `argv` only when its
    /// input holds [`TOOL_MARK`], and is otherwise one plain answer.
    pub fn start_mixed(argv: Vec<String>) -> Result<Self> {
        Self::listen(Script::Mixed(argv))
    }

    fn listen(script: Script) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("binding the fake model")?;
        Self::serve(listener, script)
    }

    fn serve(listener: TcpListener, script: Script) -> Result<Self> {
        let addr = listener.local_addr()?;
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Err(e) = answer(stream, &script) {
                    eprintln!("fake model: {e:#}");
                }
            }
        });
        Ok(Self { addr })
    }

    /// A scripted stand-in on `addr` (`fake-model --rules`): see [`Rule`].
    pub fn start_rules_on(addr: &str, rules: Vec<Rule>) -> Result<Self> {
        let listener = TcpListener::bind(addr).with_context(|| format!("binding {addr}"))?;
        Self::serve(listener, Script::Rules(rules))
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }
}

fn answer(mut stream: TcpStream, script: &Script) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut r = BufReader::new(stream.try_clone()?);
    let mut len = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().context("content-length")?;
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let req: Value = serde_json::from_slice(&body).context("request body")?;
    let events = match (script, carries_tool_result(&req)) {
        (Script::Job(_), true) => text_turn("Started; it runs in the background."),
        (Script::Mixed(_), true) => text_turn("The tool ran."),
        (Script::Job(argv), false) => tool_turn(argv),
        (Script::Mixed(argv), false) if asks_for_tool(&req) => tool_turn(argv),
        (Script::Mixed(_), false) => text_turn("A plain answer from the stand-in model."),
        (Script::Rules(_), true) => text_turn("Done."),
        (Script::Rules(rules), false) => ruled_turn(rules, &last_user_text(&req)),
    };
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
    );
    for e in events {
        out.push_str(&format!(
            "event: {}\ndata: {e}\n\n",
            e["type"].as_str().unwrap_or("")
        ));
    }
    stream.write_all(out.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// Whether the request's last message answers a tool call.
pub fn carries_tool_result(req: &Value) -> bool {
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// Whether the request's last message, the turn's input, holds [`TOOL_MARK`].
pub fn asks_for_tool(req: &Value) -> bool {
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .is_some_and(|m| m["content"].to_string().contains(TOOL_MARK))
}

/// The text of the request's last user message: a string, or its text
/// blocks joined.
pub fn last_user_text(req: &Value) -> String {
    let Some(m) = req["messages"]
        .as_array()
        .and_then(|m| m.iter().rev().find(|m| m["role"] == "user"))
    else {
        return String::new();
    };
    match &m["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// What the first rule `text` holds says: its calls, or its text; with none,
/// a plain answer.
pub fn ruled_turn(rules: &[Rule], text: &str) -> Vec<Value> {
    match rules.iter().find(|r| text.contains(&r.when)) {
        Some(r) if !r.calls.is_empty() => calls_turn(&r.calls),
        Some(r) => text_turn(r.text.as_deref().unwrap_or("Done.")),
        None => text_turn("A plain answer from the stand-in model."),
    }
}

/// `calls`, one tool_use block each.
pub fn calls_turn(calls: &[Call]) -> Vec<Value> {
    let mut v = vec![start(40)];
    for (i, c) in calls.iter().enumerate() {
        v.push(json!({"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": format!("toolu_fake_{i}"), "name": c.name, "input": {}}}));
        v.push(json!({"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": c.input.to_string()}}));
        v.push(json!({"type": "content_block_stop", "index": i}));
    }
    v.extend(end("tool_use"));
    v
}

fn start(input_tokens: u64) -> Value {
    json!({"type": "message_start", "message": {"id": "msg_bench", "type": "message", "role": "assistant", "model": MODEL, "content": [], "usage": {"input_tokens": input_tokens, "output_tokens": 1}}})
}

fn end(stop_reason: &str) -> [Value; 2] {
    [
        json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}, "usage": {"output_tokens": 12}}),
        json!({"type": "message_stop"}),
    ]
}

/// `proc.run` of `argv`, as one tool call.
pub fn tool_turn(argv: &[String]) -> Vec<Value> {
    let input = json!({"argv": argv, "timeout_secs": 3600}).to_string();
    let mut v = vec![
        start(40),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "toolu_bench_job", "name": "proc_run", "input": {}}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "input_json_delta", "partial_json": input}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    v.extend(end("tool_use"));
    v
}

pub fn text_turn(text: &str) -> Vec<Value> {
    let mut v = vec![
        start(60),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    v.extend(end("end_turn"));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_result_ends_the_turn_and_anything_else_asks_for_the_job() {
        let asks = json!({"messages": [{"role": "user", "content": "go"}]});
        let answered = json!({"messages": [
            {"role": "user", "content": "go"},
            {"role": "assistant", "content": [{"type": "tool_use", "id": "t", "name": "proc_run", "input": {}}]},
            {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t", "content": "running"}]},
        ]});
        assert!(!carries_tool_result(&asks));
        assert!(carries_tool_result(&answered));
        let call = tool_turn(&["sleep".into(), "600".into()]);
        let input: Value =
            serde_json::from_str(call[2]["delta"]["partial_json"].as_str().unwrap()).unwrap();
        assert_eq!(input["argv"], json!(["sleep", "600"]));
        assert_eq!(call[4]["delta"]["stop_reason"], "tool_use");
        assert_eq!(call[5]["type"], "message_stop");
        assert_eq!(text_turn("x")[4]["delta"]["stop_reason"], "end_turn");
    }

    #[test]
    fn a_mixed_stand_in_asks_for_its_tool_only_when_the_input_says_so() {
        let plain = json!({"messages": [{"role": "user", "content": "hello"}]});
        let marked = json!({"messages": [
            {"role": "user", "content": [{"type": "text", "text": "do it, bench-tool please"}]},
        ]});
        assert!(!asks_for_tool(&plain));
        assert!(asks_for_tool(&marked));
        assert!(!asks_for_tool(&json!({})), "no messages is no ask");
    }

    /// The scripted stand-in (37b): the first rule the last user text holds
    /// asks for its calls, or answers its text; none, a plain answer.
    #[test]
    fn a_ruled_stand_in_asks_for_the_first_matching_rules_calls() {
        let rules: Vec<Rule> = serde_json::from_value(json!([
            {"when": "⏰ wake", "text": "Checked again: green."},
            {"when": "Watch the build", "calls": [
                {"name": "task_create", "input": {"brief": "check twice"}},
                {"name": "wake_at", "input": {"after": "1m", "note": "again"}}
            ]}
        ]))
        .unwrap();
        let req = json!({"messages": [
            {"role": "user", "content": [{"type": "text", "text": "Watch the build, please"}]},
        ]});
        assert_eq!(last_user_text(&req), "Watch the build, please");
        let call = ruled_turn(&rules, &last_user_text(&req));
        assert_eq!(call[1]["content_block"]["name"], "task_create");
        assert_eq!(call[4]["content_block"]["name"], "wake_at");
        let input: Value =
            serde_json::from_str(call[5]["delta"]["partial_json"].as_str().unwrap()).unwrap();
        assert_eq!(input["after"], "1m");
        assert_eq!(call[7]["delta"]["stop_reason"], "tool_use");
        let woke = ruled_turn(&rules, "⏰ wake (set 13:05): again");
        assert_eq!(woke[2]["delta"]["text"], "Checked again: green.");
        let plain = ruled_turn(&rules, "hello");
        assert_eq!(
            plain[2]["delta"]["text"],
            "A plain answer from the stand-in model."
        );
        assert!(serde_json::from_value::<Vec<Rule>>(json!([{"when": "x", "cals": []}])).is_err());
    }
}
