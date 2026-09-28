//! A session's event stream as Discord messages. Pure: notifications in,
//! operations out, so the whole rendering is tested without Discord.
//!
//! A turn becomes: each loop's streamed text as one or more messages edited in
//! place (split under Discord's 2000-character limit, with code fences closed
//! and reopened across a split); one message per loop listing its tool calls,
//! updated as each runs; a confirm message with Approve and Decline buttons;
//! and a footer line with the turn's model, loops, tools, dollars, and time.
//! Rendering is a diff: every tick re-renders the live turns and emits only the
//! messages whose text changed, so a burst of deltas costs one edit.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde_json::Value;
use theseus_protocol::{ConfirmRequest, ConsequenceTag, TurnSubmitResult};

/// Discord's limit is 2000 characters; parts stay under it with room for a fence repair.
pub const PART_LIMIT: usize = 1900;
const DISCORD_LIMIT: usize = 2000;
/// Turns kept for late updates (a confirm answered after the turn parked).
const RECENT_TURNS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Buttons {
    /// Leave the message's components as they are.
    Keep,
    /// Approve and Decline for this correlation id.
    Confirm(String),
    /// Remove every component.
    Clear,
}

/// A structured notice (a Discord embed): a call ran that the policy alone
/// would have stopped, because the enforcement level is `notify` or `open`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoticeCard {
    pub title: String,
    /// 0xRRGGBB: amber for a skipped approval, red for an off-policy run.
    pub color: u32,
    pub description: String,
    pub fields: Vec<(String, String)>,
}

