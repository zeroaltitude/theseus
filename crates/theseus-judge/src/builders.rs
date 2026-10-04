//! The state builders (design §2.4): each takes a plain input struct this
//! crate defines, and returns a capped state and the dynamic items its pack's
//! questions draw on. The core maps its own types onto these inputs at the
//! wire-in (`theseus-core/src/judge/inputs.rs`, 23a); every string is
//! scrubbed here before it is cut, whatever the core did first.
//!
//! A builder computes what Jev is weak at (counts, shares, the most repeated
//! call) and states it as a field, so no question has to ask for it. A state
//! holds only what the session's own model could see, trimmed: a list keeps
//! its newest items, a long text its start and its end. Each field's cap is a
//! share of the pack's cap, and the shares sum below one, so the total holds
//! before any shrinking. Work is bounded by the caps, not the inputs: a tool
//! call's arguments are serialized only as far as they can be shown.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::pack::{Builder, Dynamic, Item, Pack, Source};
use crate::state::{clip_cut, clip_with, BuiltState, Keep, Scrub, StateBuilder};

/// Each builder's version: any change to what a builder emits is a new
/// version, recorded with every judgment, so a replay knows to rebuild.
pub const PROBE_VERSION: u32 = 1;
pub const LOOP_VERSION: u32 = 1;
pub const SECURITY_VERSION: u32 = 1;
/// `security.v2`'s builder (module `security2`).
pub const SECURITY2_VERSION: u32 = 1;
pub const INBOUND_VERSION: u32 = 1;
pub const CONTINUE_VERSION: u32 = 1;
pub const CATEGORIZE_VERSION: u32 = 1;
/// `rerank.v1`'s builder (module `rerank`, M6 step 32c).
pub const RERANK_VERSION: u32 = 1;

/// The most a builder keeps of each list.
pub const LOOP_CALLS: usize = 8;
pub const SECURITY_CALLS: usize = 5;
pub const SECURITY_ARGV: usize = 64;
pub const SECURITY_PATHS: usize = 32;
pub const LIVE_TASKS: usize = 20;
pub const SIGNALS: usize = 16;
pub const ROLES: usize = 30;
pub const RECENT_MESSAGES: usize = 10;
pub const TOPICS: usize = 50;
pub const MEMBERSHIPS: usize = 5;

/// A state, and its dynamic items, ready to ask.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub state: Arc<BuiltState>,
    pub dynamic: Dynamic,
}

// ------------------------------------------------------------------ inputs

/// The probe's synthetic event (the test pack's state).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeInput {
    pub service: String,
    pub event: String,
    #[serde(default)]
    pub recent_events: Vec<String>,
    pub operator_on_call: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Conversation,
    Task,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallOutcome {
    Ok,
    Error,
    /// Waiting for approval, or declined.
    Held,
    Running,
}

/// One tool call as a state shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallInput {
    pub tool: String,
    /// The call's arguments as the model wrote them.
    #[serde(default)]
    pub args: Value,
    pub outcome: CallOutcome,
    #[serde(default)]
    pub error_class: Option<String>,
}

/// `loop.v1`'s input: a turn the baseline ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopInput {
    pub session_kind: SessionKind,
    /// The exchange's first human message, or the task's brief.
    pub ask: String,
    /// The assistant's final message in this turn.
    pub final_text: String,
    /// This turn's tool calls, oldest first.
    #[serde(default)]
    pub tool_calls: Vec<ToolCallInput>,
    pub loops: u32,
    /// What the turn (or the task, for a task) has spent so far.
    pub spend_usd: f64,
    pub minutes_since_ask: u64,
}

/// The session's hold on external text (T1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldInput {
    pub tool: String,
    #[serde(default)]
    pub host: Option<String>,
    pub minutes_ago: u64,
}

/// `security.v1`'s input: a call the gate has decided.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityInput {
    pub tool: String,
    /// The tool's class (`read`, `write`, `exec`, …).
    pub class: String,
    /// The gate's posture for the call, and why.
    pub posture: String,
    #[serde(default)]
    pub posture_reason: Option<String>,
    #[serde(default)]
    pub argv: Vec<String>,
    #[serde(default)]
    pub paths: Vec<String>,
    /// A network call's URL: the state shows its host, path, and query.
    #[serde(default)]
    pub url: Option<String>,
    /// Any other arguments.
    #[serde(default)]
    pub other_args: Option<Value>,
    #[serde(default)]
    pub operator_last_ask: Option<String>,
    #[serde(default)]
    pub hold: Option<HoldInput>,
    /// The session's recent calls before this one, oldest first. `security.v1`
    /// shows the newest five; `security.v2` also computes facts over all of
    /// them (the core passes a bounded tail).
    #[serde(default)]
    pub last_calls: Vec<ToolCallInput>,
    /// `security.v2` only: what the session read lately (a file, a page), as
    /// an excerpt, oldest first. The session's own model saw it, so Jev may.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_reads: Vec<ReadInput>,
    /// `security.v2` only: files the core knows were written from external
    /// text, in this session or another. Empty when the core keeps no such
    /// provenance, which says nothing about the files.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tainted_paths: Vec<String>,
}

/// Something the session read: its source (a path or a URL) and its start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadInput {
    pub source: String,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub id: String,
    pub brief: String,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleInput {
    pub id: String,
    pub stance: String,
}

