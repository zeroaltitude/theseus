//! A stand-in for the Messages API on 127.0.0.1, for tests that drive a real
//! daemon through real turns (theseus-6qy). A call whose last message
//! carries a tool result ends the turn with text. Any other call gets the
//! tool calls `calls` gives for the turn's prompt, the last user text, or
//! text when it gives none. One response per connection, streamed as the
//! API streams it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

/// The model name the stand-in answers as: the template's live profile's.
const MODEL: &str = "claude-sonnet-5-5";

/// Tool calls for a prompt: each a tool's wire name (`proc_run`) and its input.
pub type Calls = dyn Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync;

pub struct FakeModel {
    pub base: String,
}

impl FakeModel {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start(
        calls: impl Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let calls: Arc<Calls> = Arc::new(calls);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let calls = calls.clone();
                std::thread::spawn(move || {
                    if let Err(e) = answer(stream, &*calls) {
                        eprintln!("fake model: {e}");
                    }
                });
            }
        });
        Self { base }
    }
}

fn answer(mut stream: TcpStream, calls: &Calls) -> std::io::Result<()> {
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
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let req: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let events = if carries_tool_result(&req) {
        text_turn("Done.")
    } else {
        match calls(&prompt(&req)) {
            c if c.is_empty() => text_turn("Nothing to do."),
            c => tool_turn(&c),
        }
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
    stream.flush()
}

/// Whether the request's last message answers a tool call.
fn carries_tool_result(req: &Value) -> bool {
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| m["content"].as_array())
        .is_some_and(|c| c.iter().any(|b| b["type"] == "tool_result"))
}

/// The last user message's text.
fn prompt(req: &Value) -> String {
    let Some(m) = req["messages"].as_array().and_then(|m| m.last()) else {
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

fn start(input_tokens: u64) -> Value {
    json!({"type": "message_start", "message": {"id": "msg_test", "type": "message", "role": "assistant", "model": MODEL, "content": [], "usage": {"input_tokens": input_tokens, "output_tokens": 1}}})
}

fn end(stop_reason: &str) -> [Value; 2] {
    [
        json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}, "usage": {"output_tokens": 12}}),
        json!({"type": "message_stop"}),
    ]
}

fn tool_turn(calls: &[(&'static str, Value)]) -> Vec<Value> {
    let mut v = vec![start(40)];
    for (i, (name, input)) in calls.iter().enumerate() {
        v.push(json!({"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": format!("toolu_test_{i}"), "name": name, "input": {}}}));
        v.push(json!({"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": input.to_string()}}));
        v.push(json!({"type": "content_block_stop", "index": i}));
    }
    v.extend(end("tool_use"));
    v
}

fn text_turn(text: &str) -> Vec<Value> {
    let mut v = vec![
        start(60),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    v.extend(end("end_turn"));
    v
}
