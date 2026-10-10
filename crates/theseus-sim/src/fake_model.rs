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

use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};

mod serve;
mod steps;
#[cfg(test)]
mod tests_paced;

use serve::Shared;
pub use serve::{mono_ns, Entry, Log, Pace, Watch};

/// The model name the fake answers as: the template's live profile's.
pub const MODEL: &str = "claude-sonnet-5-5";

/// What a mixed stand-in looks for in a turn's input: with it, the model asks
/// for its tool; without it, the model answers in plain text.
pub const TOOL_MARK: &str = "bench-tool";

/// What a mixed stand-in streams paced (theseus-7gir.13): with it in the
/// input, the answer is [`STREAM_TEXT`], its first byte and its chunks paced
/// as [`Pace::default`] says, so a bench can count what happens before the
/// first byte and how the deltas reach a client.
pub const STREAM_MARK: &str = "bench-stream";

/// The paced answer: long enough for [`Pace::default`]'s 8 chunks.
pub const STREAM_TEXT: &str =
    "streamed: the stand-in model answers in eight chunks, twenty-five milliseconds apart, one write each.";

/// What a scripted stand-in answers a request that offers no tools: a side
/// request (a harness's small-model calls: a title, a summary).
pub const SIDE_TEXT: &str = "A side answer from the stand-in model.";

/// What the stand-in answers a call that carries no tool result.
#[derive(Clone)]
enum Script {
    /// Always ask for `argv` (the lifecycle bench's job).
    Job(Vec<String>),
    /// Ask for `argv` when the input holds [`TOOL_MARK`], else answer in text
    /// ([`STREAM_TEXT`], paced, when it holds [`STREAM_MARK`]).
    Mixed(Vec<String>),
    /// The first rule whose `when` the turn's text holds.
    Rules(Vec<Rule>),
}

/// One rule of a scripted stand-in (`fake-model --rules`, a JSON array of
/// them): when the turn's last user text holds `when`, ask for `calls`, or,
/// with none, answer `text`, after `hold_ms` (0 by default), so a live check
/// can find the call in flight (theseus-f3wr).
///
/// With `steps` (theseus-7gir.13), `when` is matched on the turn's opening
/// user text, and step k of the turn (k tool results since that text) answers
/// `steps[k]` (past the end, the last): a whole turn's calls and its answer,
/// for any harness. `{marker}` in a step's strings is the word after
/// `marker=` in the opening text. `ttfb_ms`, `chunks` and `chunk_ms` pace
/// the rule's answers, over the stand-in's own pace.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub when: String,
    #[serde(default)]
    pub calls: Vec<Call>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub hold_ms: u64,
    #[serde(default)]
    pub steps: Vec<Step>,
    #[serde(default)]
    pub ttfb_ms: Option<u64>,
    #[serde(default)]
    pub chunks: Option<usize>,
    #[serde(default)]
    pub chunk_ms: Option<u64>,
}

impl Rule {
    /// The rule's own pace, over `base` (the stand-in's, else the default).
    fn pace(&self, base: Option<Pace>) -> Option<Pace> {
        if self.ttfb_ms.is_none() && self.chunks.is_none() && self.chunk_ms.is_none() {
            return None;
        }
        let b = base.unwrap_or_default();
        Some(Pace {
            ttfb_ms: self.ttfb_ms.unwrap_or(b.ttfb_ms),
            chunks: self.chunks.unwrap_or(b.chunks),
            chunk_ms: self.chunk_ms.unwrap_or(b.chunk_ms),
        })
    }
}

/// One step of a rule's turn: its calls, or, with none, its text.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
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

/// How a stand-in serves: its pace (none: each answer one write), its log,
/// and what watches it.
#[derive(Default)]
pub struct Serving {
    pub pace: Option<Pace>,
    pub log: Log,
    pub watch: Option<Arc<dyn Watch>>,
}

pub struct FakeModel {
    pub addr: SocketAddr,
    shared: Arc<Shared>,
}

impl FakeModel {
    /// Listen on an ephemeral port; answer until the process ends.
    pub fn start(argv: Vec<String>) -> Result<Self> {
        Self::listen(Script::Job(argv), Serving::default())
    }

