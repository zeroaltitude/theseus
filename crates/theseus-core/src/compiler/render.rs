//! The render (moved out of `compiler.rs` at theseus-0l3g): nodes to a
//! request's messages. It runs as a state that nodes are rendered into in
//! order (`State`), so a session's render can be kept and taken up again
//! where it stopped (`kept`), byte for byte what one render of every node
//! makes.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};

use super::{clearing, compaction};
use crate::attach::Media;
use crate::config::Effort;
use crate::node::{Body, Node};
use crate::provider::tool_uses_in;

pub(super) fn is_thinking(b: &Value) -> bool {
    matches!(
        b.get("type").and_then(Value::as_str),
        Some("thinking") | Some("redacted_thinking")
    )
}

fn tool_result_block(r: &Node, media: &Media, spend: &mut crate::attach::Spend) -> Value {
    match &r.body {
        Body::ToolResult {
            tool_use_id,
            content,
            is_error,
            image,
            ..
        } => {
            if *is_error {
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": true})
            } else if let Some(img) = image {
                // An image the tool returned (theseus-9g2), or a PDF's pages
                // (theseus-c9l6).
                let content = crate::attach::tool_content(content, img, media, spend);
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content})
            } else {
                json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content})
            }
        }
        _ => json!({}),
    }
}

/// A background job's real result, delivered as text after its placeholder
/// was already answered.
fn late_result_text(r: &Node) -> String {
    match &r.body {
        Body::ToolResult {
            tool_use_id,
            tool,
            status,
            content,
            meta,
            ..
        } => {
            let exit = meta
                .get("exit_code")
                .and_then(Value::as_i64)
                .map(|c| format!(", exit code {c}"))
                .unwrap_or_default();
            format!(
                "[Background result for your earlier {tool} call ({tool_use_id}): status {}{exit}]\n{content}",
                status.as_str()
            )
        }
        _ => String::new(),
    }
}

/// What a render has made so far: the messages (by role, with their
/// blocks; the last may still take more blocks of its role), the calls given
/// a synthetic result, what the images and PDFs use, the first message of
/// the tail, and each answer's recorded effort.
#[derive(Debug, Clone, Default)]
pub(super) struct State {
    pub out: Vec<(String, Vec<Value>)>,
    pub repairs: Vec<String>,
    /// What the request's images and PDFs use, in render order (theseus-c9l6).
    pub spend: crate::attach::Spend,
    pub tail_from: Option<usize>,
    pub efforts: Vec<(usize, Effort)>,
}

/// What every node of a render reads besides itself.
pub(super) struct Ctx<'a, 'n> {
    /// Each call's result.
    pub results: &'a HashMap<&'n str, &'n Node>,
    /// The answers a later call replaced.
    pub replaced: &'a HashSet<&'n str>,
    pub strip_prefix_thinking: bool,
    pub provider: &'a str,
    pub media: &'a Media<'a>,
    pub sources: &'a crate::recall::render::Sources,
    pub cleared: &'a HashSet<&'a str>,
}

/// Each call's result among `nodes`: its last one that is not late.
pub(super) fn results_of<'n>(nodes: impl Iterator<Item = &'n Node>) -> HashMap<&'n str, &'n Node> {
    let mut results = HashMap::new();
    for n in nodes {
        if let Body::ToolResult {
            tool_use_id,
            late: false,
            ..
        } = &n.body
        {
            results.insert(tool_use_id.as_str(), n);
        }
    }
    results
}

