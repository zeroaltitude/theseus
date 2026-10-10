//! The stand-in's wire (theseus-7gir.13): HTTP/1.1 with keep-alive, each
//! response streamed as the API streams it, chunked, and paced: its first
//! byte a known time after the request arrived, then its deltas a known time
//! apart, each in a write of its own, with `TCP_NODELAY` so no write waits for
//! the next. Every request is logged on `CLOCK_MONOTONIC`: when it arrived,
//! when its first and last bytes were sent, its sizes, its tools, its model.
//!
//! Unpaced (no [`Pace`]), a response is one write, as before.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// How a response is paced: its first byte `ttfb_ms` after its request
/// arrived, then `chunks` deltas `chunk_ms` apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Pace {
    pub ttfb_ms: u64,
    pub chunks: usize,
    pub chunk_ms: u64,
}

impl Default for Pace {
    /// The head-to-head's: 300 ms to the first byte, 8 chunks 25 ms apart.
    fn default() -> Self {
        Self {
            ttfb_ms: 300,
            chunks: 8,
            chunk_ms: 25,
        }
    }
}

/// `CLOCK_MONOTONIC` in nanoseconds: the clock a driver in another process
/// (Python's `time.monotonic_ns()`) stamps the screen with.
pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: a valid pointer to a timespec; CLOCK_MONOTONIC always exists.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// One request, as the log keeps it: one JSON line each.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The request's place among every request the stand-in took, from 0.
    pub seq: u64,
    /// Its connection, from 0, and its place on it: above 0, the
    /// connection was kept alive and reused.
    pub conn: u64,
    pub conn_req: u64,
    pub path: String,
    /// `CLOCK_MONOTONIC` ns: the request whole (its body read), the first
    /// byte of the response sent, and its last.
    pub arrival_ns: u64,
    pub first_byte_ns: u64,
    pub last_byte_ns: u64,
    pub req_bytes: u64,
    pub resp_bytes: u64,
    pub model: String,
    /// The tools the request offered, by name; none is a side request.
    pub tools: Vec<String>,
    pub side: bool,
    pub stream: bool,
    /// The rule that answered (its `when`), and the step it was at.
    pub rule: Option<String>,
    pub step: usize,
    /// Tool results since the turn's opening user text.
    pub tool_results: usize,
    /// The opening user text's first 120 characters, and the run's marker
    /// in it (the word after `marker=`; empty when none).
    pub opening: String,
    pub marker: String,
    /// The response's writes, each one delta (the first with the
    /// message's start): the chunks.
    pub chunks: usize,
    /// This answer's own pace, over the stand-in's (a rule's).
    #[serde(skip)]
    pub pace: Option<Pace>,
}

/// What a test or a bench watches as the stand-in answers. Each is called on
/// the connection's thread, and may block it (a lockstep stream).
pub trait Watch: Send + Sync {
    /// The request arrived whole.
    fn arrived(&self, _e: &Entry) {}
    /// The first byte is about to be sent.
    fn first_byte(&self, _e: &Entry) {}
    /// Write `k` (from 1, the first byte's included) was sent; the next
    /// waits for this to return.
    fn after_chunk(&self, _e: &Entry, _k: usize) {}
    /// The last byte was sent.
    fn done(&self, _e: &Entry) {}
}

/// Where the log goes: a JSON Lines file, kept in memory too.
#[derive(Default)]
pub struct Log {
    file: Option<Mutex<std::fs::File>>,
    pub entries: Mutex<Vec<Entry>>,
}

impl Log {
    pub fn to_file(path: &std::path::Path) -> Result<Self> {
        let f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening the log {}", path.display()))?;
        Ok(Self {
            file: Some(Mutex::new(f)),
            entries: Mutex::default(),
        })
    }

    fn push(&self, e: &Entry) {
        if let Some(f) = &self.file {
            let mut line = serde_json::to_string(e).unwrap_or_default();
            line.push('\n');
            let _ = f.lock().map(|mut f| f.write_all(line.as_bytes()));
        }
        if let Ok(mut v) = self.entries.lock() {
            v.push(e.clone());
        }
    }
}

/// What every connection shares.
pub struct Shared {
    pub pace: Option<Pace>,
    pub log: Log,
    pub watch: Option<Arc<dyn Watch>>,
    seq: AtomicU64,
    conns: AtomicU64,
}

impl Shared {
    pub fn new(pace: Option<Pace>, log: Log, watch: Option<Arc<dyn Watch>>) -> Self {
        Self {
            pace,
            log,
            watch,
            seq: AtomicU64::new(0),
            conns: AtomicU64::new(0),
        }
    }
}

/// A request read off a connection.
pub struct Request {
    pub path: String,
    pub body: Vec<u8>,
    pub close: bool,
}

