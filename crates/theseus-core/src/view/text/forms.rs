//! The three forms of an exchange, from its stored nodes (design §5.1, "Forms"; budgeter rule 8):
//!
//! - [`whole`]: as stored, the person's words, the replies' text, each tool call and its result.
//! - [`spoken`]: the person's words and the replies' text; each call and its result one line built by code
//!   from the call ([`stub`]), none of the result's text.
//! - [`line`]: the person's words exactly, then the `did` line, or `reply not summarised`, and the handle.
//!
//! A cut ([`fit`], [`ends`]) is marked where it is made and names the handle that opens the fuller form; it
//! never parts a tool call from its result, and never cuts or rewrites the person's words.

use std::collections::HashMap;

use serde_json::Value;

use crate::node::{Body, Node};
use crate::view::render::Role;
use crate::view::{Form, Handle};

use super::labels;

/// Who one part of a shown exchange is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Person,
    Theseus,
    Call,
    Result,
    /// A cut's mark: code's own words, no source.
    Mark,
}

impl Voice {
    /// The window's role of a part; `None` for a mark, which is never a turn of its own.
    pub fn role(self) -> Option<Role> {
        match self {
            Self::Person => Some(Role::Person),
            Self::Theseus => Some(Role::Theseus),
            Self::Call => Some(Role::ToolCall),
            Self::Result => Some(Role::ToolResult),
            Self::Mark => None,
        }
    }
}

/// One part of a shown exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub voice: Voice,
    pub text: String,
    /// The node it is read from; empty for a mark.
    pub node: String,
    /// A call's or a result's `tool_use_id`: what pairs them.
    pub call: Option<String>,
    /// A result from outside: its source (the page's URL, the search's request). Wrapped where rendered.
    pub outside: Option<String>,
}

impl Said {
    fn of(voice: Voice, n: &Node, text: String) -> Self {
        Self {
            voice,
            text,
            node: n.id.clone(),
            call: None,
            outside: None,
        }
    }
}

/// An exchange's parts as stored, in order. Nodes that are no part of what was said (a recall, a summary)
/// are left out; a reply with no text (only calls) has no part of its own.
pub fn whole(nodes: &[Node]) -> Vec<Said> {
    nodes
        .iter()
        .filter_map(|n| match &n.body {
            Body::UserMessage { .. } => Some(Said::of(Voice::Person, n, crate::recall::text_of(n))),
            Body::AssistantMessage { blocks, .. } => {
                let text = crate::provider::text_of(blocks);
                (!text.is_empty()).then(|| Said::of(Voice::Theseus, n, text))
            }
            Body::ToolCall {
                tool_use_id,
                wire_name,
                input,
                ..
            } => Some(Said {
                call: Some(tool_use_id.clone()),
                ..Said::of(Voice::Call, n, format!("{wire_name} {input}"))
            }),
            Body::ToolResult {
                tool_use_id,
                content,
                external,
                ..
            } => Some(Said {
                call: Some(tool_use_id.clone()),
                outside: external.as_ref().map(|e| e.url.clone()),
                ..Said::of(Voice::Result, n, content.clone())
            }),
            _ => None,
        })
        .collect()
}