pub const AMBER: u32 = 0xE3A008;
pub const RED: u32 = 0xD93025;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// "Theseus is typing…" (Discord shows it for about ten seconds).
    Typing,
    /// Create or edit the notice message for `key`.
    Notice { key: String, card: NoticeCard },
    /// Create the message for `key`, or edit it when it already exists.
    Upsert {
        key: String,
        content: String,
        buttons: Buttons,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ToolState {
    Proposed,
    Running,
    Waiting,
    Background,
    Done { status: String, ms: u64 },
    Denied(String),
    Answered { approved: bool, by: String },
}

#[derive(Debug, Clone)]
struct ToolLine {
    tool_use_id: String,
    tool: String,
    summary: String,
    correlation_id: Option<String>,
    state: ToolState,
    /// `approval_skipped` or `off_policy` when the enforcement level let it
    /// run, `irreversible` when it waits for an irreversible consequence.
    notice: Option<String>,
}

#[derive(Debug, Default)]
struct LoopView {
    text: String,
    tools: Vec<ToolLine>,
}

#[derive(Debug)]
struct TurnView {
    turn_id: String,
    loops: BTreeMap<u32, LoopView>,
    footer: Option<String>,
    failure: Option<String>,
    ended: bool,
    dirty: bool,
}

#[derive(Default)]
pub struct Renderer {
    turns: VecDeque<TurnView>,
    /// key → the content Discord last received for it.
    emitted: HashMap<String, String>,
    /// correlation id → (message key, the tool line it describes).
    confirms: HashMap<String, (String, String)>,
    /// tool_use_id → the notice card posted for it (updated when the call ends).
    notices: HashMap<String, NoticeCard>,
}

impl Renderer {
    /// True while a turn is running (the place keeps "typing…" alive).
    pub fn busy(&self) -> bool {
        self.turns.back().is_some_and(|t| !t.ended)
    }

    /// Feed one notification for this session; returns what to do right away.
    /// Streamed text and tool lines wait for [`Renderer::tick`].
    pub fn on_notification(&mut self, method: &str, p: &Value) -> Vec<Op> {
        let turn_id = p.get("turn_id").and_then(Value::as_str).unwrap_or("");
        match method {
            "turn.started" => {
                self.turns.push_back(TurnView {
                    turn_id: turn_id.to_string(),
                    loops: BTreeMap::new(),
                    footer: None,
                    failure: None,
                    ended: false,
                    dirty: false,
                });
                while self.turns.len() > RECENT_TURNS {
                    self.turns.pop_front();
                }
                vec![Op::Typing]
            }
            "model.delta" => {
                let li = p.get("loop_index").and_then(Value::as_u64).unwrap_or(0) as u32;
                let text = p.get("text").and_then(Value::as_str).unwrap_or("");
                if let Some(t) = self.turn_mut(turn_id) {
                    t.loops.entry(li).or_default().text.push_str(text);
                    t.dirty = true;
                }
                vec![]
            }
            "tool.proposed" => {
                let tool = p.get("tool").and_then(Value::as_str).unwrap_or("?");
                let input = p.get("input").cloned().unwrap_or(Value::Null);
                let state = match denial_reason(p.get("gate")) {
                    Some(r) => ToolState::Denied(r),
                    None => ToolState::Proposed,
                };
                let notice = p
                    .pointer("/gate/decision/notify/kind")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let line = ToolLine {
                    tool_use_id: str_of(p, "tool_use_id"),
                    tool: tool.to_string(),
                    summary: summarize(tool, &input),
                    correlation_id: None,
                    state,
                    notice,
                };
                if let Some(t) = self.turn_mut(turn_id) {
                    let li = t.loops.keys().next_back().copied().unwrap_or(0);
                    t.loops.entry(li).or_default().tools.push(line);
                    t.dirty = true;
                }
                vec![]
            }
            "tool.started" => {
                let corr = p
                    .get("correlation_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                self.update_tool(turn_id, &str_of(p, "tool_use_id"), |l| {
                    l.state = ToolState::Running;
                    if corr.is_some() {
                        l.correlation_id = corr.clone();
                    }
                });
                vec![]
            }
            "tool.ended" => {
                let status = str_of(p, "status");
                let ms = p.get("duration_ms").and_then(Value::as_u64).unwrap_or(0);
                let use_id = str_of(p, "tool_use_id");
                let mut ops = vec![];
                if let Some(card) = self.notices.get_mut(&use_id) {
                    let outcome = match status.as_str() {
                        "ok" => format!("✅ ok · {ms} ms"),
                        "background" => "⏳ running in the background".to_string(),
                        other => format!("❌ {other} · {ms} ms"),
                    };
                    if let Some(f) = card.fields.iter_mut().find(|(n, _)| n == "Outcome") {
                        f.1 = outcome;
                    }
                    ops.push(Op::Notice {
                        key: format!("notice:{use_id}"),
                        card: card.clone(),
                    });
                }
                self.update_tool(turn_id, &use_id, |l| {
                    l.state = match status.as_str() {
                        "background" => ToolState::Background,
                        "denied" => match &l.state {
                            ToolState::Denied(r) => ToolState::Denied(r.clone()),
                            _ => ToolState::Denied(String::new()),
                        },
                        _ => ToolState::Done {
                            status: status.clone(),
                            ms,
                        },
                    };
                });
                ops
            }
            "policy.notified" => {
                let kind = str_of(p, "kind");
                let tool = p.get("tool").and_then(Value::as_str).unwrap_or("?");
                let input = p.get("input").cloned().unwrap_or(Value::Null);
                let use_id = str_of(p, "tool_use_id");
                let (title, color, would) = if kind == "off_policy" {
                    ("🚨 Ran against policy", RED, "been refused")
                } else {
                    ("⚠️ Ran without approval", AMBER, "asked for your approval")
                };
                let mut fields = vec![
                    ("What".into(), clip(&str_of(p, "summary"), 1000)),
                    (
                        format!("The policy would have {would}"),
                        clip(&str_of(p, "rule"), 1000),
                    ),
                ];
                let named = consequences(p);
                if !named.is_empty() {
                    fields.push(("Consequences".into(), named));
                }
                fields.push(("Setting".into(), format!("`{}`", str_of(p, "setting"))));
                fields.push(("Outcome".into(), "⏳ running".into()));
                let card = NoticeCard {
                    title: title.into(),
                    color,
                    description: format!("`{tool}` {}", summarize(tool, &input)),
                    fields,
                };
                self.notices.insert(use_id.clone(), card.clone());
                vec![Op::Notice {
                    key: format!("notice:{use_id}"),
                    card,
                }]
            }
            "confirm.requested" => {
                let Ok(req) = serde_json::from_value::<ConfirmRequest>(p.clone()) else {
                    return vec![];
                };
                // The newest proposed call of this tool is the one waiting.
                let corr = req.correlation_id.clone();
                if let Some(t) = self.turns.iter_mut().rev().find(|t| !t.ended) {
                    if let Some(l) = t
                        .loops
                        .values_mut()
                        .rev()
                        .flat_map(|lv| lv.tools.iter_mut().rev())
                        .find(|l| l.tool == req.tool && l.state == ToolState::Proposed)
                    {
                        l.state = ToolState::Waiting;
                        l.correlation_id = Some(corr.clone());
                        if req.irreversible {
                            l.notice = Some("irreversible".into());
                        }
                    }
                    t.dirty = true;
                }
                let line = format!("`{}` {}", req.tool, summarize(&req.tool, &req.input));
                let key = format!("confirm:{corr}");
                let mut content = match (req.against_policy, req.irreversible) {
                    (true, true) => {
                        format!("🚨 **Against policy and irreversible. Approve anyway?** {line}")
                    }
                    (true, false) => format!("🚨 **Against policy. Approve anyway?** {line}"),
                    (false, true) => format!("⛔ **Irreversible. Approve?** {line}"),
                    (false, false) => format!("**Approve?** {line}"),
                };
                let named = ConsequenceTag::summary(&req.consequences);
                if !named.is_empty() {
                    content.push_str(&format!("\n**{named}**"));
                }
                if !req.reason.is_empty() {
                    content.push_str(&format!("\n{}", clip(&req.reason, 300)));
                }
                content.push_str(&format!(
                    "\n-# expires <t:{}:R> · you can also answer in the web UI or with `theseus confirm`",
                    req.expires_at_ms / 1000
                ));
                let line = match (req.against_policy, named.is_empty()) {
                    (true, true) => format!("{line} (against policy)"),
                    (true, false) => format!("{line} (against policy; {named})"),
                    (false, false) => format!("{line} ({named})"),
                    (false, true) => line,
                };
                self.confirms.insert(corr.clone(), (key.clone(), line));
                vec![self.upsert(&key, content, Buttons::Confirm(corr))]
            }
            "confirm.resolved" => {
                let corr = str_of(p, "correlation_id");
                let approved = p.get("approved").and_then(Value::as_bool).unwrap_or(false);
                let superseded = p
                    .get("superseded")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let by = p
                    .get("by")
                    .and_then(Value::as_str)
                    .unwrap_or("the operator")
                    .to_string();
                for t in self.turns.iter_mut() {
                    for lv in t.loops.values_mut() {
                        for l in lv.tools.iter_mut() {
                            if l.correlation_id.as_deref() == Some(corr.as_str())
                                && l.state == ToolState::Waiting
                            {
                                l.state = ToolState::Answered {
                                    approved,
                                    by: by.clone(),
                                };
                                t.dirty = true;
                            }
                        }
                    }
                }
                let Some((key, line)) = self.confirms.remove(&corr) else {
                    return vec![];
                };
                let content = if superseded {
                    format!("⏭️ **Not run**: a new message replaced this request. {line}")
                } else if approved {
                    format!("✅ **Approved** by {by} · {line}")
                } else {
                    format!("❎ **Declined** by {by} · {line}")
                };
                vec![self.upsert(&key, content, Buttons::Clear)]
            }
            "loop.ended" => {
                if let Some(t) = self.turn_mut(turn_id) {
                    t.dirty = true;
                }
                self.tick()
            }
            "turn.ended" => {
                let Ok(r) = serde_json::from_value::<TurnSubmitResult>(p.clone()) else {
                    return vec![];
                };
                match self.turn_mut(&r.turn_id) {
                    Some(t) => {
                        t.footer = Some(footer(&r));
                        t.ended = true;
                        t.dirty = true;
                    }
                    None => {
                        // It started before this place was watching (a daemon
                        // restart racing the binding): show its final text.
                        let mut loops = BTreeMap::new();
                        loops.insert(
                            0,
                            LoopView {
                                text: r.output.clone(),
                                tools: vec![],
                            },
                        );
                        self.turns.push_back(TurnView {
                            turn_id: r.turn_id.clone(),
                            loops,
                            footer: Some(footer(&r)),
                            failure: None,
                            ended: true,
                            dirty: true,
                        });
                        while self.turns.len() > RECENT_TURNS {
                            self.turns.pop_front();
                        }
                    }
                }
                self.tick()
            }
            "turn.failed" => {
                let class = p.get("class").and_then(Value::as_str).unwrap_or("error");
                let error = clip(p.get("error").and_then(Value::as_str).unwrap_or(""), 600);
                let text = format!("⚠️ **Turn failed** ({class}): {error}");
                match self
                    .turns
                    .iter_mut()
                    .rev()
                    .find(|t| t.turn_id == turn_id || (turn_id.is_empty() && !t.ended))
                {
                    Some(t) => {
                        t.failure = Some(text);
                        t.ended = true;
                        t.dirty = true;
                        self.tick()
                    }
                    None => {
                        let key = format!("failed:{}", theseus_protocol::now_unix_ms());
                        vec![self.upsert(&key, text, Buttons::Keep)]
                    }
                }
            }
            _ => vec![],
        }
    }

    /// Emit the messages whose rendered text changed since Discord last saw them.
    pub fn tick(&mut self) -> Vec<Op> {
        let mut rendered = Vec::new();
        for t in self.turns.iter_mut().filter(|t| t.dirty) {
            rendered.extend(render_turn(t));
            t.dirty = false;
        }
        rendered
            .into_iter()
            .filter_map(|(key, content)| {
                (self.emitted.get(&key) != Some(&content))
                    .then(|| self.upsert(&key, content, Buttons::Keep))
            })
            .collect()
    }

    fn upsert(&mut self, key: &str, content: String, buttons: Buttons) -> Op {
        self.emitted.insert(key.to_string(), content.clone());
        Op::Upsert {
            key: key.to_string(),
            content,
            buttons,
        }
    }

    fn turn_mut(&mut self, turn_id: &str) -> Option<&mut TurnView> {
        self.turns.iter_mut().rev().find(|t| t.turn_id == turn_id)
    }

    fn update_tool(&mut self, turn_id: &str, tool_use_id: &str, f: impl Fn(&mut ToolLine)) {
        if let Some(t) = self.turn_mut(turn_id) {
            for lv in t.loops.values_mut() {
                if let Some(l) = lv.tools.iter_mut().find(|l| l.tool_use_id == tool_use_id) {
                    f(l);
                    t.dirty = true;
                }
            }
        }
    }
}