/// The `inbound` state's input, shared by `classify.v1` and `role.v1`, so
/// their states are byte-identical and they ride one request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboundInput {
    pub message: String,
    /// `operator`, or another trusted or untrusted person.
    pub author: String,
    /// Where it came in: `discord_dm`, `discord_thread`, `web`, `cli`, …
    pub place_kind: String,
    #[serde(default)]
    pub previous_human_message: Option<String>,
    #[serde(default)]
    pub last_reply: Option<String>,
    #[serde(default)]
    pub minutes_since_last_message: Option<u64>,
    #[serde(default)]
    pub live_tasks: Vec<TaskInput>,
    #[serde(default)]
    pub current_role: Option<String>,
    /// The roles table, for `role.v1`'s options.
    #[serde(default)]
    pub roles: Vec<RoleInput>,
}

/// A candidate signal that fired, and its value in words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalInput {
    pub name: String,
    pub value: String,
}

/// `continue.v1`'s input: a compilation a candidate signal questioned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContinueInput {
    #[serde(default)]
    pub signals: Vec<SignalInput>,
    pub window_tokens: u64,
    pub prefix_tokens: u64,
    pub tail_tokens: u64,
    pub tail_nodes: u64,
    /// The provider's cache read on the last call.
    pub last_cache_read_tokens: u64,
    pub compilation_age_minutes: u64,
    pub strategy: String,
    pub trigger: String,
    pub budget_left_usd: f64,
    #[serde(default)]
    pub last_human_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MembershipInput {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopicInput {
    pub id: String,
    pub description: String,
}

/// `categorize.v1`'s input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CategorizeInput {
    pub session_title: String,
    /// Oldest first; the newest ten are kept.
    #[serde(default)]
    pub recent_human_messages: Vec<String>,
    #[serde(default)]
    pub memberships: Vec<MembershipInput>,
    #[serde(default)]
    pub candidates: Vec<TopicInput>,
}

/// Any builder's input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "builder", content = "input", rename_all = "snake_case")]
pub enum Input {
    Probe(ProbeInput),
    Loop(LoopInput),
    Security(SecurityInput),
    /// `security.v2`'s input is `security.v1`'s, with its optional fields.
    Security2(SecurityInput),
    Inbound(InboundInput),
    Continue(ContinueInput),
    Categorize(CategorizeInput),
    Rerank(RerankInput),
}

impl Input {
    pub fn builder(&self) -> Builder {
        match self {
            Input::Probe(_) => Builder::Probe,
            Input::Loop(_) => Builder::Loop,
            Input::Security(_) => Builder::Security,
            Input::Security2(_) => Builder::Security2,
            Input::Inbound(_) => Builder::Inbound,
            Input::Continue(_) => Builder::Continue,
            Input::Categorize(_) => Builder::Categorize,
            Input::Rerank(_) => Builder::Rerank,
        }
    }

