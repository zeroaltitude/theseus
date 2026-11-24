//! A stand-in for the Messages API on 127.0.0.1, for tests that drive a real
//! daemon through real turns (theseus-6qy). A call whose last message
//! carries a tool result ends the turn with text. Any other call gets the
//! tool calls `calls` gives for the turn's prompt, the last user text, or
//! text when it gives none. One response per connection, streamed as the
//! API streams it, as the model the request named. Every request's body is
//! kept, in arrival order (theseus-kol), with the time it arrived, parsed
//! and as the bytes that came (theseus-ev1). It can be told to refuse the
//! next requests with an error status, as the API refuses (theseus-ljr), or
//! to answer them as a model that declines, with the `refusal` stop reason
//! (theseus-n88g.2).

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The model name the stand-in answers as when a request names none: the
/// template's live profile's.
const MODEL: &str = "claude-sonnet-5-5";

/// Tool calls for a prompt: each a tool's wire name (`proc_run`) and its input.
pub type Calls = dyn Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync;

#[derive(Default)]
struct Seen {
    requests: Mutex<Vec<(Instant, Value, Vec<u8>)>>,
    /// Each request's connection, the client's port, in the same order
    /// (theseus-jtrc): two requests on one port came on one connection.
    peers: Mutex<Vec<u16>>,
    /// The statuses the next requests get, one each, in order; `u16::MAX`
    /// for every request from then on.
    fails: Mutex<VecDeque<u16>>,
    /// How many of the next requests the model declines (`refusal`).
    refusals: Mutex<u32>,
    /// How many of the next requests get no byte back at all, until the
    /// client gives up and closes (theseus-7gir.21).
    stalls: Mutex<u32>,
    /// While set, a request whose last user text holds this phrase gets no byte back either, as a
    /// provider that has taken the call and not answered: a call in flight, for a test to kill the
    /// daemon under.
    stall_phrase: Mutex<Option<String>>,
    /// The ladder writer's calls (a daemon's background job after each exchange), kept apart from the
    /// turns' so a test counts and reads only what its turns sent, and never answered from its queues.
    writers: Mutex<Vec<Value>>,
    /// What each request was answered with, by its place in `requests`: for a failure to print.
    answers: Mutex<std::collections::HashMap<usize, String>>,
}

pub struct FakeModel {
    pub base: String,
    seen: Arc<Seen>,
}

impl FakeModel {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start(
        calls: impl Fn(&str) -> Vec<(&'static str, Value)> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let calls: Arc<Calls> = Arc::new(calls);
        let seen: Arc<Seen> = Arc::default();
        let shared = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (calls, seen) = (calls.clone(), shared.clone());
                std::thread::spawn(move || {
                    if let Err(e) = answer(stream, &*calls, &seen) {
                        eprintln!("fake model: {e}");
                    }
                });
            }
        });
        Self { base, seen }
    }

    /// Every request's body so far, in arrival order.
    pub fn requests(&self) -> Vec<Value> {
        let r = self.seen.requests.lock().unwrap();
        r.iter().map(|(_, v, _)| v.clone()).collect()
    }

    /// The same bodies, byte for byte as they arrived (theseus-ev1).
    pub fn raw_requests(&self) -> Vec<Vec<u8>> {
        let r = self.seen.requests.lock().unwrap();
        r.iter().map(|(_, _, b)| b.clone()).collect()
    }

    /// When each request arrived, in order.
    pub fn arrivals(&self) -> Vec<Instant> {
        let r = self.seen.requests.lock().unwrap();
        r.iter().map(|(t, _, _)| *t).collect()
    }

    /// The client's port each request came from, in arrival order.
    pub fn peers(&self) -> Vec<u16> {
        self.seen.peers.lock().unwrap().clone()
    }

    /// Refuse the next requests, one per status, with the API's error body
    /// for it (400 `invalid_request_error`, 529 `overloaded_error`, …).
    pub fn fail_next(&self, statuses: &[u16]) {
        self.seen.fails.lock().unwrap().extend(statuses);
    }

    /// Refuse every request from now on with `status`.
    pub fn fail_always(&self, status: u16) {
        let mut f = self.seen.fails.lock().unwrap();
        f.clear();
        f.push_back(status);
        f.push_back(u16::MAX);
    }

    /// Decline the next `n` requests as a model does: a short text that
    /// ends with the `refusal` stop reason and its `stop_details` (category
    /// `cyber`, as b5's were), a 200 answer.
    pub fn decline_next(&self, n: u32) {
        *self.seen.refusals.lock().unwrap() += n;
    }

    /// Answer the next `n` requests with nothing, not even headers, as a
    /// provider that never starts does: the client's first-byte timeout
    /// fails each call (theseus-7gir.21).
    pub fn stall_next(&self, n: u32) {
        *self.seen.stalls.lock().unwrap() += n;
    }

    /// Answer nothing to every request whose last user text holds `phrase`, until `release_stalls`.
    pub fn stall_when(&self, phrase: &str) {
        *self.seen.stall_phrase.lock().unwrap() = Some(phrase.into());
    }

    /// The requests `stall_when` held are answered from now on.
    pub fn release_stalls(&self) {
        *self.seen.stall_phrase.lock().unwrap() = None;
    }

    /// The last `n` requests, for a failure to print: each block of the last message (its first
    /// 120 characters), the prompt the stand-in read from it, and what it answered.
    pub fn last_prompts(&self, n: usize) -> Vec<String> {
        let r = self.seen.requests.lock().unwrap();
        let answers = self.seen.answers.lock().unwrap();
        r.iter()
            .enumerate()
            .skip(r.len().saturating_sub(n))
            .map(|(i, (_, req, _))| {
                let blocks = match req["messages"].as_array().and_then(|m| m.last()) {
                    Some(m) => match &m["content"] {
                        Value::Array(b) => b.iter().map(|b| b.to_string()).collect(),
                        c => vec![c.to_string()],
                    },
                    None => vec![],
                };
                let blocks: Vec<String> = blocks
                    .iter()
                    .map(|b| format!("    block: {}", b.chars().take(120).collect::<String>()))
                    .collect();
                let answer = answers.get(&i).map_or("(not answered yet)", String::as_str);
                format!(
                    "request {i}:\n{}\n    prompt read: {:?}\n    answered: {answer}",
                    blocks.join("\n"),
                    prompt(req)
                )
            })
            .collect()
    }
}

