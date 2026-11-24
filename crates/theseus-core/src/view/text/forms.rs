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

use super::{labels, literal};

/// Who one part of a shown exchange is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Voice {
    Person,
    /// The harness's own line stored in a turn (its clock line, a nudge, the progress line, a helper's or
    /// the day ceiling's note: `content::harness_own`). It stands where a person's message does, and is never
    /// headed or prefixed as a person's words.
    Harness,
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
            Self::Person | Self::Harness => Some(Role::Person),
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
            Body::UserMessage { .. } => {
                let voice = match crate::content::harness_own(n.origin, n.author.as_deref()) {
                    true => Voice::Harness,
                    false => Voice::Person,
                };
                Some(Said::of(voice, n, crate::recall::text_of(n)))
            }
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

/// The nodes whose own text `form` lays, of an exchange's `nodes` (issue 44; read by `node.reach`): in a
/// `Line`, the person's first message, as both renderers take it; in `Spoken`, the parts it keeps whole,
/// the person's words and the replies' text (a call and its result go as a stub, which is not their text);
/// in `Whole`, every part of [`whole`]. A node that is no part of what was said (a recall, a summary, a
/// reply with no text) is in none.
pub fn laid(nodes: &[Node], form: Form) -> Vec<String> {
    match form {
        Form::Line => (nodes.iter())
            .find(|n| matches!(n.body, Body::UserMessage { .. }))
            .map(|n| vec![n.id.clone()])
            .unwrap_or_default(),
        Form::Spoken => (whole(nodes).into_iter())
            .filter(|s| matches!(s.voice, Voice::Person | Voice::Theseus))
            .map(|s| s.node)
            .collect(),
        Form::Whole => whole(nodes).into_iter().map(|s| s.node).collect(),
    }
}

/// An exchange's spoken form: [`whole`] with each call and its result one stub ([`stub`]), where the call
/// was. A result that answers no call here is its stub alone.
///
/// A result is its call's by proven identity (`compiler::pairing`, issue 44): its correlation, else the ONE
/// call its exchange's records (or, with none, its exchange's calls) name with its id; two candidates are no
/// answer, and no use of an id its response repeats is answered. A late result answers no call (it came back
/// later, and is its stub alone), and a call with no result established reads "no result".
pub fn spoken(nodes: &[Node]) -> Vec<Said> {
    let p = crate::compiler::pairing::pairing(nodes);
    let answer = |n: &Node| p.record(&n.id).and_then(|c| p.result(c));
    nodes
        .iter()
        .filter_map(|n| match &n.body {
            Body::ToolCall {
                tool_use_id,
                wire_name,
                input,
                ..
            } => {
                let r = answer(n);
                Some(Said {
                    call: Some(tool_use_id.clone()),
                    ..Said::of(Voice::Call, n, stub(wire_name, input, r))
                })
            }
            Body::ToolResult {
                tool_use_id, tool, ..
            } if p.call_of(&n.id).is_none() => Some(Said {
                call: Some(tool_use_id.clone()),
                ..Said::of(Voice::Call, n, stub(tool, &Value::Null, Some(n)))
            }),
            Body::ToolResult { .. } => None,
            _ => whole(std::slice::from_ref(n)).pop(),
        })
        .collect()
}