    /// A builder's input from its JSON (a fixture, or the probe's file).
    pub fn parse(builder: Builder, json: &str) -> anyhow::Result<Input> {
        Ok(match builder {
            Builder::Probe => Input::Probe(serde_json::from_str(json)?),
            Builder::Loop => Input::Loop(serde_json::from_str(json)?),
            Builder::Security => Input::Security(serde_json::from_str(json)?),
            Builder::Security2 => Input::Security2(serde_json::from_str(json)?),
            Builder::Inbound => Input::Inbound(serde_json::from_str(json)?),
            Builder::Continue => Input::Continue(serde_json::from_str(json)?),
            Builder::Categorize => Input::Categorize(serde_json::from_str(json)?),
            Builder::Rerank => Input::Rerank(serde_json::from_str(json)?),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("pack {pack} builds its state with {wants:?}, but the input is for {got:?}")]
pub struct WrongInput {
    pub pack: String,
    pub wants: Builder,
    pub got: Builder,
}

/// The pack's state from its builder's input, under the pack's cap.
pub fn prepare(pack: &Pack, input: &Input, scrub: &dyn Scrub) -> Result<Prepared, WrongInput> {
    if input.builder() != pack.builder {
        return Err(WrongInput {
            pack: pack.name(),
            wants: pack.builder,
            got: input.builder(),
        });
    }
    let cap = pack.state_cap_tokens;
    Ok(match input {
        Input::Probe(i) => probe(i, cap, scrub),
        Input::Loop(i) => loop_state(i, cap, scrub),
        Input::Security(i) => security(i, cap, scrub),
        Input::Security2(i) => security2::security2(i, cap, scrub),
        Input::Inbound(i) => inbound(i, cap, scrub),
        Input::Continue(i) => continue_state(i, cap, scrub),
        Input::Categorize(i) => categorize(i, cap, scrub),
        Input::Rerank(i) => rerank::rerank(i, cap, scrub),
    })
}

// ---------------------------------------------------------------- helpers

/// A share of the cap, in tokens.
fn share(cap: u64, percent: u64) -> u64 {
    (cap * percent / 100).max(1)
}

/// The newest `n` of a list (oldest first).
fn newest<T>(v: &[T], n: usize) -> &[T] {
    &v[v.len().saturating_sub(n)..]
}

/// Dollars to four places, as a state shows money.
fn dollars(x: f64) -> f64 {
    if x.is_finite() {
        (x * 10_000.0).round() / 10_000.0
    } else {
        0.0
    }
}

/// A writer that takes `limit` bytes and then refuses, so serializing a
/// huge value stops as soon as enough of it is written.
struct Limited {
    buf: Vec<u8>,
    limit: usize,
}

impl std::io::Write for Limited {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let room = self.limit.saturating_sub(self.buf.len());
        if room == 0 {
            return Err(std::io::Error::other("enough"));
        }
        let n = data.len().min(room);
        self.buf.extend_from_slice(&data[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A value's compact JSON, at most `limit` bytes of it (a string's own
/// text, unquoted).
fn compact(v: &Value, limit: usize) -> String {
    if let Value::String(s) = v {
        let end = s.len().min(limit);
        let mut end = end;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        return s[..end].to_string();
    }
    if v.is_null() {
        return String::new();
    }
    let mut w = Limited {
        buf: Vec::new(),
        limit,
    };
    let _ = serde_json::to_writer(&mut w, v);
    String::from_utf8_lossy(&w.buf).into_owned()
}

/// Clips a field's strings (scrubbed first, marked `…` in place), and
/// remembers whether any was cut, so the field is listed as truncated.
struct Clipper<'a> {
    scrub: &'a dyn Scrub,
    cut: Cell<bool>,
}

impl<'a> Clipper<'a> {
    fn new(scrub: &'a dyn Scrub) -> Self {
        Self {
            scrub,
            cut: Cell::new(false),
        }
    }

    fn clip(&self, text: &str, max_chars: usize) -> String {
        let (s, cut) = clip_cut(self.scrub, text, max_chars);
        if cut {
            self.cut.set(true);
        }
        s
    }

    /// Whether anything was cut since the last call; starts afresh.
    fn cut(&self) -> bool {
        self.cut.replace(false)
    }
}

/// How many items a builder's own list cap left out.
fn left_out<T>(all: &[T], kept: usize) -> usize {
    all.len().saturating_sub(kept)
}

/// A call as a list item: its tool, its arguments (compact JSON, clipped),
/// its outcome, and its error class.
fn call_item(c: &Clipper, call: &ToolCallInput, args_chars: usize) -> Value {
    let args = compact(&call.args, args_chars * 4 + 4096);
    let mut item = json!({
        "tool": c.clip(&call.tool, 60),
        "args": c.clip(&args, args_chars),
        "outcome": call.outcome,
    });
    if let Some(e) = &call.error_class {
        item["error_class"] = json!(c.clip(e, 60));
    }
    item
}

/// How often the commonest identical call (tool and arguments) appears.
fn most_repeated(calls: &[ToolCallInput]) -> usize {
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for c in calls {
        *counts
            .entry((c.tool.clone(), compact(&c.args, 8192)))
            .or_default() += 1;
    }
    counts.values().copied().max().unwrap_or(0)
}

/// A text's first non-blank line, looked for in its first 4 KB.
fn first_line(s: &str) -> &str {
    let mut end = s.len().min(4096);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end]
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

// --------------------------------------------------------------- builders

/// The test pack's state: the service, the event, what happened just before
/// (newest last), and whether someone is on call.
pub fn probe(i: &ProbeInput, cap_tokens: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("probe", PROBE_VERSION, cap_tokens, scrub);
    b.scalar("service", c.clip(&i.service, 80))
        .cut_if("service", c.cut());
    b.text("event", 9, cap_tokens / 2, Keep::Both, &i.event);
    let recent: Vec<Value> = i
        .recent_events
        .iter()
        .map(|e| Value::String(c.clip(e, 300)))
        .collect();
    b.list("recent_events", 5, cap_tokens / 3, recent)
        .cut_if("recent_events", c.cut())
        .scalar("operator_on_call", i.operator_on_call);
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

/// `loop.v1`: the ask, the final text, the last eight calls, and the
/// numbers Jev should not have to count: calls this turn, the most repeated
/// call among the recent ones, loops, spend, and minutes.
pub fn loop_state(i: &LoopInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let recent = newest(&i.tool_calls, LOOP_CALLS);
    let calls: Vec<Value> = recent.iter().map(|t| call_item(&c, t, 240)).collect();
    let mut b = StateBuilder::new("loop", LOOP_VERSION, cap, scrub);
    b.scalar("session_kind", json!(i.session_kind))
        .text("ask", 9, share(cap, 28), Keep::Both, &i.ask)
        .text("final_text", 8, share(cap, 28), Keep::Both, &i.final_text)
        .list_after(
            "last_tool_calls",
            6,
            share(cap, 30),
            calls,
            left_out(&i.tool_calls, recent.len()),
        )
        .cut_if("last_tool_calls", c.cut())
        .scalar("tool_calls_this_turn", i.tool_calls.len())
        .scalar("most_repeated_call", most_repeated(recent))
        .scalar("loops", i.loops)
        .scalar("spend_usd", dollars(i.spend_usd))
        .scalar("minutes_since_ask", i.minutes_since_ask);
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

mod rerank;
mod security2;

pub use rerank::{RerankInput, RerankNote, RERANK_NOTES};

/// `security.v1`: the call (tool, class, posture and why), its arguments
/// (argv, paths, and a URL as host, path, and query, each clipped), the
/// operator's last ask, the session's hold on external text, and its last
/// five calls.
pub fn security(i: &SecurityInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let mut b = StateBuilder::new("security", SECURITY_VERSION, cap, scrub);
    security_fields(&mut b, &c, i, cap, &SecurityShares::V1);
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

/// The share of a security pack's cap each list may take. The shares sum
/// below one; `security.v2` adds fields, so it narrows these.
struct SecurityShares {
    argv: u64,
    paths: u64,
    other_args: u64,
    ask: u64,
    last_calls: u64,
}

impl SecurityShares {
    const V1: Self = Self {
        argv: 20,
        paths: 10,
        other_args: 12,
        ask: 20,
        last_calls: 20,
    };
}

/// The fields `security.v1` and `security.v2` share.
fn security_fields(
    b: &mut StateBuilder,
    c: &Clipper,
    i: &SecurityInput,
    cap: u64,
    sh: &SecurityShares,
) {
    b.scalar("tool", c.clip(&i.tool, 60))
        .cut_if("tool", c.cut())
        .scalar("class", c.clip(&i.class, 30))
        .cut_if("class", c.cut())
        .scalar("posture", c.clip(&i.posture, 30))
        .cut_if("posture", c.cut());
    if let Some(r) = &i.posture_reason {
        b.scalar("posture_reason", c.clip(r, 200))
            .cut_if("posture_reason", c.cut());
    }
    // An argv's first words matter most (the program, its flags), so a cut
    // keeps the head; so for paths.
    if !i.argv.is_empty() {
        let argv: Vec<Value> = i
            .argv
            .iter()
            .take(SECURITY_ARGV)
            .map(|a| Value::String(c.clip(a, 200)))
            .collect();
        let later = left_out(&i.argv, argv.len());
        b.list_head("argv", 9, share(cap, sh.argv), argv, later)
            .cut_if("argv", c.cut());
    }
    if !i.paths.is_empty() {
        let paths: Vec<Value> = i
            .paths
            .iter()
            .take(SECURITY_PATHS)
            .map(|p| Value::String(c.clip(p, 200)))
            .collect();
        let later = left_out(&i.paths, paths.len());
        b.list_head("paths", 8, share(cap, sh.paths), paths, later)
            .cut_if("paths", c.cut());
    }
    if let Some(u) = &i.url {
        // A URL past 8 KB is shown as text: parsing it is not worth it.
        match (u.len() <= 8192)
            .then(|| reqwest::Url::parse(u).ok())
            .flatten()
        {
            Some(url) => {
                b.scalar("url_host", c.clip(url.host_str().unwrap_or(""), 120))
                    .cut_if("url_host", c.cut())
                    .scalar("url_path", c.clip(url.path(), 200))
                    .cut_if("url_path", c.cut())
                    .scalar("url_query", c.clip(url.query().unwrap_or(""), 300))
                    .cut_if("url_query", c.cut());
            }
            None => {
                b.scalar("url", c.clip(u, 300)).cut_if("url", c.cut());
            }
        }
    }
    if let Some(o) = &i.other_args {
        let cap_bytes = share(cap, sh.other_args) as usize * 4;
        b.text(
            "other_args",
            7,
            share(cap, sh.other_args),
            Keep::Head,
            &compact(o, cap_bytes + 8192),
        );
    }
    b.opt_text(
        "operator_last_ask",
        9,
        share(cap, sh.ask),
        Keep::Both,
        i.operator_last_ask.as_deref(),
    )
    .scalar("session_holds_external_text", i.hold.is_some());
    if let Some(h) = &i.hold {
        b.scalar(
            "hold",
            json!({
                "tool": c.clip(&h.tool, 60),
                "host": h.host.as_deref().map(|x| c.clip(x, 120)),
                "minutes_ago": h.minutes_ago,
            }),
        )
        .cut_if("hold", c.cut());
    }
    let recent = newest(&i.last_calls, SECURITY_CALLS);
    let last: Vec<Value> = recent.iter().map(|t| call_item(c, t, 200)).collect();
    b.list_after(
        "last_calls",
        5,
        share(cap, sh.last_calls),
        last,
        left_out(&i.last_calls, recent.len()),
    )
    .cut_if("last_calls", c.cut());
}

/// The `inbound` state for `classify.v1` and `role.v1`: the message, who
/// sent it and where, the previous human message and the last reply
/// (trimmed), the live tasks, the minutes since the last message, and the
/// current role. Its dynamic items: the live tasks (for `addressed_task`)
/// and the roles table (for `role`).
pub fn inbound(i: &InboundInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let clip = |s: &str, n: usize| clip_with(scrub, s, n);
    // The core lists live tasks most relevant first; the state and the
    // `addressed_task` options show the same first twenty.
    let tasks = &i.live_tasks[..i.live_tasks.len().min(LIVE_TASKS)];
    let mut b = StateBuilder::new("inbound", INBOUND_VERSION, cap, scrub);
    b.text("message", 10, share(cap, 30), Keep::Both, &i.message)
        .scalar("author", c.clip(&i.author, 40))
        .cut_if("author", c.cut())
        .scalar("place_kind", c.clip(&i.place_kind, 40))
        .cut_if("place_kind", c.cut())
        .opt_text(
            "previous_human_message",
            7,
            share(cap, 15),
            Keep::Both,
            i.previous_human_message.as_deref(),
        )
        .opt_text(
            "last_reply",
            6,
            share(cap, 15),
            Keep::Both,
            i.last_reply.as_deref(),
        );
    if let Some(m) = i.minutes_since_last_message {
        b.scalar("minutes_since_last_message", m);
    }
    let task_items: Vec<Value> = tasks
        .iter()
        .map(|t| {
            json!({
                "id": c.clip(&t.id, 40),
                "brief": c.clip(first_line(&t.brief), 160),
                "state": c.clip(&t.state, 30),
            })
        })
        .collect();
    b.list_head(
        "live_tasks",
        8,
        share(cap, 20),
        task_items,
        left_out(&i.live_tasks, tasks.len()),
    )
    .cut_if("live_tasks", c.cut());
    let role = i
        .current_role
        .as_deref()
        .map_or_else(|| "none".to_string(), |r| c.clip(r, 40));
    b.scalar("current_role", role)
        .cut_if("current_role", c.cut());
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Tasks,
        tasks
            .iter()
            .map(|t| Item {
                key: t.id.clone(),
                text: format!(
                    "The live task {}: {} (it is {}).",
                    clip(&t.id, 40),
                    clip(first_line(&t.brief), 160),
                    clip(&t.state, 30)
                ),
            })
            .collect(),
    );
    dynamic.sources.insert(
        Source::Roles,
        i.roles
            .iter()
            .take(ROLES)
            .map(|r| Item {
                key: r.id.clone(),
                text: clip(&r.stance, 300),
            })
            .collect(),
    );
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}

/// `continue.v1`: the signals that fired, the compilation (strategy,
/// trigger, age), the sizes and the shares of the window they fill, the
/// last call's cache read against the prefix (all worked out here), the
/// budget left, and the last human message.
pub fn continue_state(i: &ContinueInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let percent = |part: u64, whole: u64| part.saturating_mul(100).checked_div(whole).unwrap_or(0);
    let context = i.prefix_tokens.saturating_add(i.tail_tokens);
    let mut b = StateBuilder::new("continue", CONTINUE_VERSION, cap, scrub);
    let recent = newest(&i.signals, SIGNALS);
    let signals: Vec<Value> = recent
        .iter()
        .map(|s| json!({"name": c.clip(&s.name, 60), "value": c.clip(&s.value, 200)}))
        .collect();
    b.list_after(
        "signals",
        10,
        share(cap, 25),
        signals,
        left_out(&i.signals, recent.len()),
    )
    .cut_if("signals", c.cut())
    .scalar("strategy", c.clip(&i.strategy, 40))
    .cut_if("strategy", c.cut())
    .scalar("trigger", c.clip(&i.trigger, 60))
    .cut_if("trigger", c.cut())
    .scalar("compilation_age_minutes", i.compilation_age_minutes)
    .scalar("window_tokens", i.window_tokens)
    .scalar("prefix_tokens", i.prefix_tokens)
    .scalar("tail_tokens", i.tail_tokens)
    .scalar("tail_nodes", i.tail_nodes)
    .scalar(
        "tail_percent_of_window",
        percent(i.tail_tokens, i.window_tokens),
    )
    .scalar(
        "context_percent_of_window",
        percent(context, i.window_tokens),
    )
    .scalar("last_call_read_the_cache", i.last_cache_read_tokens > 0)
    .scalar(
        "cache_read_percent_of_prefix",
        percent(i.last_cache_read_tokens, i.prefix_tokens),
    )
    .scalar("budget_left_usd", dollars(i.budget_left_usd))
    .opt_text(
        "last_human_message",
        8,
        share(cap, 40),
        Keep::Both,
        i.last_human_message.as_deref(),
    );
    Prepared {
        state: Arc::new(b.build()),
        dynamic: Dynamic::default(),
    }
}

/// `categorize.v1`: the session's title and its last ten human messages.
/// Its dynamic items: the candidate topics (up to 50, each with its
/// description, as the `topic` options) and the current memberships (one
/// `still_member` Noul each, naming its topic by its description, at most
/// five).
///
/// The state leaves the current memberships out, unlike design §2.4: with
/// the session's membership listed, a live call put a session's messages in
/// that topic (0.76) although they were about another, which it chose at
/// 0.74 once the membership was gone (lane report, L2). So `topic` reads the
/// messages alone, and `still_member` learns its topic from its own words.
pub fn categorize(i: &CategorizeInput, cap: u64, scrub: &dyn Scrub) -> Prepared {
    let c = Clipper::new(scrub);
    let clip = |s: &str, n: usize| clip_with(scrub, s, n);
    let mut b = StateBuilder::new("categorize", CATEGORIZE_VERSION, cap, scrub);
    b.scalar("session_title", c.clip(&i.session_title, 120))
        .cut_if("session_title", c.cut());
    let recent = newest(&i.recent_human_messages, RECENT_MESSAGES);
    let messages: Vec<Value> = recent
        .iter()
        .map(|m| Value::String(c.clip(m, 400)))
        .collect();
    b.list_after(
        "recent_human_messages",
        10,
        share(cap, 80),
        messages,
        left_out(&i.recent_human_messages, recent.len()),
    )
    .cut_if("recent_human_messages", c.cut());
    // The core lists memberships most relevant first.
    let memberships = &i.memberships[..i.memberships.len().min(MEMBERSHIPS)];
    let mut dynamic = Dynamic::default();
    dynamic.sources.insert(
        Source::Topics,
        i.candidates
            .iter()
            .take(TOPICS)
            .map(|t| Item {
                key: t.id.clone(),
                text: clip(&t.description, 300),
            })
            .collect(),
    );
    // A membership's Noul names its topic as the `topic` options do: by the
    // candidate's description when the topic is among them, else its title.
    dynamic.sources.insert(
        Source::Memberships,
        memberships
            .iter()
            .map(|m| {
                let named = i.candidates.iter().find(|t| t.id == m.id).map_or_else(
                    || clip(&m.title, 120),
                    |t| clip(t.description.trim().trim_end_matches('.'), 300),
                );
                Item {
                    key: m.id.clone(),
                    text: format!("“{named}”"),
                }
            })
            .collect(),
    );
    Prepared {
        state: Arc::new(b.build()),
        dynamic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::Question;
    use crate::pack::{by_name, embedded};
    use crate::state::NoScrub;
    use crate::tests::golden;

    fn fixture(builder: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/fixtures/inputs/{builder}.json",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    fn builder_name(b: Builder) -> String {
        format!("{b:?}").to_lowercase()
    }

    fn prepared(pack: &str) -> (Arc<Pack>, Prepared) {
        let p = by_name(pack).unwrap();
        let input = Input::parse(p.builder, &fixture(&builder_name(p.builder))).unwrap();
        let prepared = prepare(&p, &input, &NoScrub).unwrap();
        (p, prepared)
    }

    #[test]
    fn each_builder_matches_its_golden_state() {
        for pack in [
            "probe.v1",
            "loop.v1",
            "security.v1",
            "security.v2",
            "security.v3",
            "classify.v1",
            "continue.v1",
            "categorize.v1",
            "rerank.v1",
        ] {
            let (p, prepared) = prepared(pack);
            golden(
                &format!("fixtures/golden/{}.state.json", builder_name(p.builder)),
                &prepared.state.json,
            );
            assert!(prepared.state.tokens <= p.state_cap_tokens);
            assert!(
                prepared.state.truncated.is_empty(),
                "{pack}: {:?}",
                prepared.state.truncated
            );
        }
    }

    #[test]
    fn each_pack_asks_its_golden_request() {
        for p in embedded().as_ref().unwrap() {
            let (_, prepared) = prepared(&p.name());
            let questions = p
                .ask(&prepared.dynamic)
                .into_iter()
                .map(|a| (crate::batch::namespaced(&p.name(), &a.id), a.question))
                .collect();
            let req = crate::client::Request {
                state: prepared.state.json.clone(),
                model: p.jev_model.clone(),
                questions,
            };
            golden(
                &format!("fixtures/golden/{}.request.json", p.name()),
                &req.body(),
            );
        }
    }

    #[test]
    fn classify_and_role_build_byte_identical_states() {
        let (_, c) = prepared("classify.v1");
        let (_, r) = prepared("role.v1");
        assert_eq!(c.state.json, r.state.json);
        assert_eq!(c.state.sha256, r.state.sha256);
    }

    #[test]
    fn the_loop_state_counts_for_jev() {
        let (_, p) = prepared("loop.v1");
        let v = p.state.value();
        assert_eq!(v["tool_calls_this_turn"], 5);
        // The two identical fs.read calls.
        assert_eq!(v["most_repeated_call"], 2);
        assert_eq!(v["session_kind"], "task");
        assert_eq!(v["spend_usd"], 0.0412);
        assert_eq!(v["last_tool_calls"].as_array().unwrap().len(), 5);
        assert_eq!(v["last_tool_calls"][1]["error_class"], "no_match");
    }

    #[test]
    fn the_security_state_splits_a_url_and_shows_the_hold() {
        let (_, p) = prepared("security.v1");
        let v = p.state.value();
        assert_eq!(v["url_host"], "collect.example.net");
        assert_eq!(v["url_path"], "/v1/ingest");
        assert!(v["url_query"]
            .as_str()
            .unwrap()
            .starts_with("source=setup&data="));
        assert_eq!(v["session_holds_external_text"], true);
        assert_eq!(v["hold"]["host"], "docs.example.org");
        assert!(v.get("argv").is_none(), "an empty argv is left out");
    }

    #[test]
    fn dynamic_questions_expand_from_the_inputs() {
        let (p, classify) = prepared("classify.v1");
        let asked = p.ask(&classify.dynamic);
        let task = asked.iter().find(|a| a.id == "addressed_task").unwrap();
        let Question::Choice { options, .. } = &task.question else {
            panic!()
        };
        let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, vec!["a1b2c3", "d4e5f6", "none"]);
        let first = options[0].means.as_deref().unwrap();
        assert!(first.contains("Migrate the ledger printer"));
        assert!(
            !first.contains("column order"),
            "the brief's first line only"
        );
        // With no live tasks, the question is not asked at all.
        let mut input: InboundInput = serde_json::from_str(&fixture("inbound")).unwrap();
        input.live_tasks.clear();
        let none = prepare(&p, &Input::Inbound(input.clone()), &NoScrub).unwrap();
        assert!(p
            .ask(&none.dynamic)
            .iter()
            .all(|a| a.id != "addressed_task"));
        // role.v1 without a roles table asks nothing.
        input.roles.clear();
        let role = by_name("role.v1").unwrap();
        let bare = prepare(&role, &Input::Inbound(input), &NoScrub).unwrap();
        assert!(role.ask(&bare.dynamic).is_empty());
        // categorize.v1: one still_member Noul per membership, naming its
        // topic by the candidate's description (its title when the topic is
        // not a candidate); the state leaves the memberships out.
        let (cp, cprep) = prepared("categorize.v1");
        let asked = cp.ask(&cprep.dynamic);
        let member: Vec<&crate::pack::Asked> =
            asked.iter().filter(|a| a.def == "still_member").collect();
        assert_eq!(member.len(), 1);
        assert_eq!(member[0].id, "still_member.1");
        assert_eq!(member[0].about.as_deref(), Some("openclaw"));
        assert_eq!(
            member[0].question.instructions(),
            "Are the recent human messages in this session mostly about the topic \
             “OpenClaw, the agent gateway: plugins, channels, sessions, and provenance”?"
        );
        assert!(cprep.state.value().get("current_memberships").is_none());
        let mut input: CategorizeInput = serde_json::from_str(&fixture("categorize")).unwrap();
        input.candidates.retain(|t| t.id != "openclaw");
        let other = prepare(&cp, &Input::Categorize(input), &NoScrub).unwrap();
        assert_eq!(
            other.dynamic.items(Source::Memberships)[0].text,
            "“OpenClaw, the agent gateway”"
        );
        let topic = asked.iter().find(|a| a.id == "topic").unwrap();
        let Question::Choice { options, .. } = &topic.question else {
            panic!()
        };
        assert_eq!(options.len(), 6, "four candidates, new_topic, none");
    }

    #[test]
    fn a_pack_refuses_another_builders_input() {
        let p = by_name("loop.v1").unwrap();
        let input = Input::parse(Builder::Probe, &fixture("probe")).unwrap();
        assert_eq!(
            prepare(&p, &input, &NoScrub).unwrap_err(),
            WrongInput {
                pack: "loop.v1".into(),
                wants: Builder::Loop,
                got: Builder::Probe
            }
        );
        assert!(
            Input::parse(Builder::Loop, r#"{"ask": "x"}"#).is_err(),
            "fields are required"
        );
        let extra = r#"{"service":"s","event":"e","operator_on_call":true,"extra":1}"#;
        assert!(Input::parse(Builder::Probe, extra).is_err());
    }

    #[test]
    fn huge_arguments_are_serialized_only_as_far_as_they_show() {
        let v = json!({"blob": "x".repeat(1_000_000)});
        assert_eq!(compact(&v, 100).len(), 100);
        assert_eq!(compact(&json!("é".repeat(10)), 5), "éé");
        assert_eq!(compact(&Value::Null, 10), "");
        assert_eq!(compact(&json!({"a": 1}), 100), r#"{"a":1}"#);
    }

    #[test]
    fn a_builders_own_cuts_are_marked_in_place_and_listed() {
        // Twelve messages: the newest ten are kept, and the list says so;
        // a long title is clipped, marked, and listed.
        let mut input: CategorizeInput = serde_json::from_str(&fixture("categorize")).unwrap();
        input.recent_human_messages.insert(0, "an older one".into());
        input.recent_human_messages.insert(0, "the oldest".into());
        input.session_title = "t".repeat(300);
        let p = by_name("categorize.v1").unwrap();
        let s = prepare(&p, &Input::Categorize(input), &NoScrub)
            .unwrap()
            .state;
        let v = s.value();
        let messages = v["recent_human_messages"].as_array().unwrap();
        assert_eq!(messages[0], json!({"cut": "2 earlier left out"}));
        assert_eq!(messages.len(), 11);
        assert_eq!(
            messages[1],
            "How does the breaker decide when to probe again?"
        );
        assert!(v["session_title"].as_str().unwrap().ends_with('…'));
        assert_eq!(s.truncated, vec!["session_title", "recent_human_messages"]);
        // An argv keeps its head: the program survives, and the end says
        // how many arguments went.
        let mut sec: SecurityInput = serde_json::from_str(&fixture("security")).unwrap();
        sec.argv = (0..70).map(|i| format!("arg{i}")).collect();
        sec.argv[0] = "rm".into();
        let p = by_name("security.v1").unwrap();
        let s = prepare(&p, &Input::Security(sec), &NoScrub).unwrap().state;
        let argv = s.value()["argv"].as_array().unwrap().clone();
        assert_eq!(argv[0], "rm");
        assert_eq!(argv.len(), SECURITY_ARGV + 1);
        assert_eq!(argv.last().unwrap(), &json!({"cut": "6 later left out"}));
        assert_eq!(s.truncated, vec!["argv"]);
        // Nothing cut, nothing listed.
        let (_, lp) = prepared("loop.v1");
        assert!(lp.state.truncated.is_empty());
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn every_builder_stays_under_its_cap_on_huge_inputs() {
        let big = "a very long line of text that goes on ".repeat(30_000); // 1.1 MB
        let item = "an item of two kilobytes or so ".repeat(64);
        let call = |i: usize| ToolCallInput {
            tool: format!("tool{}", i % 3),
            args: json!({"argv": [item.clone()]}),
            outcome: CallOutcome::Error,
            error_class: Some(item.clone()),
        };
        let mut calls: Vec<ToolCallInput> = (0..2_000).map(call).collect();
        calls.push(ToolCallInput {
            tool: "fs.write".into(),
            args: json!({"path": "big.txt", "content": big}),
            outcome: CallOutcome::Ok,
            error_class: None,
        });
        let sec = SecurityInput {
            tool: big.clone(),
            class: big.clone(),
            posture: big.clone(),
            posture_reason: Some(big.clone()),
            argv: (0..2_000).map(|_| item.clone()).collect(),
            paths: (0..2_000).map(|_| item.clone()).collect(),
            url: Some(format!(
                "https://example.com/{}?q={}",
                "p".repeat(4_000),
                "q".repeat(4_000)
            )),
            other_args: Some(json!({ "blob": big })),
            operator_last_ask: Some(big.clone()),
            hold: Some(HoldInput {
                tool: big.clone(),
                host: Some(big.clone()),
                minutes_ago: 1,
            }),
            last_calls: calls.clone(),
            recent_reads: (0..50)
                .map(|_| ReadInput {
                    source: item.clone(),
                    excerpt: big.clone(),
                })
                .collect(),
            tainted_paths: (0..500).map(|_| item.clone()).collect(),
        };
        let inputs: Vec<(&str, Input)> = vec![
            (
                "loop.v1",
                Input::Loop(LoopInput {
                    session_kind: SessionKind::Conversation,
                    ask: big.clone(),
                    final_text: big.clone(),
                    tool_calls: calls.clone(),
                    loops: 9_999,
                    spend_usd: 99.123456,
                    minutes_since_ask: 99_999,
                }),
            ),
            ("security.v1", Input::Security(sec.clone())),
            ("security.v2", Input::Security2(sec.clone())),
            ("security.v3", Input::Security2(sec)),
            (
                "classify.v1",
                Input::Inbound(InboundInput {
                    message: big.clone(),
                    author: big.clone(),
                    place_kind: big.clone(),
                    previous_human_message: Some(big.clone()),
                    last_reply: Some(big.clone()),
                    minutes_since_last_message: Some(1),
                    live_tasks: (0..2_000)
                        .map(|i| TaskInput {
                            id: format!("t{i}"),
                            brief: item.clone(),
                            state: item.clone(),
                        })
                        .collect(),
                    current_role: Some(big.clone()),
                    roles: (0..2_000)
                        .map(|i| RoleInput {
                            id: format!("r{i}"),
                            stance: item.clone(),
                        })
                        .collect(),
                }),
            ),
            (
                "continue.v1",
                Input::Continue(ContinueInput {
                    signals: (0..2_000)
                        .map(|_| SignalInput {
                            name: item.clone(),
                            value: item.clone(),
                        })
                        .collect(),
                    window_tokens: 200_000,
                    prefix_tokens: u64::MAX,
                    tail_tokens: u64::MAX,
                    tail_nodes: 1,
                    last_cache_read_tokens: 0,
                    compilation_age_minutes: 1,
                    strategy: big.clone(),
                    trigger: big.clone(),
                    budget_left_usd: f64::NAN,
                    last_human_message: Some(big.clone()),
                }),
            ),
            (
                "categorize.v1",
                Input::Categorize(CategorizeInput {
                    session_title: big,
                    recent_human_messages: (0..2_000).map(|_| item.clone()).collect(),
                    memberships: (0..2_000)
                        .map(|i| MembershipInput {
                            id: format!("m{i}"),
                            title: item.clone(),
                        })
                        .collect(),
                    candidates: (0..2_000)
                        .map(|i| TopicInput {
                            id: format!("c{i}"),
                            description: item.clone(),
                        })
                        .collect(),
                }),
            ),
        ];
        for (pack, input) in inputs {
            let p = by_name(pack).unwrap();
            let started = std::time::Instant::now();
            let prepared = prepare(&p, &input, &NoScrub).unwrap();
            let took = started.elapsed();
            let s = &prepared.state;
            assert!(
                s.tokens <= p.state_cap_tokens,
                "{pack}: {} tokens over {}",
                s.tokens,
                p.state_cap_tokens
            );
            assert!(!s.truncated.is_empty(), "{pack}: truncation is marked");
            assert!(s.value().is_object());
            // Dynamic items stay within their caps, and so do the options.
            for items in prepared.dynamic.sources.values() {
                assert!(items.len() <= TOPICS, "{pack}");
                assert!(
                    items.iter().all(|i| i.text.chars().count() <= 400),
                    "{pack}"
                );
            }
            for a in p.ask(&prepared.dynamic) {
                if let Question::Choice { options, .. } = &a.question {
                    assert!(options.len() <= crate::client::MAX_CHOICE_OPTIONS);
                }
            }
            // Bounded by the caps, not the input (generous, for a debug
            // build; the release bench of under 1 ms is 23a's).
            assert!(
                took < std::time::Duration::from_millis(500),
                "{pack} took {took:?}"
            );
        }
    }
}