/// The next request on `r`, or none when the client closed the connection.
pub fn read_request(r: &mut impl BufRead) -> Result<Option<Request>> {
    let mut line = String::new();
    let (mut len, mut chunked, mut close, mut path) = (0usize, false, false, String::new());
    let mut first = true;
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let l = line.trim_end();
        if first {
            if l.is_empty() {
                continue; // a stray CRLF between requests
            }
            path = l.split_whitespace().nth(1).unwrap_or("").to_string();
            first = false;
            continue;
        }
        if l.is_empty() {
            break;
        }
        let lower = l.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().context("content-length")?;
        } else if let Some(v) = lower.strip_prefix("transfer-encoding:") {
            chunked = v.contains("chunked");
        } else if let Some(v) = lower.strip_prefix("connection:") {
            close = v.trim() == "close";
        }
    }
    let body = if chunked {
        read_chunked(r)?
    } else {
        let mut b = vec![0; len];
        r.read_exact(&mut b)?;
        b
    };
    Ok(Some(Request { path, body, close }))
}

fn read_chunked(r: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        r.read_line(&mut line)?;
        let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16)
            .context("a chunk's size")?;
        if size == 0 {
            // The trailer, to its empty line.
            loop {
                line.clear();
                if r.read_line(&mut line)? == 0 || line.trim().is_empty() {
                    return Ok(body);
                }
            }
        }
        let mut b = vec![0; size + 2];
        r.read_exact(&mut b)?;
        body.extend_from_slice(&b[..size]);
    }
}

/// How the script answers one request: the events, its log fields filled.
pub type Answer = dyn Fn(&Value, &mut Entry) -> Vec<Value> + Send + Sync;

/// Serve one connection until its client closes it: each request answered,
/// paced and logged.
pub fn connection(stream: TcpStream, shared: &Shared, answer: &Answer) -> Result<()> {
    stream.set_nodelay(true)?;
    let conn = shared.conns.fetch_add(1, Ordering::Relaxed);
    let mut r = BufReader::new(stream.try_clone()?);
    let mut w = stream;
    for conn_req in 0.. {
        let Some(req) = read_request(&mut r)? else {
            return Ok(());
        };
        let arrival_ns = mono_ns();
        let mut e = Entry {
            seq: shared.seq.fetch_add(1, Ordering::Relaxed),
            conn,
            conn_req,
            path: req.path.clone(),
            arrival_ns,
            req_bytes: req.body.len() as u64,
            ..Entry::default()
        };
        let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        respond(&mut w, shared, answer, &req, &body, &mut e)?;
        shared.log.push(&e);
        if let Some(watch) = &shared.watch {
            watch.done(&e);
        }
        if req.close {
            return Ok(());
        }
    }
    Ok(())
}