/// A call and its result as one line, from the call and the result's size, never its text; what the call
/// names is a quoted literal ([`literal::literal`]) and the tool's name a plain name or one:
/// `[fs_read "src/lib.rs", lines 1-400: 18,233 characters]`, `[proc_run "cargo test": error, 41,002
/// characters]`, `[http_fetch "https://example.org": 3,000 characters of outside text]`,
/// `[fs_read "a.rs": no result]`.
pub fn stub(name: &str, input: &Value, result: Option<&Node>) -> String {
    let name = literal::name(name);
    let what = match call_line(input) {
        None => name,
        Some(what) => format!("{name} {what}"),
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
/// argv or steps, a URL, a query), else its input cut short; each as a quoted literal. A line range that
/// does not add up is said to be invalid, never computed past `u64`.
fn call_line(input: &Value) -> Option<String> {
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
        let p = literal::literal(p, 160);
        let n = |k: &str| input.get(k).and_then(Value::as_u64);
        return Some(match (n("offset"), n("limit")) {
            (Some(o), Some(l)) => match l.checked_sub(1).and_then(|l| o.checked_add(l)) {
                Some(end) => format!("{p}, lines {o}-{end}"),
                None => format!("{p}, an invalid line range ({o}, {l} lines)"),
            },
            (Some(o), None) => format!("{p}, from line {o}"),
            _ => p,
        });
    }
    let line = match (input.get("argv").and_then(argv), input.get("steps")) {
        (Some(a), _) => a,
        (None, Some(steps)) => steps
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|st| argv(&st["argv"]))
            .collect::<Vec<_>>()
            .join(" && "),
        (None, None) => match s("url").or_else(|| s("query")) {
            Some(v) => v.to_string(),
            None if input.is_null() => return None,
            None => input.to_string(),
        },
    };
    Some(literal::literal(&line, 160))
}

