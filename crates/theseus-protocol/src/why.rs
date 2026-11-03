//! `theseus why` and Discord's `/why` (theseus-ighf): what a turn's model was
//! shown, and why each piece was there, as plain lines. One function builds
//! them from what the daemon already answers (`context.explain`, the turn's
//! `route.decided` row, the turn's tool calls, and `policy.explain`), so the
//! CLI, Discord, and the commands that follow them say the same words. Pure:
//! results in, text out; no colour, no clock, no write. A client that styles
//! text reads the plain lines (term-render).
//!
//! The short form is the same sections with their top items and a count of
//! the rest, cut to a character budget (Discord's limits): never a second
//! wording.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::context::{ContextDetour, ContextExplainResult, ContextPart, ContextSource};
use crate::memory::{RecallDrop, RecallItem, RecallManifest};
use crate::{LedgerTailResult, NodeListResult, PolicyExplainResult, ToolExplain};

/// What one `why` is built from.
pub struct WhyInput<'a> {
    pub explain: &'a ContextExplainResult,
    /// `policy.explain` for the turn's session: the gate's layers per tool.
    pub policy: Option<&'a PolicyExplainResult>,
    /// The data of the turn's `route.decided` row, when it has one.
    pub route: Option<&'a Value>,
    /// The tools the turn called, in the order of their first call.
    pub tools: &'a [ToolCalled],
    /// Show no private or imported source's words, only that one was there
    /// (a shared place). A `shared` class forces it on.
    pub hide_sources: bool,
}

/// The sections of a `why`, each a list of lines, so the short form can keep
/// a section's top items and say how many it left.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Why {
    pub head: Vec<String>,
    pub ran: Vec<String>,
    pub blocks: Vec<String>,
    pub recalled: Vec<String>,
    pub left_out: Vec<String>,
    pub tools: Vec<String>,
}

/// A tool the turn called: how many times, and what the gate decided for the
/// calls (the last one's, as its `tool_call` node recorded it).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolCalled {
    pub tool: String,
    pub calls: u32,
    /// The gate's result for the call: `open`, `notify`, `approve`, `refused`.
    pub gate: Option<String>,
    /// The gate's reason for it.
    pub reason: Option<String>,
}

/// The tools a turn's `tool_call` nodes (`node.list`'s) name, in the order of
/// their first call.
pub fn tools_called(nodes: &NodeListResult, turn: &str) -> Vec<ToolCalled> {
    let mut calls: Vec<&crate::NodeInfo> = nodes
        .nodes
        .iter()
        .filter(|n| n.kind == "tool_call" && n.turn_id.as_deref() == Some(turn))
        .collect();
    calls.sort_by_key(|n| n.position);
    let mut out: Vec<ToolCalled> = Vec::new();
    for n in calls {
        let Some(name) = n.detail.get("tool").and_then(Value::as_str) else {
            continue;
        };
        let gate = n
            .detail
            .pointer("/gate/result/gate")
            .and_then(Value::as_str);
        let reason = n
            .detail
            .pointer("/gate/decision/reason")
            .and_then(Value::as_str);
        let at = out.iter().position(|t| t.tool == name);
        let t = match at {
            Some(k) => &mut out[k],
            None => {
                out.push(ToolCalled {
                    tool: name.to_string(),
                    ..ToolCalled::default()
                });
                out.last_mut().unwrap()
            }
        };
        t.calls += 1;
        t.gate = gate.map(str::to_string).or(t.gate.take());
        t.reason = reason.map(str::to_string).or(t.reason.take());
    }
    out
}

/// The data of the turn's rows in a ledger read, the newest last.
pub fn turn_rows(rows: &LedgerTailResult, turn: &str) -> Vec<Value> {
    let mut of: Vec<_> = rows
        .rows
        .iter()
        .filter(|e| e.turn_id.as_deref() == Some(turn))
        .collect();
    of.sort_by_key(|e| e.position);
    of.into_iter().map(|e| e.data.clone()).collect()
}

/// All of it: every section, every item.
pub fn why_lines(i: &WhyInput) -> Vec<String> {
    let w = build(i, false);
    let mut out = w.head;
    out.extend(w.ran);
    out.extend(w.blocks);
    out.extend(w.recalled);
    out.extend(w.left_out);
    out.extend(w.tools);
    out
}