fn respond(
    w: &mut TcpStream,
    shared: &Shared,
    answer: &Answer,
    req: &Request,
    body: &Value,
    e: &mut Entry,
) -> Result<()> {
    e.model = body["model"].as_str().unwrap_or("").to_string();
    e.tools = body["tools"]
        .as_array()
        .map(|t| {
            t.iter()
                .filter_map(|t| t["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    e.side = e.tools.is_empty();
    e.stream = body["stream"].as_bool().unwrap_or(true);
    if req.path.contains("count_tokens") {
        let out = json!({"input_tokens": req.body.len() / 4}).to_string();
        return send_json(w, shared, e, &out);
    }
    let events = answer(body, e);
    if let Some(watch) = &shared.watch {
        watch.arrived(e);
    }
    let pace = e.pace.or(shared.pace);
    if let Some(p) = pace {
        sleep_until(e.arrival_ns + p.ttfb_ms * 1_000_000);
    }
    if !e.stream {
        return send_json(w, shared, e, &whole_message(&events).to_string());
    }
    let writes = match pace {
        Some(p) => group(&split_deltas(&events, p.chunks)),
        None => vec![events],
    };
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\n\
                transfer-encoding: chunked\r\nconnection: keep-alive\r\n\r\n";
    e.chunks = writes.len();
    if let Some(watch) = &shared.watch {
        watch.first_byte(e);
    }
    e.first_byte_ns = mono_ns();
    let mut sent = 0u64;
    for (k, events) in writes.iter().enumerate() {
        if k > 0 {
            if let Some(p) = pace {
                sleep_until(e.first_byte_ns + k as u64 * p.chunk_ms * 1_000_000);
            }
        }
        let sse: String = events
            .iter()
            .map(|ev| {
                format!(
                    "event: {}\ndata: {ev}\n\n",
                    ev["type"].as_str().unwrap_or("")
                )
            })
            .collect();
        let mut out = if k == 0 {
            head.to_string()
        } else {
            String::new()
        };
        out.push_str(&format!("{:x}\r\n{sse}\r\n", sse.len()));
        if k + 1 == writes.len() {
            out.push_str("0\r\n\r\n");
        }
        w.write_all(out.as_bytes())?;
        sent += out.len() as u64;
        if let Some(watch) = &shared.watch {
            watch.after_chunk(e, k + 1);
        }
    }
    w.flush()?;
    e.last_byte_ns = mono_ns();
    e.resp_bytes = sent;
    Ok(())
}

fn send_json(w: &mut TcpStream, shared: &Shared, e: &mut Entry, out: &str) -> Result<()> {
    if let Some(watch) = &shared.watch {
        watch.first_byte(e);
    }
    let msg = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
         connection: keep-alive\r\n\r\n{out}",
        out.len()
    );
    e.first_byte_ns = mono_ns();
    w.write_all(msg.as_bytes())?;
    w.flush()?;
    e.last_byte_ns = mono_ns();
    e.resp_bytes = msg.len() as u64;
    Ok(())
}

/// Sleep until `CLOCK_MONOTONIC` reads `ns` (now, if it has).
fn sleep_until(ns: u64) {
    let now = mono_ns();
    if ns > now {
        std::thread::sleep(Duration::from_nanos(ns - now));
    }
}

/// The streamed events as one message, for a request that asked for no
/// stream (`"stream": false`).
pub fn whole_message(events: &[Value]) -> Value {
    let mut msg = events
        .first()
        .map(|e| e["message"].clone())
        .unwrap_or_else(|| json!({}));
    let mut content: Vec<Value> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    for ev in events {
        match ev["type"].as_str() {
            Some("content_block_start") => {
                content.push(ev["content_block"].clone());
                partial.push(String::new());
            }
            Some("content_block_delta") => {
                let (Some(block), Some(p)) = (content.last_mut(), partial.last_mut()) else {
                    continue;
                };
                if let Some(t) = ev["delta"]["text"].as_str() {
                    let was = block["text"].as_str().unwrap_or("").to_string();
                    block["text"] = json!(was + t);
                }
                if let Some(j) = ev["delta"]["partial_json"].as_str() {
                    p.push_str(j);
                }
            }
            Some("message_delta") => {
                msg["stop_reason"] = ev["delta"]["stop_reason"].clone();
            }
            _ => {}
        }
    }
    for (block, p) in content.iter_mut().zip(partial) {
        if block["type"] == "tool_use" {
            block["input"] = serde_json::from_str(&p).unwrap_or_else(|_| json!({}));
        }
    }
    msg["content"] = json!(content);
    msg
}

/// The events with their deltas cut into `n` deltas in all (each text and
/// each tool input cut in proportion; a text's first delta holds at least its
/// first word, so a marker there is on screen with the first text).
pub fn split_deltas(events: &[Value], n: usize) -> Vec<Value> {
    let deltas: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["type"] == "content_block_delta")
        .map(|(i, _)| i)
        .collect();
    if deltas.is_empty() || n == 0 {
        return events.to_vec();
    }
    // Each delta's share of n: at least one, the rest to the first.
    let mut shares = vec![(n / deltas.len()).max(1); deltas.len()];
    let given: usize = shares.iter().sum();
    if n > given {
        shares[0] += n - given;
    }
    let mut out = Vec::new();
    let mut d = 0;
    for (i, e) in events.iter().enumerate() {
        if deltas.get(d) != Some(&i) {
            out.push(e.clone());
            continue;
        }
        let (key, text) = if let Some(t) = e["delta"]["text"].as_str() {
            ("text", t)
        } else {
            (
                "partial_json",
                e["delta"]["partial_json"].as_str().unwrap_or(""),
            )
        };
        for piece in cut(text, shares[d], key == "text") {
            let mut c = e.clone();
            c["delta"][key] = json!(piece);
            out.push(c);
        }
        d += 1;
    }
    out
}

/// `text` in `n` pieces of about equal length, cut at character boundaries;
/// with `word_first`, the first piece runs at least to the first space.
/// Fewer pieces when the text is shorter than `n` characters.
pub fn cut(text: &str, n: usize, word_first: bool) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    if n <= 1 || len <= 1 {
        return vec![text.to_string()];
    }
    let first_word = if word_first {
        chars.iter().position(|c| *c == ' ').unwrap_or(len)
    } else {
        0
    };
    let mut bounds = Vec::new();
    let mut last = 0;
    for i in 1..n {
        let mut b = (i * len).div_ceil(n);
        if i == 1 {
            b = b.max(first_word);
        }
        if b > last && b < len {
            bounds.push(b);
        }
        last = last.max(b);
    }
    bounds.push(len);
    let mut out = Vec::new();
    let mut from = 0;
    for b in bounds {
        out.push(chars[from..b].iter().collect());
        from = b;
    }
    out
}

/// The events grouped into writes: the first write runs to the first delta
/// (the message's start with it), each later write is one delta, and what
/// follows the last delta rides with it.
pub fn group(events: &[Value]) -> Vec<Vec<Value>> {
    let mut writes: Vec<Vec<Value>> = vec![Vec::new()];
    let mut seen_delta = false;
    for e in events {
        let delta = e["type"] == "content_block_delta";
        if delta && seen_delta {
            writes.push(Vec::new());
        }
        seen_delta |= delta;
        writes
            .last_mut()
            .expect("one write at least")
            .push(e.clone());
    }
    writes
}