/// A tool result's text as the model reads it wherever it is rendered, the window's real turns included: its
/// content, wrapped when it came from outside ([`labels::wrap_outside`]). `None` for a node that is no result.
pub fn result_text(n: &Node) -> Option<String> {
    match &n.body {
        Body::ToolResult {
            content, external, ..
        } => Some(match external {
            Some(e) => labels::wrap_outside(Some(&e.url), content),
            None => content.clone(),
        }),
        _ => None,
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
    parts.iter().map(said_text).collect::<Vec<_>>().join("\n")
}

/// One part as [`text`] shows it.
pub fn said_text(p: &Said) -> String {
    match p.voice {
        Voice::Person => format!("person: {}", p.text),
        Voice::Harness => format!("harness: {}", p.text),
        Voice::Theseus => format!("Theseus: {}", p.text),
        Voice::Call => format!("call: {}", p.text),
        Voice::Result => match &p.outside {
            Some(src) => format!("result: {}", labels::wrap_outside(Some(src), &p.text)),
            None => format!("result: {}", p.text),
        },
        Voice::Mark => p.text.clone(),
    }
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

/// Why [`try_fit`] could not keep an exchange within its maximum: the words it must keep (the person's, and
/// the last reply), with their framing and cut marks, are `needed` tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overflow {
    pub handle: Handle,
    pub needed: u64,
    pub max: u64,
}

/// `parts` within `max` tokens of their rendered text ([`text`]: prefixes, wrappers and cut marks all
/// charged). The person's words and the last reply are always kept, whole; the other units are kept in order
/// while the whole still fits, and each run of cut ones is one mark where it was:
/// `[cut here: 3 parts, about 9,000 tokens; open <handle> for the whole exchange]`. A call and its result are
/// one unit, so a cut never parts them. When what must be kept cannot fit, an [`Overflow`] says how much it
/// needs: the person's words are never cut to fit.
pub fn try_fit(parts: Vec<Said>, max: u64, handle: &Handle) -> Result<Vec<Said>, Overflow> {
    try_fit_kept(parts, max, handle).map(|kept| kept.into_iter().map(|(p, _)| p).collect())
}

/// What [`try_fit`] or [`fit`] kept of one part it was given: which one (its index), and the bytes of its
/// text, as they are. A fit cuts between parts, never inside one, so the bytes are always all of them.
pub type Kept = (usize, std::ops::Range<usize>);

/// [`try_fit`], each part it gives with what it kept ([`Kept`]), or none for a cut's mark (code's own words).
pub fn try_fit_kept(
    parts: Vec<Said>,
    max: u64,
    handle: &Handle,
) -> Result<Vec<(Said, Option<Kept>)>, Overflow> {
    let units = units(&parts);
    let bytes: Vec<u64> = parts.iter().map(|p| said_text(p).len() as u64).collect();
    let last_reply = units
        .iter()
        .rposition(|u| parts[u[0]].voice == Voice::Theseus);
    let mut keep: Vec<bool> = units
        .iter()
        .enumerate()
        .map(|(k, u)| {
            matches!(parts[u[0]].voice, Voice::Person | Voice::Harness) || Some(k) == last_reply
        })
        .collect();
    let size = |keep: &[bool]| tokens_of(runs(&units, keep, &bytes, handle).1);
    let needed = size(&keep);
    if needed > max {
        return Err(Overflow {
            handle: handle.clone(),
            needed,
            max,
        });
    }
    let optional: Vec<usize> = (0..keep.len()).filter(|&k| !keep[k]).collect();
    for k in optional {
        keep[k] = true;
        keep[k] = size(&keep) <= max;
    }
    let (runs, _) = runs(&units, &keep, &bytes, handle);
    Ok(runs
        .into_iter()
        .flat_map(|r| match r {
            Run::Kept(k) => (units[k].iter())
                .map(|&i| (parts[i].clone(), Some((i, 0..parts[i].text.len()))))
                .collect(),
            Run::Cut(m) => vec![(m, None)],
        })
        .collect())
}

/// [`try_fit`], for a caller that takes parts whatever happens: over its maximum, one mark that says so
/// instead (`[too long to show here: about 12,000 tokens, over the 3,000 allowed; open <handle> for the whole
/// exchange]`), never the words silently over.
pub fn fit(parts: Vec<Said>, max: u64, handle: &Handle) -> Vec<Said> {
    (fit_kept(parts, max, handle).into_iter())
        .map(|(p, _)| p)
        .collect()
}

/// [`fit`], each part it gives with what it kept ([`Kept`]), or none for a mark.
pub fn fit_kept(parts: Vec<Said>, max: u64, handle: &Handle) -> Vec<(Said, Option<Kept>)> {
    try_fit_kept(parts, max, handle).unwrap_or_else(|o| {
        vec![(
            mark_text(format!(
                "[too long to show here: about {} tokens, over the {} allowed; open {} for the whole exchange]",
                crate::narrative::thousands(o.needed),
                crate::narrative::thousands(o.max),
                o.handle.0
            )),
            None,
        )]
    })
}

/// The tokens of a text of `bytes` bytes, as [`super::tokens`] counts them.
fn tokens_of(bytes: u64) -> u64 {
    (bytes as f64 / crate::catalog::TokenRates::CLAUDE.text).ceil() as u64
}

/// A kept unit, or the mark for a run of cut ones.
enum Run {
    Kept(usize),
    Cut(Said),
}

/// The units as kept or cut, a mark for each run of cut ones, and the bytes of their [`text`] (each part's,
/// each mark's, and the newline between parts).
fn runs(units: &[Vec<usize>], keep: &[bool], bytes: &[u64], handle: &Handle) -> (Vec<Run>, u64) {
    let mut out = Vec::new();
    let (mut total, mut lines) = (0u64, 0u64);
    let mut cut: (usize, u64) = (0, 0);
    let flush = |cut: &mut (usize, u64), out: &mut Vec<Run>, total: &mut u64, lines: &mut u64| {
        if cut.0 > 0 {
            let m = mark(*cut, handle);
            *total += m.text.len() as u64;
            *lines += 1;
            out.push(Run::Cut(m));
            *cut = (0, 0);
        }
    };
    for (k, u) in units.iter().enumerate() {
        let unit_bytes: u64 = u.iter().map(|&i| bytes[i]).sum();
        if !keep[k] {
            cut = (cut.0 + u.len(), cut.1 + tokens_of(unit_bytes));
            continue;
        }
        flush(&mut cut, &mut out, &mut total, &mut lines);
        total += unit_bytes;
        lines += u.len() as u64;
        out.push(Run::Kept(k));
    }
    flush(&mut cut, &mut out, &mut total, &mut lines);
    (out, total + lines.saturating_sub(1))
}

fn mark((n, tokens): (usize, u64), handle: &Handle) -> Said {
    let parts = crate::narrative::count(n as u64, "part", "parts");
    mark_text(format!(
        "[cut here: {parts}, about {} tokens; open {} for the whole exchange]",
        crate::narrative::thousands(tokens),
        handle.0
    ))
}

fn mark_text(text: String) -> Said {
    Said {
        voice: Voice::Mark,
        text,
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
    if matches!(part.voice, Voice::Person | Voice::Harness) || s.len() <= h.saturating_add(t) {
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