/// The short form: each section's first `top` items and a line counting the
/// rest, cut to `max_chars` (whole lines, the last saying how many it left).
pub fn why_short(i: &WhyInput, top: usize, max_chars: usize) -> String {
    // Same sections; the blocks' top items are the biggest.
    let w = build(i, true);
    let mut lines = w.head;
    lines.extend(w.ran);
    for sec in [w.blocks, w.recalled, w.left_out, w.tools] {
        lines.extend(top_items(sec, top));
    }
    // One long line (a withheld text's reason, a tool list) must not use the
    // room the sections after it need.
    let lines = lines.iter().map(|l| clip(l, LINE_CHARS)).collect();
    fit(lines, max_chars)
}

/// A section is a heading line, then items (each item's line, with its
/// continuation lines indented four spaces or more, kept with it).
fn top_items(sec: Vec<String>, top: usize) -> Vec<String> {
    let Some((head, rest)) = sec.split_first() else {
        return sec;
    };
    let mut items: Vec<Vec<&String>> = Vec::new();
    for l in rest {
        if l.starts_with("     ") && !items.is_empty() {
            items.last_mut().unwrap().push(l);
        } else {
            items.push(vec![l]);
        }
    }
    let mut out = vec![head.clone()];
    for it in items.iter().take(top) {
        out.extend(it.iter().map(|l| (*l).clone()));
    }
    if items.len() > top {
        out.push(format!("  … and {} more", items.len() - top));
    }
    out
}

fn fit(lines: Vec<String>, max_chars: usize) -> String {
    let total = lines.len();
    let mut out = String::new();
    let mut kept = 0;
    for l in &lines {
        // Room for the closing line, which names what was left.
        let reserve = 60;
        if out.chars().count() + l.chars().count() + 1 + reserve > max_chars && kept < total {
            break;
        }
        out.push_str(l);
        out.push('\n');
        kept += 1;
    }
    if kept < total {
        out.push_str(&format!(
            "… {} more lines: `theseus why` in a terminal has all of it",
            total - kept
        ));
    }
    out.trim_end().to_string()
}

/// `largest_first`: the short form's blocks, biggest first, so its top items
/// are the ones that cost the most.
fn build(i: &WhyInput, largest_first: bool) -> Why {
    let x = i.explain;
    let hide = i.hide_sources || x.class != "private";
    let mut w = Why::default();
    let turn = x.turn_id.as_deref().unwrap_or("(no turn yet)");
    w.head.push(format!(
        "turn {turn} · session {}{}",
        x.session_id,
        x.title
            .as_deref()
            .map(|t| format!(" ({t})"))
            .unwrap_or_default()
    ));
    w.head.push(format!("place: {} ({})", x.place, x.class));
    if x.turn_id.is_none() {
        w.head
            .push("no turn of this session has compiled a context yet".into());
        return w;
    }
    if let Some(d) = &x.detour {
        w.head.push(detour_words(d));
    }
    w.ran = ran(i);
    w.blocks = blocks(i, largest_first);
    w.recalled = recalled(i, hide);
    w.left_out = left_out(i);
    w.tools = tools(i);
    w
}

/// The model and profile, and what routing said about them.
fn ran(i: &WhyInput) -> Vec<String> {
    let x = i.explain;
    let mut line = format!("ran on: {} ({})", x.profile, x.model);
    if let Some(w) = x.window {
        line.push_str(&format!(", a window of {} tokens", num(w)));
    }
    let mut out = vec![line];
    let Some(r) = i.route else {
        out.push(
            "routing: no decision recorded for this turn (a pinned profile, or routing off)".into(),
        );
        return out;
    };
    let s = |k: &str| r.get(k).and_then(Value::as_str);
    let reason = s("reason").unwrap_or("unknown");
    if s("pack").is_some() {
        out.push(open_routing(r));
        if let Some(e) = s("effort") {
            let how = s("effort_reason").unwrap_or("recorded");
            let applied = match r.get("effort_ran").and_then(Value::as_str) {
                Some(ran) if r["effort_applied"] == true => {
                    format!(", the request carried {ran}")
                }
                _ => String::new(),
            };
            out.push(format!("effort: Jev said {e} ({how}){applied}"));
        }
        if let Some(i) = r.get("injection") {
            out.push(injection_words(i));
        }
        return out;
    }
    let mut why = match (s("source"), s("mode")) {
        (Some(src), Some(m)) => format!("task class {m}, placed by {src}"),
        (Some(src), None) => format!("placed by {src}"),
        (None, Some(m)) => format!("task class {m}, {}", reason_words(reason)),
        (None, None) => reason_words(reason).to_string(),
    };
    let (from, to) = (s("from").unwrap_or(""), s("profile").unwrap_or(""));
    if !from.is_empty() && !to.is_empty() && from != to {
        match r["detour"] == true {
            true => why.push_str(&format!(", on {to} instead of {from}")),
            false => why.push_str(&format!(", moved from {from} to {to}")),
        }
    }
    out.push(format!("routing: {why}"));
    if let Some(e) = s("effort") {
        let how = s("effort_reason").unwrap_or("recorded");
        let applied = match r.get("effort_ran").and_then(Value::as_str) {
            Some(ran) if r["effort_applied"] == true => format!(", the request carried {ran}"),
            _ => String::new(),
        };
        out.push(format!("effort: Jev said {e} ({how}){applied}"));
    }
    if let Some(i) = r.get("injection") {
        out.push(injection_words(i));
    }
    out
}