/// The status and body the API refuses with, by status.
fn refusal(status: u16) -> String {
    let (reason, kind, message) = match status {
        400 => (
            "Bad Request",
            "invalid_request_error",
            "model: the model claude-lighthouse-9 is not served",
        ),
        401 => ("Unauthorized", "authentication_error", "invalid x-api-key"),
        429 => ("Too Many Requests", "rate_limit_error", "rate limited"),
        529 => ("Overloaded", "overloaded_error", "Overloaded"),
        _ => (
            "Internal Server Error",
            "api_error",
            "Internal server error",
        ),
    };
    let body = json!({"type": "error", "error": {"type": kind, "message": message}}).to_string();
    format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn answer(mut stream: TcpStream, calls: &Calls, seen: &Seen) -> std::io::Result<()> {
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
    if is_writers(&req) {
        let model = req["model"].as_str().unwrap_or(MODEL).to_string();
        seen.writers.lock().unwrap().push(req);
        stream.write_all(sse(text_turn(&model, "Read the gauge.")).as_bytes())?;
        return stream.flush();
    }
    let port = stream.peer_addr().map_or(0, |a| a.port());
    let at = {
        let mut kept = seen.requests.lock().unwrap();
        kept.push((Instant::now(), req.clone(), body));
        seen.peers.lock().unwrap().push(port);
        kept.len() - 1
    };
    let answered = |what: String| {
        seen.answers.lock().unwrap().insert(at, what);
    };
    let refused = {
        let mut f = seen.fails.lock().unwrap();
        match f.front().copied() {
            Some(s) if f.get(1) == Some(&u16::MAX) => Some(s),
            Some(_) => f.pop_front(),
            None => None,
        }
    };
    if let Some(status) = refused {
        answered(format!("refused {status}"));
        stream.write_all(refusal(status).as_bytes())?;
        return stream.flush();
    }
    let stalls = {
        let mut s = seen.stalls.lock().unwrap();
        let stalls = *s > 0;
        *s = s.saturating_sub(1);
        stalls
    } || seen
        .stall_phrase
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|p| prompt(&req).contains(p.as_str()));
    if stalls {
        answered("nothing (stalled)".into());
        // Nothing back: wait for the client to close (its read sees the end),
        // or the read timeout.
        let _ = r.read(&mut [0u8; 1]);
        return Ok(());
    }
    let model = req["model"].as_str().unwrap_or(MODEL);
    let declines = {
        let mut r = seen.refusals.lock().unwrap();
        let declines = *r > 0;
        *r = r.saturating_sub(1);
        declines
    };
    let events = if declines {
        answered("declined".into());
        declined(model)
    } else if carries_tool_result(&req) {
        answered("text \"Done.\" (it carries a tool result)".into());
        text_turn(model, "Done.")
    } else {
        match calls(&prompt(&req)) {
            c if c.is_empty() => {
                answered("text \"Nothing to do.\" (no call for this prompt)".into());
                text_turn(model, "Nothing to do.")
            }
            c => {
                let names: Vec<&str> = c.iter().map(|(name, _)| *name).collect();
                answered(format!("{} call(s): {}", c.len(), names.join(", ")));
                tool_turn(model, &c)
            }
        }
    };
    stream.write_all(sse(events).as_bytes())?;
    stream.flush()
}