    /// As [`FakeModel::start`], but a turn asks for `argv` only when its
    /// input holds [`TOOL_MARK`], and is otherwise one plain answer; watched
    /// by `watch` (the turn bench's counted rows).
    pub fn start_mixed_watched(argv: Vec<String>, watch: Arc<dyn Watch>) -> Result<Self> {
        let serving = Serving {
            watch: Some(watch),
            ..Serving::default()
        };
        Self::listen(Script::Mixed(argv), serving)
    }

    fn listen(script: Script, serving: Serving) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").context("binding the fake model")?;
        Self::serve(listener, script, serving)
    }

    fn serve(listener: TcpListener, script: Script, serving: Serving) -> Result<Self> {
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared::new(serving.pace, serving.log, serving.watch));
        let served = shared.clone();
        let turns: Arc<Turns> = Arc::default();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                // Each connection its own thread: a held rule holds no other
                // call, and a connection kept alive holds no other client.
                let (script, shared, turns) = (script.clone(), served.clone(), turns.clone());
                std::thread::spawn(move || {
                    let base = shared.pace;
                    let answer =
                        move |req: &Value, e: &mut Entry| answer(&script, base, &turns, req, e);
                    if let Err(e) = serve::connection(stream, &shared, &answer) {
                        eprintln!("fake model: {e:#}");
                    }
                });
            }
        });
        Ok(Self { addr, shared })
    }

    /// A scripted stand-in on `addr` (`fake-model --rules`): see [`Rule`].
    #[cfg(test)]
    pub fn start_rules_on(addr: &str, rules: Vec<Rule>) -> Result<Self> {
        Self::start_rules_with(addr, rules, Serving::default())
    }

    /// [`FakeModel::start_rules_on`], paced and logged as `serving` says.
    pub fn start_rules_with(addr: &str, rules: Vec<Rule>, serving: Serving) -> Result<Self> {
        let listener = TcpListener::bind(addr).with_context(|| format!("binding {addr}"))?;
        Self::serve(listener, Script::Rules(rules), serving)
    }

    pub fn base(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Every request answered so far, once there are at least `n` (or 5 s
    /// passed): an entry is logged just after its last byte is sent, so a
    /// client that has read the answer may be ahead of it.
    pub fn entries_at_least(&self, n: usize) -> Vec<Entry> {
        let until = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let e = self.entries();
            if e.len() >= n || std::time::Instant::now() > until {
                return e;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Every request answered so far, in the order they were answered.
    pub fn entries(&self) -> Vec<Entry> {
        self.shared
            .log
            .entries
            .lock()
            .map(|v| v.clone())
            .unwrap_or_default()
    }
}

/// The requests a stepped rule has answered for each run's marker: a turn's
/// step, where its messages cannot say it (Claude Code merges its messages
/// by role, so a turn's results sit before its prompt).
type Turns = std::sync::Mutex<std::collections::HashMap<String, usize>>;

/// The events that answer `req`, and its log entry's fields.
fn answer(
    script: &Script,
    base: Option<Pace>,
    turns: &Turns,
    req: &Value,
    e: &mut Entry,
) -> Vec<Value> {
    let (opening, step) = steps::opening_text(req);
    e.opening = opening.chars().take(120).collect();
    e.marker = steps::marker(&opening).to_string();
    e.step = step;
    e.tool_results = step;
    match (script, carries_tool_result(req)) {
        (Script::Job(_), true) => text_turn("Started; it runs in the background."),
        (Script::Mixed(_), true) => text_turn("The tool ran."),
        (Script::Job(argv), false) => tool_turn(argv),
        (Script::Mixed(argv), false) if asks_for_tool(req) => tool_turn(argv),
        (Script::Mixed(_), false) if streams(req) => {
            e.pace = Some(base.unwrap_or_default());
            text_turn(STREAM_TEXT)
        }
        (Script::Mixed(_), false) => text_turn("A plain answer from the stand-in model."),
        // A side request, where the rules script whole turns: a harness's
        // small-model call, never a step of the turn.
        (Script::Rules(rules), _) if e.side && rules.iter().any(|r| !r.steps.is_empty()) => {
            text_turn(SIDE_TEXT)
        }
        (Script::Rules(rules), result) => {
            let stepped = rules
                .iter()
                .find(|r| !r.steps.is_empty() && opening.contains(&r.when));
            if let Some(r) = stepped {
                e.rule = Some(r.when.clone());
                // With a marker, the step is the requests answered for it.
                let step = if e.marker.is_empty() {
                    step
                } else {
                    let mut t = turns
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let n = t.entry(e.marker.clone()).or_insert(0);
                    *n += 1;
                    *n - 1
                };
                e.step = step;
                e.pace = r.pace(base);
                let s = &r.steps[step.min(r.steps.len() - 1)];
                let s = steps::fill(
                    &serde_json::to_value(StepOut::of(s)).unwrap_or_default(),
                    steps::marker(&opening),
                );
                return step_turn(&s);
            }
            if result {
                return text_turn("Done.");
            }
            let text = last_user_text(req);
            let hit = rules.iter().find(|r| text.contains(&r.when));
            e.rule = hit.map(|r| r.when.clone());
            e.pace = hit.and_then(|r| r.pace(base));
            if let Some(ms) = hit.map(|r| r.hold_ms).filter(|&ms| ms > 0) {
                std::thread::sleep(Duration::from_millis(ms));
            }
            ruled_turn(rules, &text)
        }
    }
}

/// A step as JSON, so its strings can take the marker.
#[derive(serde::Serialize)]
struct StepOut {
    calls: Vec<Value>,
    text: Option<String>,
}

impl StepOut {
    fn of(s: &Step) -> Self {
        Self {
            calls: s
                .calls
                .iter()
                .map(|c| json!({"name": c.name, "input": c.input}))
                .collect(),
            text: s.text.clone(),
        }
    }
}

/// A filled step's turn: its calls, or its text.
fn step_turn(s: &Value) -> Vec<Value> {
    let calls: Vec<Call> = serde_json::from_value(s["calls"].clone()).unwrap_or_default();
    if calls.is_empty() {
        text_turn(s["text"].as_str().unwrap_or("Done."))
    } else {
        calls_turn(&calls)
    }
}

/// Whether the request's last message, the turn's input, holds
/// [`STREAM_MARK`].
pub fn streams(req: &Value) -> bool {
    req["messages"]
        .as_array()
        .and_then(|m| m.last())
        .is_some_and(|m| m["content"].to_string().contains(STREAM_MARK))
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
            // The task graph's view (39a) is the harness's, not the user's.
            .filter(|t| !t.starts_with(theseus_core::task_graph::view::HEAD))
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

/// `calls`, one tool_use block each. Each block's id is new, as the API's
/// are, across turns and across restarts of the stand-in (the time and a
/// count): a session's later turn reusing an earlier id read as answered,
/// and an approved call of the later one never ran (39b's live check).
pub fn calls_turn(calls: &[Call]) -> Vec<Value> {
    static TURNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = format!(
        "{:x}{:x}",
        theseus_protocol::now_unix_ms(),
        TURNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let mut v = vec![start(40)];
    for (i, c) in calls.iter().enumerate() {
        v.push(json!({"type": "content_block_start", "index": i, "content_block": {"type": "tool_use", "id": format!("toolu_fake_{n}_{i}"), "name": c.name, "input": {}}}));
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
    use std::io::{Read, Write};
    use std::net::TcpStream;

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

    /// A rule's `hold_ms` holds its answer that long, and holds no other
    /// connection: a call asked after it is answered first (theseus-f3wr).
    #[test]
    fn a_held_rule_answers_late_and_holds_no_other_call() {
        let rules: Vec<Rule> = serde_json::from_value(json!([
            {"when": "slow", "text": "late", "hold_ms": 1500},
            {"when": "quick", "text": "early"}
        ]))
        .unwrap();
        let fake = FakeModel::start_rules_on("127.0.0.1:0", rules).unwrap();
        let ask = |text: &str| {
            let body = json!({"messages": [{"role": "user", "content": text}]}).to_string();
            let mut s = TcpStream::connect(fake.addr).unwrap();
            write!(
                s,
                "POST /v1/messages HTTP/1.1\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            s
        };
        let read = |mut s: TcpStream| {
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let t0 = std::time::Instant::now();
        let slow = ask("slow, please");
        let quick = read(ask("quick, please"));
        assert!(quick.contains("early"), "{quick}");
        assert!(t0.elapsed() < Duration::from_millis(1500), "not held");
        let slow = read(slow);
        assert!(slow.contains("late"), "{slow}");
        assert!(t0.elapsed() >= Duration::from_millis(1500), "held");
    }
}