/// The route line when route.v10 acted (theseus-h66v4): the model that ran, who
/// chose it and the first sentences of the block it chose, the effort Jev
/// named, and the reason, in every case, with what qualifies it
/// (`restricted_models=not_allowed`, `-P fable`, `deepseek: no key`).
///
/// `routing: model=claude-opus-5-5 (jev 0.91: "Opus 5.5 (Anthropic, US). … For
/// reviewing code, a plan or an argument for mistakes") effort=medium (0.84)
/// reason=verdict pack=route.v10`
fn open_routing(r: &Value) -> String {
    let s = |k: &str| r.get(k).and_then(Value::as_str);
    let f = |k: &str| r.get(k).and_then(Value::as_f64);
    let reason = s("reason").unwrap_or("unknown");
    let ran = s("model").or(s("profile")).unwrap_or("?");
    let mut line = format!("routing: model={ran}");
    if let (Some(pick), Some(c)) = (s("pick"), f("pick_confidence")) {
        match (reason, s("block")) {
            ("verdict", Some(block)) => {
                line.push_str(&format!(" (jev {c:.2}: \"{block}\")"));
            }
            ("verdict", None) => line.push_str(&format!(" (jev {c:.2})")),
            _ => line.push_str(&format!(" (jev picked {pick} {c:.2})")),
        }
    }
    if let (Some(e), Some(c)) = (s("effort"), f("effort_confidence")) {
        line.push_str(&format!(" effort={e} ({c:.2})"));
    }
    line.push_str(&format!(" reason={reason}"));
    if let Some(d) = s("detail") {
        line.push_str(&format!(" ({d})"));
    }
    line.push_str(&format!(" pack={}", s("pack").unwrap_or("?")));
    line
}

/// What the turn's tier gave it (theseus-d9je7): `injection: deep: recall 8,000
/// tokens, 16 items; tools to 3x the place's budget`.
fn injection_words(i: &Value) -> String {
    let n = |k: &str| i.get(k).and_then(Value::as_u64).unwrap_or(0);
    let tier = i.get("tier").and_then(Value::as_str).unwrap_or("ordinary");
    let tier = match i.get("fallback").and_then(Value::as_str) {
        Some(f) => format!("{tier} ({f})"),
        None => tier.to_string(),
    };
    let recall = match n("recall_items") {
        0 => "no recall (a trivial detour carries none)".to_string(),
        items => format!("recall {} tokens, {items} items", num(n("recall_tokens"))),
    };
    let ceiling = i
        .get("admit_ceiling")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    format!("injection: {tier}: {recall}; tools to {ceiling}x the place's budget")
}

/// How a detoured turn ran (theseus-r5ydi): its request is built outside the
/// conversation's compilation and never stored, so the parts below are the
/// ones its `context.detoured` row names.
pub fn detour_words(d: &ContextDetour) -> String {
    let kind = if d.kind.is_empty() {
        "steered"
    } else {
        &d.kind
    };
    format!(
        "detour: ran as a {kind} detour on {} ({}); its request was built outside the conversation's compilation",
        d.profile, d.model
    )
}