/// Every message a turn shows, in the order Discord should first see them.
fn render_turn(t: &TurnView) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (li, lv) in &t.loops {
        for (i, part) in split_text(&lv.text, PART_LIMIT).into_iter().enumerate() {
            out.push((format!("{}:L{li}:p{i}", t.turn_id), part));
        }
        if !lv.tools.is_empty() {
            out.push((format!("{}:L{li}:tools", t.turn_id), tool_lines(&lv.tools)));
        }
    }
    if let Some(f) = &t.failure {
        out.push((format!("{}:failed", t.turn_id), f.clone()));
    }
    if let Some(f) = &t.footer {
        // The footer rides on the last text part when it fits, else stands alone.
        match out.last_mut() {
            Some((key, content))
                if key.contains(":p") && content.len() + f.len() < DISCORD_LIMIT =>
            {
                content.push('\n');
                content.push_str(f);
            }
            _ => out.push((format!("{}:footer", t.turn_id), f.clone())),
        }
    }
    out
}

fn tool_lines(tools: &[ToolLine]) -> String {
    let lines: Vec<String> = tools
        .iter()
        .map(|l| {
            let mark = match l.notice.as_deref() {
                Some("off_policy") => " · 🚨 against policy",
                Some("irreversible") => " · ⛔ irreversible",
                Some(_) => " · ⚠️ without approval",
                None => "",
            };
            let head = format!("`{}` {}{mark}", l.tool, l.summary);
            match &l.state {
                ToolState::Proposed => format!("▫️ {head}"),
                ToolState::Running => format!("⏳ {head}"),
                ToolState::Waiting => format!("⏸️ {head} · waiting for approval"),
                ToolState::Background => {
                    format!("⏳ {head} · running in the background; the result will come back here")
                }
                ToolState::Done { status, ms } if status == "ok" => format!("✅ {head} · {ms} ms"),
                ToolState::Done { status, ms } => format!("❌ {head} · {status} · {ms} ms"),
                ToolState::Denied(r) if r.is_empty() => format!("🚫 {head} · denied"),
                ToolState::Denied(r) => format!("🚫 {head} · denied: {}", clip(r, 200)),
                ToolState::Answered { approved: true, by } => {
                    format!("👍 {head} · approved by {by}")
                }
                ToolState::Answered {
                    approved: false,
                    by,
                } => {
                    format!("👎 {head} · declined by {by}")
                }
            }
        })
        .collect();
    // Keep the newest lines when a loop made more calls than one message holds.
    let mut kept: Vec<&String> = Vec::new();
    let mut len = 0;
    for l in lines.iter().rev() {
        if len + l.len() + 1 > PART_LIMIT - 40 {
            break;
        }
        len += l.len() + 1;
        kept.push(l);
    }
    kept.reverse();
    let hidden = lines.len() - kept.len();
    let mut s = String::new();
    if hidden > 0 {
        s.push_str(&format!("-# … {hidden} earlier call(s)\n"));
    }
    s.push_str(
        &kept
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    s
}

fn footer(r: &TurnSubmitResult) -> String {
    let mut bits = vec![];
    if !r.profile.is_empty() {
        bits.push(r.profile.clone());
    }
    bits.push(r.model.clone());
    bits.push(format!(
        "{} loop{}",
        r.loops,
        if r.loops == 1 { "" } else { "s" }
    ));
    if r.tool_calls > 0 {
        bits.push(format!(
            "{} tool call{}",
            r.tool_calls,
            if r.tool_calls == 1 { "" } else { "s" }
        ));
    }
    if let Some(c) = r.cost_usd {
        bits.push(format!("${c:.4}"));
    }
    bits.push(format!("{:.1} s", r.elapsed_ms as f64 / 1000.0));
    if r.continuation {
        bits.push("continued".into());
    }
    if r.awaiting_confirm.is_some() {
        bits.push("waiting for your approval".into());
    } else if !matches!(r.stop_reason.as_str(), "" | "no_tool_calls" | "end_turn") {
        bits.push(format!("stopped: {}", r.stop_reason));
    }
    format!("-# {}", bits.join(" · "))
}

/// One short line for a tool call: the argument a person would look for.
pub fn summarize(tool: &str, input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::to_string);
    let text = if let Some(argv) = input.get("argv").and_then(Value::as_array) {
        argv.iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    } else if let Some(p) = s("pattern") {
        match s("path") {
            Some(d) => format!("{p} in {d}"),
            None => p,
        }
    } else if let Some(p) = s("path").or_else(|| s("rev")).or_else(|| s("file")) {
        p
    } else if tool == "fs.patch" {
        "(patch)".into()
    } else {
        serde_json::to_string(input).unwrap_or_default()
    };
    clip(&text.replace('`', "'").replace('\n', " "), 90)
}