/// A 200 answer streaming `events`.
fn sse(events: Vec<Value>) -> String {
    let mut out = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close\r\n\r\n",
    );
    for e in events {
        out.push_str(&format!(
            "event: {}\ndata: {e}\n\n",
            e["type"].as_str().unwrap_or("")
        ));
    }
    out
}

/// Whether `req` is the ladder writer's: its system text (a block, a string, or a first `system` message
/// on a provider that sends it so) is one of the writer's own.
fn is_writers(req: &Value) -> bool {
    let first = &req["messages"][0];
    let system = [
        &req["system"][0]["text"],
        &req["system"],
        match first["role"] == "system" {
            true => &first["content"],
            false => &Value::Null,
        },
    ];
    system.iter().filter_map(|v| v.as_str()).any(|text| {
        theseus_core::ladder::writer::INSTRUCTIONS
            .iter()
            .any(|i| text.starts_with(i))
    })
}

/// Whether the request's last message answers a tool call.
fn carries_tool_result(req: &Value) -> bool {
    // Nothing was said after the results: the last block that is not one of the harness's tail blocks (the
    // task view, the date, the block beside the message: `is_view`, which `prompt` skips too) is a result. A
    // new message laid after an earlier exchange's result (the view's window) merges into that user message,
    // after it, and is the prompt to act on (issue 44).
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .and_then(|m| m["content"].as_array())
        .and_then(|c| {
            c.iter()
                .rev()
                .find(|b| !theseus_core::task_graph::view::is_view(b))
        })
        .is_some_and(|b| b["type"] == "tool_result")
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
            // The task graph's view (39a) is the harness's, not the prompt.
            .filter(|t| !theseus_core::task_graph::view::is_head(t))
            // So is the surface tag at its head (theseus-slvl.3).
            .map(theseus_core::surface::strip_tag)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn start(model: &str, input_tokens: u64) -> Value {
    json!({"type": "message_start", "message": {"id": "msg_test", "type": "message", "role": "assistant", "model": model, "content": [], "usage": {"input_tokens": input_tokens, "output_tokens": 1}}})
}

fn end(stop_reason: &str) -> [Value; 2] {
    [
        json!({"type": "message_delta", "delta": {"stop_reason": stop_reason}, "usage": {"output_tokens": 12}}),
        json!({"type": "message_stop"}),
    ]
}

fn tool_turn(model: &str, calls: &[(&'static str, Value)]) -> Vec<Value> {
    let mut v = vec![start(model, 40)];
    for (i, (name, input)) in calls.iter().enumerate() {
        v.push(json!({"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": format!("toolu_test_{i}"), "name": name, "input": {}}}));
        v.push(json!({"type": "content_block_delta", "index": i, "delta": {"type": "input_json_delta", "partial_json": input.to_string()}}));
        v.push(json!({"type": "content_block_stop", "index": i}));
    }
    v.extend(end("tool_use"));
    v
}

fn text_turn(model: &str, text: &str) -> Vec<Value> {
    text_turn_ending(model, text, "end_turn")
}

/// A refusal, as the API streams one: its stop reason and its details.
fn declined(model: &str) -> Vec<Value> {
    let mut v = text_turn_ending(model, "I can't help with that.", "refusal");
    let details =
        json!({"type": "refusal", "category": "cyber", "explanation": "declined (stand-in)"});
    let n = v.len();
    v[n - 2]["delta"]["stop_details"] = details;
    v
}

fn text_turn_ending(model: &str, text: &str, stop_reason: &str) -> Vec<Value> {
    let mut v = vec![
        start(model, 60),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": text}}),
        json!({"type": "content_block_stop", "index": 0}),
    ];
    v.extend(end(stop_reason));
    v
}