/// What a detour's request leaves out of the conversation's own, as plain
/// lines: a quick detour carries the turn's own recall, a trivial one none.
fn detour_left(d: &ContextDetour) -> Vec<String> {
    let mut out = vec![
        "  context files: a detour carries none".to_string(),
        "  the persona's body: a detour carries only its tail line, when one answers".to_string(),
        "  the conversation's summary and older turns: only the last exchanges ride a detour"
            .to_string(),
        "  earlier recall: a detour carries none".to_string(),
    ];
    if !d.recall.unwrap_or(d.kind == "quick") {
        out.push("  this turn's own recall: a trivial detour carries none".to_string());
    }
    out
}

fn reason_words(r: &str) -> &str {
    match r {
        "verdict" => "by the route pack's verdict",
        "detour" => "a detour, outside the session's compilation",
        "capped" => "held at a cap",
        "fallback" => "a refusal's fallback",
        "cache_hold" => "held to keep the cache",
        "unsure" => "the verdict was unsure",
        "late" => "the verdict came too late, so the last one stood",
        "unreachable" => "the router could not be reached",
        "no_verdict" => "no verdict was read",
        "pinned" => "the profile is pinned",
        "data_gate" => "the data rule kept it off a model of restricted origin",
        "correction" => "the owner's correction placed it",
        "shadow" => "routing only watched",
        other => other,
    }
}

/// The compiled context in the request's order (or biggest first).
fn blocks(i: &WhyInput, largest_first: bool) -> Vec<String> {
    let x = i.explain;
    let total: u64 = x.parts.iter().map(|p| p.tokens).sum();
    let mut out = vec![format!(
        "context: {} parts, {} tokens{}",
        x.parts.len(),
        num(total),
        match x.unchanged {
            Some(true) => " (the system blocks are the bytes this turn saw)".to_string(),
            Some(false) => {
                " (the system blocks have changed since this turn: parts as the next turn would build them)"
                    .to_string()
            }
            None => String::new(),
        }
    )];
    if let Some(u) = &x.unbuilt {
        out.push(format!("  the parts could not be built: {u}"));
    }
    // The project slice rides the request's last block (theseus-n5ui2).
    if let Some(p) = x.compiled.as_ref().and_then(|c| c.project.as_ref()) {
        let left = match p.left_out {
            0 => String::new(),
            n => format!(", {n} left out"),
        };
        out.push(format!(
            "  project     {} · the slice in the request's last block · {} lines · {} tok{left}",
            p.project, p.lines, p.tokens
        ));
    }
    let hide = i.hide_sources || x.class != "private";
    let mut parts: Vec<&ContextPart> = x.parts.iter().collect();
    if largest_first {
        parts.sort_by_key(|p| std::cmp::Reverse(p.tokens));
    }
    out.extend(parts.into_iter().map(|p| part_line(p, hide)));
    out
}

fn part_line(p: &ContextPart, hide: bool) -> String {
    let name = if hide && p.block == "recall" {
        "a recalled source"
    } else {
        p.name.as_str()
    };
    let mut l = format!(
        "  {:<12} {name} · {} tok · {}",
        p.block,
        num(p.tokens),
        part_source(p)
    );
    match p.then.as_deref() {
        Some("changed") => l.push_str(" · changed since"),
        Some("new") => l.push_str(" · new since"),
        _ => {}
    }
    if let Some(n) = &p.note {
        l.push_str(&format!(" · {}", clip(n, NOTE_CHARS)));
    }
    l
}

/// The most characters of a part's note a line keeps (a tools part's note
/// names every tool).
const NOTE_CHARS: usize = 80;

/// The most characters of a line the short form keeps.
const LINE_CHARS: usize = 150;

/// `s` cut to `n` characters, an ellipsis where it was.
fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((at, _)) => format!("{}…", s[..at].trim_end()),
        None => s.to_string(),
    }
}

fn part_source(p: &ContextPart) -> String {
    match p.block.as_str() {
        "header" => "the system's first block".into(),
        "context" => "a context file".into(),
        "guidance" => match p.version {
            Some(v) => format!("a category's guidance, version {v}"),
            None => "a category's guidance".into(),
        },
        "tools" => "the tools offered".into(),
        "recall" => "recalled in front of the model".into(),
        "conversation" => "the conversation so far".into(),
        // A detour's request (theseus-r5ydi).
        "session" => "the session's own block".into(),
        "persona" => "the line saying who answers".into(),
        "standing" => "the standing records of this surface".into(),
        "exchanges" => "the last exchanges, kept for the detour".into(),
        "message" => "this turn's message".into(),
        "project" => "the project this conversation serves".into(),
        other => other.into(),
    }
}

