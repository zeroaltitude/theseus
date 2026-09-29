//! A stand-in for the Messages API on 127.0.0.1, so a bench turn can start
//! a real job through the product's own path (theseus-qa0): a call whose
//! last message carries no tool result asks for `proc.run` of `argv`, and
//! the call that carries its result ends the turn. One response per
//! connection, streamed as the API streams it.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{json, Value};

/// The model name the fake answers as: the template's live profile's.
pub const MODEL: &str = "claude-sonnet-5-5";

pub struct FakeModel {
    pub addr: SocketAddr,
}

impl FakeModel {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start(argv: Vec<String>) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("binding the fake model")?;
        let addr = listener.local_addr()?;
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if let Err(e) = answer(stream, &argv) {
                    eprintln!("fake model: {e:#}");
                }
            }
        });
        Ok(Self { addr })
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }
}

fn answer(mut stream: TcpStream, argv: &[String]) -> Result<()> {
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
    let events = if carries_tool_result(&req) {
        text_turn("Started; it runs in the background.")
    } else {
        tool_turn(argv)
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
}