/// An exchange's spoken form: [`whole`] with each call and its result one stub ([`stub`]), where the call
/// was. A result whose call is not among `nodes` is its stub alone.
pub fn spoken(nodes: &[Node]) -> Vec<Said> {
    let results: HashMap<&str, &Node> = nodes
        .iter()
        .filter_map(|n| match &n.body {
            Body::ToolResult { tool_use_id, .. } => Some((tool_use_id.as_str(), n)),
            _ => None,
        })
        .collect();
    let called: Vec<&str> = nodes
        .iter()
        .filter_map(|n| match &n.body {
            Body::ToolCall { tool_use_id, .. } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    nodes
        .iter()
        .filter_map(|n| match &n.body {
            Body::ToolCall {
                tool_use_id,
                wire_name,
                input,
                ..
            } => {
                let r = results.get(tool_use_id.as_str()).copied();
                Some(Said {
                    call: Some(tool_use_id.clone()),
                    ..Said::of(Voice::Call, n, stub(wire_name, input, r))
                })
            }
            Body::ToolResult {
                tool_use_id, tool, ..
            } if !called.contains(&tool_use_id.as_str()) => Some(Said {
                call: Some(tool_use_id.clone()),
                ..Said::of(Voice::Call, n, stub(tool, &Value::Null, Some(n)))
            }),
            Body::ToolResult { .. } => None,
            _ => whole(std::slice::from_ref(n)).pop(),
        })
        .collect()
}

/// A call and its result as one line, from the call and the result's size, never its text:
/// `[fs_read src/lib.rs: 18,233 characters]`, `[proc_run cargo test: error, 41,002 characters]`,
/// `[http_fetch https://example.org: 3,000 characters of outside text]`, `[fs_read a.rs: no result]`.
pub fn stub(name: &str, input: &Value, result: Option<&Node>) -> String {
    let what = labels::one_line(&call_line(input), 160);
    let what = match what.is_empty() {
        true => name.to_string(),
        false => format!("{name} {what}"),
    };
    let back = match result.map(|r| &r.body) {
        Some(Body::ToolResult {
            content,
            is_error,
            external,
            ..
        }) => {
            let chars = crate::narrative::thousands(content.chars().count() as u64);
            let error = if *is_error { "error, " } else { "" };
            let outside = if external.is_some() {
                " of outside text"
            } else {
                ""
            };
            format!("{error}{chars} characters{outside}")
        }
        _ => "no result".into(),
    };
    format!("[{what}: {back}]")
}

/// What a call was asked, read from its input's typed fields in order (a path and its lines, a program's
/// argv or steps, a URL, a query), else its input cut short.
fn call_line(input: &Value) -> String {
    let s = |k: &str| input.get(k).and_then(Value::as_str);
    let argv = |v: &Value| {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
    };
    if let Some(p) = s("path") {
        return match (
            input.get("offset").and_then(Value::as_u64),
            input.get("limit").and_then(Value::as_u64),
        ) {
            (Some(o), Some(l)) => format!("{p}, lines {o}-{}", o + l.saturating_sub(1)),
            (Some(o), None) => format!("{p}, from line {o}"),
            _ => p.to_string(),
        };
    }
    if let Some(a) = input.get("argv").and_then(argv) {
        return a;
    }
    if let Some(steps) = input.get("steps").and_then(Value::as_array) {
        return steps
            .iter()
            .filter_map(|st| argv(&st["argv"]))
            .collect::<Vec<_>>()
            .join(" && ");
    }
    match s("url").or_else(|| s("query")) {
        Some(v) => v.to_string(),
        None if input.is_null() => String::new(),
        None => input.to_string(),
    }
}

/// The person's words of an exchange: its first message's text, exactly.
pub fn ask_of(nodes: &[Node]) -> Option<String> {
    nodes
        .iter()
        .find(|n| matches!(n.body, Body::UserMessage { .. }))
        .map(crate::recall::text_of)
}

/// The line form: the person's words exactly, then `→` and the `did` line, or `reply not summarised` when
/// none was written (or its job failed), and the handle that opens the exchange.
pub fn line(ask: &str, did: Option<&str>, handle: &Handle) -> String {
    let did = did
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .unwrap_or("reply not summarised");
    format!("{ask}\n→ {did} (open {})", handle.0)
}

/// An exchange in `form`, as the text of a marked block ([`text`]); the line form needs the person's words
/// and the `did` line, the others the nodes.
pub fn render(form: Form, nodes: &[Node], did: Option<&str>, handle: &Handle) -> String {
    match form {
        Form::Whole => text(&whole(nodes)),
        Form::Spoken => text(&spoken(nodes)),
        Form::Line => line(&ask_of(nodes).unwrap_or_default(), did, handle),
    }
}

/// Parts as one text: each on its own line after who it is (`person:`, `Theseus:`, `call:`, `result:`); a
/// result from outside wrapped ([`labels::wrap_outside`]); a mark as it is.
pub fn text(parts: &[Said]) -> String {
    parts
        .iter()
        .map(|p| match p.voice {
            Voice::Person => format!("person: {}", p.text),
            Voice::Theseus => format!("Theseus: {}", p.text),
            Voice::Call => format!("call: {}", p.text),
            Voice::Result => match &p.outside {
                Some(src) => format!("result: {}", labels::wrap_outside(Some(src), &p.text)),
                None => format!("result: {}", p.text),
            },
            Voice::Mark => p.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The units a cut takes or leaves whole: a message of the person's, or a reply with the calls it made and
/// their results (by `tool_use_id`, wherever the result was stored). Each unit is the indices of its parts.
fn units(parts: &[Said]) -> Vec<Vec<usize>> {
    let mut units: Vec<Vec<usize>> = Vec::new();
    let mut of_call: HashMap<&str, usize> = HashMap::new();
    for (i, p) in parts.iter().enumerate() {
        let call = p.call.as_deref();
        match p.voice {
            Voice::Result => match call.and_then(|c| of_call.get(c).copied()) {
                Some(u) => units[u].push(i),
                None => units.push(vec![i]),
            },
            // A call joins the reply that made it (a reply's text, or calls with none).
            Voice::Call => {
                match units.last_mut() {
                    Some(u) if matches!(parts[u[0]].voice, Voice::Theseus | Voice::Call) => {
                        u.push(i)
                    }
                    _ => units.push(vec![i]),
                }
                if let Some(c) = call {
                    of_call.insert(c, units.len() - 1);
                }
            }
            _ => units.push(vec![i]),
        }
    }
    units
}

/// `parts` within about `max` tokens: the person's words and the last reply are always kept; the other units
/// are kept in order while they fit, and each run of cut ones is one mark where it was:
/// `[cut here: 3 parts, about 9,000 tokens; open <handle> for the whole exchange]`. A call and its result are
/// one unit, so a cut never parts them.
pub fn fit(parts: Vec<Said>, max: u64, handle: &Handle) -> Vec<Said> {
    let units = units(&parts);
    let size = |u: &Vec<usize>| -> u64 { u.iter().map(|&i| super::tokens(&parts[i].text)).sum() };
    let last_reply = units
        .iter()
        .rposition(|u| parts[u[0]].voice == Voice::Theseus);
    let kept_always = |k: usize| parts[units[k][0]].voice == Voice::Person || Some(k) == last_reply;
    let mut room = max.saturating_sub(
        (0..units.len())
            .filter(|&k| kept_always(k))
            .map(|k| size(&units[k]))
            .sum(),
    );
    let mut out = Vec::new();
    let mut cut: (usize, u64) = (0, 0);
    for (k, u) in units.iter().enumerate() {
        let keep = kept_always(k) || size(u) <= room;
        if !keep {
            cut = (cut.0 + u.len(), cut.1 + size(u));
            continue;
        }
        if !kept_always(k) {
            room -= size(u);
        }
        if cut.0 > 0 {
            out.push(mark(cut, handle));
            cut = (0, 0);
        }
        out.extend(u.iter().map(|&i| parts[i].clone()));
    }
    if cut.0 > 0 {
        out.push(mark(cut, handle));
    }
    out
}

fn mark((n, tokens): (usize, u64), handle: &Handle) -> Said {
    let parts = crate::narrative::count(n as u64, "part", "parts");
    Said {
        voice: Voice::Mark,
        text: format!(
            "[cut here: {parts}, about {} tokens; open {} for the whole exchange]",
            crate::narrative::thousands(tokens),
            handle.0
        ),
        node: String::new(),
        call: None,
        outside: None,
    }
}

/// A long reply's two ends: its first `head` and last `tail` tokens (about), with a mark where the middle was
/// cut: `[cut here: about 4,200 tokens; open <handle> for all of it]`. The person's words are never cut: a part
/// of theirs, or one that fits, comes back as it is.
pub fn ends(part: &Said, head: u64, tail: u64, handle: &Handle) -> Said {
    let per = crate::catalog::TokenRates::CLAUDE.text;
    let (h, t) = ((head as f64 * per) as usize, (tail as f64 * per) as usize);
    let s = &part.text;
    if part.voice == Voice::Person || s.len() <= h + t {
        return part.clone();
    }
    let a = floor_char(s, h);
    let b = ceil_char(s, s.len() - t);
    let gone = super::tokens(&s[a..b]);
    Said {
        text: format!(
            "{}\n[cut here: about {} tokens; open {} for all of it]\n{}",
            &s[..a],
            crate::narrative::thousands(gone),
            handle.0,
            &s[b..]
        ),
        ..part.clone()
    }
}

fn floor_char(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char(s: &str, mut i: usize) -> usize {
    while !s.is_char_boundary(i) {
        i += 1;
    }
    i
}