/// Each recalled note with its date, its source, and why it was recalled.
fn recalled(i: &WhyInput, hide: bool) -> Vec<String> {
    let x = i.explain;
    let mut out = Vec::new();
    for m in &x.recalls {
        let n = m.admitted.len();
        out.push(format!(
            "recalled: {n} of {} candidates{}",
            m.candidates,
            outcome_words(m)
        ));
        for (k, it) in m.admitted.iter().enumerate() {
            out.push(format!(
                "  {}. {}{} · {} · {} tok",
                k + 1,
                dated(it, x, hide),
                source_words(&it.session_id, x, hide),
                it.kind,
                num(it.tokens)
            ));
            out.push(format!("     why: {}", why_recalled(it, m)));
        }
    }
    if x.recalls.is_empty() {
        out.push("recalled: nothing recorded for this turn".into());
    }
    out
}

fn outcome_words(m: &RecallManifest) -> String {
    match m.outcome.as_str() {
        "ran" => String::new(),
        o => format!(
            " ({o}{})",
            m.why
                .as_deref()
                .map(|w| format!(": {w}"))
                .unwrap_or_default()
        ),
    }
}

/// Its rank in the science's order, its score, and each source's own rank.
fn why_recalled(it: &RecallItem, m: &RecallManifest) -> String {
    let mut by: Vec<String> = it
        .sources
        .iter()
        .map(|(k, s)| format!("{k} #{} ({:.2})", s.rank, s.score))
        .collect();
    by.sort();
    let mut s = format!(
        "ranked {} of {} hits, fused score {:.2}",
        it.rank, m.candidates, it.fused
    );
    if !by.is_empty() {
        s.push_str(&format!(": {}", by.join(", ")));
    }
    s
}

/// The note's own date and a separator: when it was written, as the daemon
/// read its record. A note of another session in a shared place has none
/// shown, since the source is not.
fn dated(it: &RecallItem, x: &ContextExplainResult, hide: bool) -> String {
    if hide && it.session_id != x.session_id {
        return String::new();
    }
    // An older daemon sends no times: an imported note's episode ends then.
    let ms = x.noted.get(&it.node_id).copied().or_else(|| {
        let e = x.sources.get(&it.session_id)?.imported.as_ref()?;
        Some(e.end_ms)
    });
    ms.map(|ms| format!("{} · ", ymd(ms))).unwrap_or_default()
}

/// Where a recalled node came from; nothing of a private or imported
/// source's own words when `hide`, only that one was there.
fn source_words(session_id: &str, x: &ContextExplainResult, hide: bool) -> String {
    let src = x.sources.get(session_id);
    if hide {
        return match src.and_then(|s| s.imported.as_ref()) {
            Some(_) => "an imported source (not shown in a shared place)".into(),
            None if session_id == x.session_id => "this conversation".into(),
            None => "another session's note (not shown in a shared place)".into(),
        };
    }
    let Some(ContextSource {
        place,
        title,
        imported,
    }) = src
    else {
        return format!("session {session_id}");
    };
    if let Some(e) = imported {
        return format!(
            "an imported {}{} from {}",
            e.place_kind,
            e.place_name
                .as_deref()
                .map(|n| format!(" ({n})"))
                .unwrap_or_default(),
            e.source
        );
    }
    let own = if session_id == x.session_id {
        "this conversation".to_string()
    } else {
        format!("session {session_id}")
    };
    match title {
        Some(t) => format!("{own} ({t}) in {place}"),
        None => format!("{own} in {place}"),
    }
}