/// Nodes → provider messages. Tool results are placed in one user message
/// right after the assistant message whose `tool_use` blocks they answer,
/// `tool_result` blocks first; consecutive same-role messages merge. An
/// assistant message whose provider is not `provider`, the request's, or
/// whose model is not the request's, keeps no thinking block: its signature
/// is one only its own provider and model can verify (theseus-kol; 25e). An answer cut at the window that a later call replaced is
/// left out, with its calls (theseus-9p88): the one `retrying` names, and
/// any that a later answer of its turn followed. Also returns the repaired
/// calls and the tokens the images are estimated at.
pub fn render_messages(
    prefix: &[&Node],
    tail: &[&Node],
    strip_prefix_thinking: bool,
    provider: &str,
    media: &Media,
    retrying: Option<&str>,
    (sources, cleared): (&crate::recall::render::Sources, &HashSet<&str>),
) -> Messages {
    let all = || prefix.iter().chain(tail.iter()).copied();
    let results = results_of(all());
    let replaced = replaced_answers(all(), retrying);
    let ctx = Ctx {
        results: &results,
        replaced: &replaced,
        strip_prefix_thinking,
        provider,
        media,
        sources,
        cleared,
    };
    let mut st = State::default();
    let items = prefix
        .iter()
        .map(|n| (*n, true))
        .chain(tail.iter().map(|n| (*n, false)));
    for (n, in_prefix) in items {
        st.item(n, in_prefix, &ctx);
    }
    st.finish()
}

impl State {
    /// Render one node into the state.
    pub(super) fn item(&mut self, n: &Node, in_prefix: bool, ctx: &Ctx<'_, '_>) {
        let (out, media) = (&mut self.out, ctx.media);
        if !in_prefix && self.tail_from.is_none() {
            self.tail_from = Some(out.len());
        }
        match &n.body {
            Body::UserMessage { text, attachments } => {
                // A message of attachments alone has no text block; one
                // without attachments renders as it always did.
                let blocks = user_blocks(n, text, attachments, media, &mut self.spend);
                push(out, "user", blocks)
            }
            // A task's arrangement reads right after its brief, as the
            // same user message (M5 27); a check's claim after its pieces (28a).
            Body::Arrangement { pieces, claim, .. } => push(
                out,
                "user",
                vec![json!({"type": "text", "text": crate::check::render(pieces, claim.as_ref())})],
            ),
            Body::AssistantMessage { .. } if ctx.replaced.contains(n.id.as_str()) => {}
            Body::AssistantMessage {
                blocks,
                provider: wrote,
                model: wrote_model,
                effort,
                ..
            } => {
                // A signature is the writing model's: a detour's answer on
                // another model (M5 25e) carries none back either.
                let strip = (in_prefix && ctx.strip_prefix_thinking)
                    || wrote != ctx.provider
                    || (wrote_model != media.model && media.also != Some(wrote_model));
                let bl: Vec<Value> = if strip {
                    blocks.iter().filter(|b| !is_thinking(b)).cloned().collect()
                } else {
                    blocks.clone()
                };
                // The provider's own calls go back whole and in order, or its
                // API refuses the request (theseus-s0s4x).
                let bl = super::server_calls::closed(bl);
                if bl.is_empty() {
                    return;
                }
                let uses = tool_uses_in(&bl);
                self.efforts.extend(effort.map(|e| (out.len(), e)));
                push(out, "assistant", bl);
                if !uses.is_empty() {
                    let rb: Vec<Value> = uses
                        .iter()
                        .map(|u| match ctx.results.get(u.id.as_str()) {
                            // An old result cleared to save context (theseus-3ki0).
                            Some(r) if ctx.cleared.contains(r.id.as_str()) => {
                                clearing::block(&u.name, &u.input, r)
                            }
                            Some(r) => tool_result_block(r, media, &mut self.spend),
                            None => {
                                self.repairs.push(u.id.clone());
                                repaired(&u.id)
                            }
                        })
                        .collect();
                    push(out, "user", rb);
                }
            }
            Body::ToolResult { late: true, .. } => {
                let line = late_result_text(n);
                push(out, "user", vec![json!({"type": "text", "text": line})])
            }
            // Recall's testimony, after the new message in its user turn
            // (M6 30b), read from its sources over their frozen ranges.
            Body::Recall { items, .. } => {
                let text = crate::recall::render::render(items, ctx.sources);
                push(out, "user", vec![json!({"type": "text", "text": text})])
            }
            // A compaction's summary (30c), first in its prefix; in a tail
            // its range is still in the prefix, so it renders nothing.
            Body::Summary { header, text, .. } if in_prefix => {
                let text = compaction::rendered(header, text);
                push(out, "user", vec![json!({"type": "text", "text": text})])
            }
            // A moved range's one line (theseus-q9b85): where the range began in
            // a recompile (`moved::in_place`), and at its own place in a tail.
            Body::Moved {
                undoes: None,
                quiet: false,
                thread,
                gist,
                ..
            } => push(
                out,
                "user",
                vec![json!({"type": "text", "text": crate::moved::line(thread, gist)})],
            ),
            _ => {}
        }
    }