fn denial_reason(gate: Option<&Value>) -> Option<String> {
    let r = gate?.get("result")?;
    (r.get("gate").and_then(Value::as_str) == Some("deny")).then(|| str_of(r, "reason"))
}

/// The consequences a notice names (`needs approval: opaque`), or nothing.
fn consequences(p: &Value) -> String {
    let tags: Vec<ConsequenceTag> =
        serde_json::from_value(p.get("consequences").cloned().unwrap_or_default())
            .unwrap_or_default();
    ConsequenceTag::summary(&tags)
}

fn str_of(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Split streamed text into Discord-sized parts. Cuts prefer a newline, then a
/// space, in the second half of the budget; a code fence left open by a cut is
/// closed at the end of the part and reopened (with its language) at the start
/// of the next. Stable under appends: text only grows, so earlier cuts stay put.
pub fn split_text(text: &str, limit: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut rest = text;
    let mut carry: Option<String> = None;
    while !rest.is_empty() {
        let prefix = carry.as_ref().map(|f| format!("{f}\n")).unwrap_or_default();
        let budget = limit.saturating_sub(prefix.len() + 4).max(1);
        if rest.len() <= budget {
            parts.push(prefix + rest);
            break;
        }
        let mut cut = floor_boundary(rest, budget);
        let head = &rest[..cut];
        if let Some(i) = head.rfind('\n').filter(|&i| i > budget / 2) {
            cut = i + 1;
        } else if let Some(i) = head.rfind(' ').filter(|&i| i > budget / 2) {
            cut = i + 1;
        }
        let (head, tail) = rest.split_at(cut);
        let mut part = prefix + head;
        carry = open_fence(&part);
        if carry.is_some() {
            if !part.ends_with('\n') {
                part.push('\n');
            }
            part.push_str("```");
        }
        parts.push(part);
        rest = tail;
    }
    parts
}

fn floor_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// The opening line of a code fence left open at the end of `s`, if any.
fn open_fence(s: &str) -> Option<String> {
    let mut open: Option<String> = None;
    for line in s.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            open = match open {
                Some(_) => None,
                None => Some(t.trim_end().to_string()),
            };
        }
    }
    open
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn upserts(ops: &[Op]) -> Vec<(String, String)> {
        ops.iter()
            .filter_map(|o| match o {
                Op::Upsert { key, content, .. } => Some((key.clone(), content.clone())),
                Op::Typing | Op::Notice { .. } => None,
            })
            .collect()
    }

    fn ended(turn: &str, awaiting: Option<&str>) -> Value {
        json!({"session_id": "s", "turn_id": turn, "loops": 2, "output": "", "stop_reason": "no_tool_calls",
               "provider_stop_reason": null, "model": "claude-sonnet-5", "provider": "anthropic", "profile": "sonnet",
               "usage": {"input_tokens": 1, "output_tokens": 2, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0},
               "elapsed_ms": 4200, "tool_calls": 1, "cost_usd": 0.0123, "awaiting_confirm": awaiting, "continuation": false})
    }

    #[test]
    fn a_turn_streams_as_edits_and_ends_with_a_footer() {
        let mut r = Renderer::default();
        assert_eq!(
            r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"})),
            vec![Op::Typing]
        );
        assert!(r.busy());
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 0, "text": "Hel"}),
        );
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 0, "text": "lo"}),
        );
        let first = upserts(&r.tick());
        assert_eq!(first, vec![("t1:L0:p0".into(), "Hello".into())]);
        assert!(r.tick().is_empty(), "nothing changed, nothing sent");
        r.on_notification(
            "tool.proposed",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
            "input": {"path": "src/lib.rs"}, "gate": {"result": {"gate": "allow"}}}),
        );
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 12}),
        );
        let ops =
            upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
        assert_eq!(
            ops,
            vec![(
                "t1:L0:tools".into(),
                "✅ `fs.read` src/lib.rs · 12 ms".into()
            )]
        );
        r.on_notification(
            "model.delta",
            &json!({"turn_id": "t1", "loop_index": 1, "text": "Done."}),
        );
        let ops = upserts(&r.on_notification("turn.ended", &ended("t1", None)));
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t1:L1:p0");
        assert!(
            ops[0].1.starts_with(
                "Done.\n-# sonnet · claude-sonnet-5 · 2 loops · 1 tool call · $0.0123 · 4.2 s"
            ),
            "{}",
            ops[0].1
        );
        assert!(!r.busy());
    }

    #[test]
    fn a_confirm_gets_buttons_and_loses_them_when_answered() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "needs_confirm", "by": "operator"}}}));
        let ops = r.on_notification("confirm.requested", &json!({"correlation_id": "act_1", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "reason": "run cargo test",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 1790000000000u64}));
        match &ops[0] {
            Op::Upsert {
                key,
                content,
                buttons,
            } => {
                assert_eq!(key, "confirm:act_1");
                assert!(content.starts_with("**Approve?** `proc.run` cargo test\nrun cargo test"));
                assert_eq!(buttons, &Buttons::Confirm("act_1".into()));
            }
            other => panic!("{other:?}"),
        }
        let tools = upserts(&r.on_notification("turn.ended", &ended("t1", Some("act_1"))));
        assert!(tools
            .iter()
            .any(|(k, c)| k == "t1:L0:tools" && c.contains("waiting for approval")));
        let ops = r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": true, "by": "discord:eddie"}),
        );
        assert_eq!(
            ops,
            vec![Op::Upsert {
                key: "confirm:act_1".into(),
                content: "✅ **Approved** by discord:eddie · `proc.run` cargo test".into(),
                buttons: Buttons::Clear
            }]
        );
        let tools = upserts(&r.tick());
        assert!(
            tools[0].1.contains("approved by discord:eddie"),
            "{tools:?}"
        );
    }

    #[test]
    fn a_notified_call_posts_a_card_and_the_card_gets_its_outcome() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["cargo", "test"]}, "gate": {"result": {"gate": "allow"},
            "decision": {"mode": "allow", "notify": {"kind": "approval_skipped", "setting": "enforcement = notify", "rule": "run `cargo test`: proc.run is `confirm` for run tools"}}}}));
        let ops = r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u1",
            "tool": "proc.run", "input": {"argv": ["cargo", "test"]}, "summary": "run `cargo test` in /w",
            "kind": "approval_skipped", "setting": "enforcement = notify", "rule": "run `cargo test`: proc.run is `confirm` for run tools"}));
        let Op::Notice { key, card } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(key, "notice:u1");
        assert_eq!(card.color, AMBER);
        assert_eq!(card.description, "`proc.run` cargo test");
        assert_eq!(card.fields[3], ("Outcome".into(), "⏳ running".into()));
        let ops = r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "ok", "duration_ms": 900}),
        );
        let Op::Notice { card, .. } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert_eq!(card.fields[3].1, "✅ ok · 900 ms");
        let lines =
            upserts(&r.on_notification("loop.ended", &json!({"turn_id": "t1", "loop_index": 0})));
        assert!(lines[0].1.contains("⚠️ without approval"), "{lines:?}");
    }

    #[test]
    fn irreversible_confirms_and_named_notices_say_what_the_call_would_do() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "proc.run",
            "input": {"argv": ["git", "push", "--force"]}, "gate": {"result": {"gate": "confirm"}}}));
        let ops = r.on_notification("confirm.requested", &json!({"correlation_id": "act_1", "session_id": "s",
            "execution_id": "e", "tool": "proc.run", "input": {"argv": ["git", "push", "--force"]},
            "reason": "irreversible: history_rewrite: an irreversible call waits for approval at every enforcement level (enforcement = notify)",
            "by": "operator", "requested_at_ms": 1, "expires_at_ms": 900001, "irreversible": true,
            "consequences": [{"kind": "history_rewrite", "irreversible": true, "rule": "git.push.force", "detail": "git push --force"}]}));
        let up = upserts(&ops);
        assert!(
            up[0].1.starts_with("⛔ **Irreversible. Approve?**"),
            "{}",
            up[0].1
        );
        assert!(
            up[0].1.contains("**irreversible: history_rewrite**"),
            "{}",
            up[0].1
        );
        let lines = upserts(&r.tick());
        assert!(
            lines[0].1.contains("⛔ irreversible") && lines[0].1.contains("waiting for approval"),
            "{lines:?}"
        );
        let ops = r.on_notification(
            "confirm.resolved",
            &json!({"correlation_id": "act_1", "approved": false, "by": "discord:eddie"}),
        );
        assert!(
            upserts(&ops)[0]
                .1
                .contains("(irreversible: history_rewrite)"),
            "{ops:?}"
        );
        // A notice names a needs-approval kind that ran.
        let ops = r.on_notification("policy.notified", &json!({"session_id": "s", "turn_id": "t1", "tool_use_id": "u2",
            "tool": "proc.run", "input": {"argv": ["python3", "-c", "x"]}, "summary": "run `python3 -c x` in /w",
            "kind": "approval_skipped", "setting": "enforcement = notify", "rule": "run: proc.run is `confirm` for run tools",
            "consequences": [{"kind": "opaque", "irreversible": false, "rule": "opaque.inline"}]}));
        let Op::Notice { card, .. } = &ops[0] else {
            panic!("{ops:?}")
        };
        assert!(
            card.fields
                .contains(&("Consequences".into(), "needs approval: opaque".into())),
            "{card:?}"
        );
        assert!(card.fields.iter().any(|(n, _)| n == "Outcome"));
    }

    #[test]
    fn a_turn_seen_only_at_its_end_still_shows_its_text() {
        let mut r = Renderer::default();
        let mut end = ended("t9", None);
        end["output"] = json!("Done. The command printed `done`.");
        end["continuation"] = json!(true);
        let ops = upserts(&r.on_notification("turn.ended", &end));
        assert_eq!(ops.len(), 1);
        assert_eq!(ops[0].0, "t9:L0:p0");
        assert!(
            ops[0]
                .1
                .starts_with("Done. The command printed `done`.\n-# sonnet"),
            "{}",
            ops[0].1
        );
        assert!(ops[0].1.contains("continued"));
        assert!(!r.busy());
    }

    #[test]
    fn denied_and_failed_turns_say_why() {
        let mut r = Renderer::default();
        r.on_notification("turn.started", &json!({"session_id": "s", "turn_id": "t1"}));
        r.on_notification("tool.proposed", &json!({"turn_id": "t1", "tool_use_id": "u1", "tool": "fs.read",
            "input": {"path": "~/.ssh/config"}, "gate": {"result": {"gate": "deny", "reason": "protected path"}}}));
        r.on_notification(
            "tool.ended",
            &json!({"turn_id": "t1", "tool_use_id": "u1", "status": "denied", "duration_ms": 0}),
        );
        let ops = upserts(&r.on_notification(
            "turn.failed",
            &json!({"session_id": "s", "turn_id": "t1", "class": "overloaded", "error": "try later"}),
        ));
        assert!(ops.contains(&(
            "t1:L0:tools".into(),
            "🚫 `fs.read` ~/.ssh/config · denied: protected path".into()
        )));
        assert!(ops.contains(&(
            "t1:failed".into(),
            "⚠️ **Turn failed** (overloaded): try later".into()
        )));
        assert!(!r.busy());
    }

    #[test]
    fn long_text_splits_under_the_limit_and_repairs_fences() {
        let code: String = (0..300).map(|i| format!("let x{i} = {i};\n")).collect();
        let text = format!("Here:\n```rust\n{code}```\nThat is all.");
        let parts = split_text(&text, PART_LIMIT);
        assert!(parts.len() > 1);
        for p in &parts {
            assert!(p.len() <= DISCORD_LIMIT, "{}", p.len());
            assert_eq!(
                p.matches("```").count() % 2,
                0,
                "each part's fences balance: {p}"
            );
        }
        assert!(parts[1].starts_with("```rust\n"));
        // Appending never moves an earlier cut.
        let longer = split_text(&format!("{text} More."), PART_LIMIT);
        assert_eq!(parts[..parts.len() - 1], longer[..parts.len() - 1]);
    }

    #[test]
    fn summaries_pick_the_argument_a_person_looks_for() {
        assert_eq!(
            summarize("proc.run", &json!({"argv": ["cargo", "test", "-q"]})),
            "cargo test -q"
        );
        assert_eq!(
            summarize("fs.grep", &json!({"pattern": "fn main", "path": "src"})),
            "fn main in src"
        );
        assert_eq!(summarize("fs.read", &json!({"path": "a`b"})), "a'b");
        assert!(summarize("fs.write", &json!({"path": "x", "content": "secret body"})) == "x");
    }
}