/// What was left out, and why.
fn left_out(i: &WhyInput) -> Vec<String> {
    let x = i.explain;
    let mut lines = x.detour.as_ref().map(detour_left).unwrap_or_default();
    for m in &x.recalls {
        let mut by: BTreeMap<&str, u64> = BTreeMap::new();
        for (k, v) in &m.drops {
            *by.entry(k).or_default() += v;
        }
        if by.is_empty() {
            for d in &m.dropped {
                *by.entry(&d.reason).or_default() += 1;
            }
        }
        for (reason, n) in by {
            let tokens: u64 = m
                .dropped
                .iter()
                .filter(|d| d.reason == reason)
                .map(|d: &RecallDrop| d.tokens)
                .sum();
            lines.push(format!(
                "  recall: {n} {} {}{}",
                if n == 1 { "candidate" } else { "candidates" },
                drop_words(reason),
                if tokens > 0 {
                    format!(" ({} tok)", num(tokens))
                } else {
                    String::new()
                }
            ));
        }
        if let Some(b) = &m.budget {
            if let Some(o) = &b.overage {
                lines.push(format!(
                    "  recall: {} tokens over its limit: {}",
                    num(o.tokens),
                    o.why
                ));
            }
        }
    }
    if let Some(c) = &x.compiled {
        if c.withheld > 0 {
            lines.push(format!(
                "  {} context {} carried as {} alone (a shared place)",
                c.withheld,
                if c.withheld == 1 { "file" } else { "files" },
                if c.withheld == 1 {
                    "its header"
                } else {
                    "their headers"
                }
            ));
        }
    }
    if let Some(w) = &x.withheld {
        lines.push(format!("  text withheld: {w}"));
    }
    if lines.is_empty() {
        return vec!["left out: nothing recorded".into()];
    }
    lines.insert(0, "left out:".into());
    lines
}

fn drop_words(reason: &str) -> &'static str {
    match reason {
        "place" => "left out by the place rule",
        "in_context" => "left out, already in context",
        "untrusted" => "left out as external text",
        "labeled_wrong" => "left out, labeled wrong",
        "recursion" => "left out as a harness line",
        "superseded" => "left out, corrected by a newer note",
        "duplicate" => "left out as a newer note's duplicate",
        "threshold" => "left out below the threshold",
        "subject" => "left out, another subject of the same kind",
        "budget" => "left out for the budget",
        "thread" => "left out, another thread's and under the bar",
        "sealed" => "withheld, a sealed thread's",
        _ => "left out for another reason",
    }
}

/// Each tool the turn called: the gate's posture and the layer that set it.
fn tools(i: &WhyInput) -> Vec<String> {
    if i.tools.is_empty() {
        return vec!["tools: the turn called none".into()];
    }
    let mut out = vec!["tools called, and the gate's posture for each:".to_string()];
    let place = i.policy.and_then(|p| p.places.first());
    for c in i.tools {
        let found = place.and_then(|p| p.tools.iter().find(|t| same_tool(&t.tool, &c.tool)));
        out.push(tool_line(c, found));
    }
    out
}

/// A tool's wire name (`fs_read`) and its canonical one (`fs.read`) name the
/// same tool.
fn same_tool(a: &str, b: &str) -> bool {
    let key = |s: &str| s.replace(|c: char| !c.is_ascii_alphanumeric(), "_");
    key(a) == key(b)
}

fn tool_line(c: &ToolCalled, t: Option<&ToolExplain>) -> String {
    let times = if c.calls > 1 {
        format!(" ×{}", c.calls)
    } else {
        String::new()
    };
    let name = format!("{}{times}", c.tool);
    let posture = c
        .gate
        .as_deref()
        .or(t.map(|t| t.result.as_str()))
        .unwrap_or("unknown");
    let Some(t) = t else {
        return format!("  {name:<22} {posture:<8} the gate's layers are not listed for it here");
    };
    if !t.offered {
        return format!(
            "  {name:<22} {posture:<8} not offered here{}",
            t.refused
                .as_deref()
                .map(|r| format!(": {r}"))
                .unwrap_or_default()
        );
    }
    // The layer that last raised the posture; else the config's own.
    let set = t
        .layers
        .iter()
        .rfind(|l| l.raised)
        .or_else(|| t.layers.iter().find(|l| l.layer == "posture"));
    let why = match set {
        Some(l) => match &l.setting {
            Some(s) => format!("set by {}: {s}", l.layer),
            None => format!("set by {}: {}", l.layer, l.says),
        },
        None => t.reason.clone(),
    };
    format!("  {name:<22} {posture:<8} {why}")
}

pub(crate) fn num(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A unix-ms time as a UTC date, `2026-03-04`.
fn ymd(ms: u64) -> String {
    let (y, m, d) = crate::time::civil_from_days((ms / 86_400_000) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}