    /// The messages the state holds.
    pub(super) fn finish(self) -> Messages {
        let out_len = self.out.len();
        let messages = self
            .out
            .into_iter()
            .map(|(role, content)| json!({"role": role, "content": content}))
            .collect();
        Messages {
            messages,
            repairs: self.repairs,
            image_tokens: self.spend.tokens,
            tail_from: self.tail_from.unwrap_or(out_len),
            efforts: self.efforts,
        }
    }
}

/// Add a message's blocks: consecutive same-role messages merge.
fn push(out: &mut Vec<(String, Vec<Value>)>, role: &str, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    match out.last_mut() {
        Some((r, b)) if r == role => b.extend(blocks),
        _ => out.push((role.to_string(), blocks)),
    }
}

/// The synthetic result of a call that has none recorded.
fn repaired(id: &str) -> Value {
    json!({
        "type": "tool_result",
        "tool_use_id": id,
        "content": "No result was recorded for this call (the harness was interrupted before it ran).",
        "is_error": true,
    })
}

/// `render_messages`' output: the messages, the calls given a synthetic
/// result, and the tokens its images are estimated at.
pub struct Messages {
    pub messages: Vec<Value>,
    pub repairs: Vec<String>,
    pub image_tokens: u64,
    /// The first message that carries a tail node (`messages.len()` when
    /// none does).
    pub tail_from: usize,
    /// Each answer's recorded effort, at its message's index (`effort::place`).
    pub efforts: Vec<(usize, Effort)>,
}

/// An operator's message's blocks: each attachment its own (an image or a
/// PDF, two), then the typed text (theseus-9g2, theseus-c9l6).
fn user_blocks(
    n: &Node,
    text: &str,
    attachments: &[crate::node::Attachment],
    media: &Media,
    spend: &mut crate::attach::Spend,
) -> Vec<Value> {
    let mut blocks: Vec<Value> = attachments
        .iter()
        .flat_map(|a| crate::attach::blocks(a, n.author.as_deref(), media, spend))
        .collect();
    if !text.is_empty() || attachments.is_empty() {
        blocks.push(json!({"type": "text", "text": text}));
    }
    crate::surface::lead(n, blocks)
}

/// The answers cut at the window (theseus-9p88), or refused (theseus-7gir.18),
/// that a later call replaced: `retrying`, the one the request being rendered
/// replaces, and each that a later answer of its own turn followed. A turn
/// makes that call only to replace it, so the rule needs no record of its own.
fn replaced_answers<'n>(
    nodes: impl Iterator<Item = &'n Node>,
    retrying: Option<&'n str>,
) -> HashSet<&'n str> {
    cut_answers(nodes, retrying).0
}

/// `replaced_answers`, and whether a cut answer is still open at the end: an
/// answer a later one of its turn would replace (theseus-0l3g: a render that
/// leaves one open is not kept for the next).
pub(super) fn cut_answers<'n>(
    nodes: impl Iterator<Item = &'n Node>,
    retrying: Option<&'n str>,
) -> (HashSet<&'n str>, bool) {
    let mut replaced: HashSet<&str> = retrying.into_iter().collect();
    // Each turn's cut answers not yet followed by a later answer of it.
    let mut open: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in nodes {
        let Body::AssistantMessage { stop_reason, .. } = &n.body else {
            continue;
        };
        let Some(turn) = n.turn_id.as_deref() else {
            continue;
        };
        if let Some(cut) = open.remove(turn) {
            replaced.extend(cut);
        }
        let stop = stop_reason.as_deref().unwrap_or_default();
        if stop == crate::provider::WINDOW_EXCEEDED || stop == "refusal" {
            open.entry(turn).or_default().push(n.id.as_str());
        }
    }
    (replaced, !open.is_empty())
}
