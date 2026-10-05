//! The arrangement on `task.create` (M5 step 27, theseus-vug.2; M5 §2.10).
//! Promoting work to a task needs an authored arrangement (decision 6,
//! theseus-vmh): the model quotes the messages that define the work, rather
//! than paraphrasing them into the brief, and the child reads them verbatim.
//!
//! - **The input.** `arrangement: { pieces: [{quote, role} | {node, role}],
//!   trust?: [i], supersedes?: [[older, newer]] }` and `fidelity_ack?`, each
//!   index into `pieces`, from 0. A role is `objective`, `acceptance`,
//!   `design`, or `context`. Without an arrangement, or with no admitted
//!   `objective` or `design` piece, the call is refused (`REFUSAL`), as invalid
//!   input, before the gate.
//! - **Resolution** (`resolve`). A quote matches a node of the calling
//!   session's own transcript (the place rule: the child sees nothing its
//!   parent could not): a user message's text, a reply's text, or a tool
//!   result's content. Matching is exact and case-sensitive, with every run
//!   of whitespace, in the quote and in the node, read as one space, and the
//!   quote's ends trimmed. It needs at least `MIN_QUOTE_CHARS` characters so
//!   read, and exactly one node holding it: otherwise the call fails and says
//!   why (no match; ambiguous, with each candidate's id, author, and time;
//!   too short, naming any node whose whole text it is), and the model tries
//!   again. A `{node}` piece names a node of the same transcript by its id.
//!   The reply that holds the call is never a source: a piece is what someone
//!   said before the call, not the call's own words; nor is an earlier
//!   `task.create`'s result, which echoes its pieces' first lines.
//! - **The fidelity check** (`fidelity`). A brief under
//!   `FIDELITY_BRIEF_CHARS` characters, from a session with more than
//!   `FIDELITY_HUMAN_MESSAGES` operator messages since its last task, with a
//!   single admitted piece, fails unless the call says `fidelity_ack: true`.
//! - **The node.** The pieces go into the child's session as one
//!   `Body::Arrangement` node, written after the brief in the frame that
//!   opens the task, with a `derived_from` edge to each piece's node
//!   (`VIA_ARRANGEMENT`). It carries each admitted piece's full text (capped
//!   at `PIECE_CHARS`), author, time, and session, so the compiler renders it
//!   from the child's own nodes (`render`); a superseded piece carries no
//!   text, and renders by reference only.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::node::{Body, Node, Origin, ResultStatus};

/// The refusal without an arrangement (M5 §2.10).
pub const REFUSAL: &str =
    "Promotion needs an arrangement: quote the messages that define this work.";
/// The shortest quote, in characters, with whitespace runs read as one.
pub const MIN_QUOTE_CHARS: usize = 20;
/// The most pieces an arrangement takes.
pub const MAX_PIECES: usize = 12;
/// The most of one piece's text the child reads, in characters.
pub const PIECE_CHARS: usize = 16_000;
/// The fidelity check: a brief under this many characters...
pub const FIDELITY_BRIEF_CHARS: usize = 200;
/// ...from a session with more than this many operator messages since its
/// last task, with a single piece.
pub const FIDELITY_HUMAN_MESSAGES: usize = 10;

/// What a piece is to the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// What to do.
    Objective,
    /// How to know it is done.
    Acceptance,
    /// How it was decided it should be done.
    Design,
    /// Anything else the task should know.
    Context,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Objective => "objective",
            Role::Acceptance => "acceptance",
            Role::Design => "design",
            Role::Context => "context",
        }
    }
}

/// One piece as the call names it: a quote, or a node's id.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceRef {
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    pub role: Role,
}

/// `task.create`'s `arrangement`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub pieces: Vec<PieceRef>,
    /// Pieces the child reads as trusted testimony.
    #[serde(default)]
    pub trust: Vec<usize>,
    /// `[older, newer]`: the older piece is shown by reference only.
    #[serde(default)]
    pub supersedes: Vec<[usize; 2]>,
}

impl Input {
    /// What supersedes each superseded piece: the first pair that names it.
    pub fn superseded(&self) -> BTreeMap<usize, usize> {
        let mut out = BTreeMap::new();
        for [older, newer] in &self.supersedes {
            out.entry(*older).or_insert(*newer);
        }
        out
    }

    /// Check its shape, before anything is read: the refusal without an
    /// admitted `objective` or `design` piece, and every index in range.
    pub fn check(&self) -> Result<(), String> {
        let n = self.pieces.len();
        if n == 0 {
            return Err(format!("{REFUSAL} `arrangement.pieces` is empty."));
        }
        if n > MAX_PIECES {
            return Err(format!(
                "the arrangement has {n} pieces, over the {MAX_PIECES} a task takes: quote the \
                 messages that define the work, not the whole conversation"
            ));
        }
        for (i, p) in self.pieces.iter().enumerate() {
            match (&p.quote, &p.node) {
                (Some(_), Some(_)) => {
                    return Err(format!("piece {i} has both `quote` and `node`: give one"))
                }
                (None, None) => {
                    return Err(format!(
                        "piece {i} has neither `quote` nor `node`: quote the message exactly"
                    ))
                }
                _ => {}
            }
        }
        let bad = |i: usize| i >= n;
        if let Some(i) = self.trust.iter().find(|i| bad(**i)) {
            return Err(format!(
                "`trust` names piece {i}, and there are {n} (from 0)"
            ));
        }
        for [older, newer] in &self.supersedes {
            if bad(*older) || bad(*newer) {
                return Err(format!(
                    "`supersedes` names [{older}, {newer}], and there are {n} pieces (from 0)"
                ));
            }
            if older == newer {
                return Err(format!("`supersedes` says piece {older} supersedes itself"));
            }
        }
        let superseded = self.superseded();
        let defines = self.pieces.iter().enumerate().any(|(i, p)| {
            !superseded.contains_key(&i) && matches!(p.role, Role::Objective | Role::Design)
        });
        if !defines {
            return Err(format!(
                "{REFUSAL} None of its pieces that stand (not superseded) has the role \
                 `objective` or `design`."
            ));
        }
        Ok(())
    }
}

/// A piece as the arrangement node keeps it: what the child's first
/// compilation renders.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    pub role: Role,
    /// The node it names, and that node's session (the parent's).
    pub node: String,
    pub session_id: String,
    pub origin: Origin,
    #[serde(default)]
    pub author: Option<String>,
    pub at_ms: u64,
    /// The node's full text, capped at `PIECE_CHARS`; none for a superseded
    /// piece, which the child sees by reference only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The node's first line, for the task's surfaces.
    #[serde(default)]
    pub first_line: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub trusted: bool,
    /// The index of the piece that supersedes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<u32>,
}

/// A refused arrangement: the class the ledger counts, and what the model
/// reads.
#[derive(Debug, Clone)]
pub struct Refused {
    /// `no_match`, `ambiguous`, `short`, `unknown_node`, `same_node`, or
    /// `fidelity`.
    pub class: &'static str,
    pub message: String,
}

/// Whitespace runs as one space, the ends trimmed: how quotes match.
pub fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A node's text, as a quote matches it and the child reads it: an
/// operator's or relayed message's text, a reply's text blocks, a tool
/// result's content. None for a node with none (a call, an arrangement, a
/// recall, whose text is its sources', a summary, which stands for its range).
pub fn text_of(n: &Node) -> Option<String> {
    let t = match &n.body {
        Body::UserMessage { text, .. } => text.clone(),
        Body::AssistantMessage { blocks, .. } => crate::provider::text_of(blocks),
        Body::ToolResult { content, .. } => content.clone(),
        Body::ToolCall { .. }
        | Body::Arrangement { .. }
        | Body::Recall { .. }
        | Body::Summary { .. }
        | Body::Synthesis { .. } => return None,
    };
    (!t.trim().is_empty()).then_some(t)
}

/// A node that only echoes others: an earlier `task.create`'s result,
/// which lists the first lines of the pieces it quoted.
fn echoes(n: &Node) -> bool {
    matches!(&n.body, Body::ToolResult { tool, .. } if tool == crate::task::CREATE)
}

/// Unix milliseconds as `2026-10-04 08:12 UTC`.
pub fn utc(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let (y, mo, d) = crate::wake::civil_from_days(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{mo:02}-{d:02} {:02}:{:02} UTC",
        t / 3600,
        t / 60 % 60
    )
}

/// Who wrote a node, in words.
fn who(origin: Origin, author: Option<&str>) -> String {
    match (origin, author) {
        (Origin::Operator, Some(a)) => format!("the operator ({a})"),
        (Origin::Operator, None) => "the operator".into(),
        (Origin::Agent, _) => "the model".into(),
        (Origin::Tool, _) => "a tool's result".into(),
        (Origin::Harness, Some(a)) => format!("the harness ({a})"),
        (Origin::Harness, None) => "the harness".into(),
        (Origin::Mcp, Some(a)) => format!("an MCP prompt ({a})"),
        (Origin::Mcp, None) => "an MCP prompt".into(),
    }
}

fn origin_str(o: Origin) -> &'static str {
    match o {
        Origin::Operator => "operator",
        Origin::Agent => "agent",
        Origin::Tool => "tool",
        Origin::Harness => "harness",
        Origin::Mcp => "mcp",
    }
}

/// A candidate's line in a refusal: its id, who wrote it, and when.
fn candidate(n: &Node) -> String {
    format!(
        "{} ({}, {})",
        n.id,
        who(n.origin, n.author.as_deref()),
        utc(n.created_at_ms)
    )
}

/// Resolve the arrangement's pieces against `nodes`, the calling session's
/// transcript, leaving out `holder`, the reply that holds the call.
pub fn resolve(
    input: &Input,
    nodes: &[(u64, Arc<Node>)],
    holder: Option<&str>,
) -> Result<Vec<Piece>, Refused> {
    let sources: Vec<(&Node, String)> = nodes
        .iter()
        .map(|(_, n)| &**n)
        .filter(|n| Some(n.id.as_str()) != holder && !echoes(n))
        .filter_map(|n| text_of(n).map(|t| (n, t)))
        .collect();
    let trusted: BTreeSet<usize> = input.trust.iter().copied().collect();
    let superseded = input.superseded();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = Vec::with_capacity(input.pieces.len());
    for (i, p) in input.pieces.iter().enumerate() {
        let (node, text) = match (&p.quote, &p.node) {
            (Some(q), _) => find(i, q, &sources)?,
            (None, Some(id)) => sources
                .iter()
                .find(|(n, _)| &n.id == id)
                .map(|(n, t)| (*n, t.clone()))
                .ok_or_else(|| Refused {
                    class: "unknown_node",
                    message: format!(
                        "piece {i} names node {id}, which is not a message of this session with \
                         text: quote the message instead"
                    ),
                })?,
            (None, None) => unreachable!("checked"),
        };
        if let Some(j) = seen.insert(node.id.clone(), i) {
            return Err(Refused {
                class: "same_node",
                message: format!(
                    "pieces {j} and {i} both resolve to {}: one piece per message, with the \
                     role that matters most",
                    candidate(node)
                ),
            });
        }
        let superseded_by = superseded.get(&i).map(|n| *n as u32);
        let body = superseded_by.is_none().then(|| {
            let (t, _) = crate::toolrun::cap(&text, PIECE_CHARS, |_| {
                format!(
                    "the whole message is node {} of session {}",
                    node.id, node.session_id
                )
            });
            t
        });
        out.push(Piece {
            role: p.role,
            node: node.id.clone(),
            session_id: node.session_id.clone(),
            origin: node.origin,
            author: node.author.clone(),
            at_ms: node.created_at_ms,
            text: body,
            first_line: crate::session::title_from(&text),
            trusted: trusted.contains(&i),
            superseded_by,
        });
    }
    Ok(out)
}

/// The one node a quote names.
fn find<'n>(
    i: usize,
    quote: &str,
    sources: &[(&'n Node, String)],
) -> Result<(&'n Node, String), Refused> {
    let q = normalize(quote);
    let shown: String = q.chars().take(60).collect();
    let shown = if q.chars().count() > 60 {
        format!("{shown}…")
    } else {
        shown
    };
    let chars = q.chars().count();
    if chars < MIN_QUOTE_CHARS {
        let whole: Vec<String> = sources
            .iter()
            .filter(|(_, t)| normalize(t) == q)
            .map(|(n, _)| candidate(n))
            .collect();
        let hint = if whole.is_empty() {
            "quote a longer span of the message".to_string()
        } else {
            format!(
                "it is the whole of {}: name it as {{\"node\": \"<id>\", \"role\": …}} instead",
                whole.join("; ")
            )
        };
        return Err(Refused {
            class: "short",
            message: format!(
                "piece {i}'s quote \"{shown}\" is {chars} characters, under the \
                 {MIN_QUOTE_CHARS} a quote needs: {hint}"
            ),
        });
    }
    let hits: Vec<&(&Node, String)> = sources
        .iter()
        .filter(|(_, t)| normalize(t).contains(&q))
        .collect();
    match hits.as_slice() {
        [(n, t)] => Ok((*n, t.clone())),
        [] => Err(Refused {
            class: "no_match",
            message: format!(
                "piece {i}'s quote \"{shown}\" matches no message of this session. Quote it \
                 exactly as it was written, character for character (whitespace runs count as \
                 one space); a quote is never a paraphrase, and your own reply that makes this \
                 call is not a source"
            ),
        }),
        many => Err(Refused {
            class: "ambiguous",
            message: format!(
                "piece {i}'s quote \"{shown}\" matches {} messages: {}. Quote a longer span that \
                 only one of them holds, or name one as {{\"node\": \"<id>\", \"role\": …}}",
                many.len(),
                many.iter()
                    .map(|(n, _)| candidate(n))
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }),
    }
}

/// Operator messages in `nodes` since the session's last task started: after
/// its last `task.create` whose result was ok.
pub fn human_messages_since_last_task(nodes: &[(u64, Arc<Node>)]) -> usize {
    nodes
        .iter()
        .rev()
        .take_while(|(_, n)| {
            !matches!(&n.body, Body::ToolResult { tool, status: ResultStatus::Ok, .. }
                if tool == crate::task::CREATE)
        })
        .filter(|(_, n)| n.origin == Origin::Operator && matches!(n.body, Body::UserMessage { .. }))
        .count()
}

/// The fidelity check: a one-line brief drawn from a long discussion, with a
/// single admitted piece, fails without the ack.
pub fn fidelity(brief: &str, humans: usize, pieces: &[Piece], ack: bool) -> Result<(), Refused> {
    let chars = brief.trim().chars().count();
    let admitted = pieces.iter().filter(|p| p.superseded_by.is_none()).count();
    if ack || chars >= FIDELITY_BRIEF_CHARS || humans <= FIDELITY_HUMAN_MESSAGES || admitted != 1 {
        return Ok(());
    }
    Err(Refused {
        class: "fidelity",
        message: format!(
            "Not started (the fidelity check): the brief is {chars} characters, drawn from {humans} \
             messages of the person's since this session's last task, with a single piece, so \
             the task would see little of what was decided. Attach the design: quote the \
             messages that settled how it should be done (role `design`) and how to know it is \
             done (`acceptance`). If the one piece really carries the whole work, call again \
             with `fidelity_ack: true`."
        ),
    })
}

/// The arrangement node's text, as the child's compilation renders it after
/// the brief: each admitted piece's full text with its author, time, and
/// session; a superseded piece by reference only.
pub fn render(pieces: &[Piece]) -> String {
    let n = pieces.len();
    let mut out = format!(
        "[Arrangement: the conversation that started you quoted {} that define this work. Each \
         admitted piece below is a message's full original text, verbatim: testimony of what \
         was said, where the brief is your parent's reading of it. A piece marked trusted \
         testimony was vouched for by your parent: take it as settled. A superseded piece is \
         named only; a later piece replaces it.]",
        crate::narrative::count(n as u64, "message", "messages")
    );
    for (i, p) in pieces.iter().enumerate() {
        let from = format!(
            "{} in session {}, {}",
            who(p.origin, p.author.as_deref()),
            crate::narrative::short(&p.session_id),
            utc(p.at_ms)
        );
        let trusted = if p.trusted { ", trusted testimony" } else { "" };
        out.push_str("\n\n");
        match (&p.text, p.superseded_by) {
            (Some(text), None) => {
                out.push_str(&format!(
                    "--- Piece {} of {n} ({}{trusted}): from {from} ---\n{text}",
                    i + 1,
                    p.role.as_str()
                ));
            }
            (_, by) => {
                let by = by.map_or(String::new(), |b| format!(" by piece {}", b + 1));
                out.push_str(&format!(
                    "--- Piece {} of {n} ({}{trusted}): superseded{by}. It was node {}, from \
                     {from}; its text is not shown here, and stays in that session. ---",
                    i + 1,
                    p.role.as_str(),
                    p.node
                ));
            }
        }
    }
    out
}

/// The task's surfaces' view of an arrangement node.
pub fn info(node: &Node) -> Option<theseus_protocol::TaskArrangement> {
    let Body::Arrangement {
        pieces,
        fidelity_ack,
        ..
    } = &node.body
    else {
        return None;
    };
    Some(theseus_protocol::TaskArrangement {
        node_id: node.id.clone(),
        fidelity_ack: *fidelity_ack,
        pieces: pieces
            .iter()
            .enumerate()
            .map(|(i, p)| theseus_protocol::ArrangementPiece {
                index: i as u32,
                role: p.role.as_str().into(),
                node_id: p.node.clone(),
                session_id: p.session_id.clone(),
                origin: origin_str(p.origin).into(),
                author: p.author.clone(),
                at_ms: p.at_ms,
                first_line: p.first_line.clone(),
                trusted: p.trusted,
                superseded_by: p.superseded_by,
            })
            .collect(),
    })
}

/// The result's list of what resolved: each piece's node, author, time, and
/// first line.
pub fn resolved_lines(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut marks = String::new();
            if p.trusted {
                marks.push_str(", trusted");
            }
            if let Some(b) = p.superseded_by {
                marks.push_str(&format!(", superseded by piece {b}"));
            }
            format!(
                "- piece {i} ({}{marks}): {} by {}, {}: \"{}\"",
                p.role.as_str(),
                p.node,
                who(p.origin, p.author.as_deref()),
                utc(p.at_ms),
                p.first_line
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The result's meta: each piece by reference.
pub fn meta(pieces: &[Piece]) -> Value {
    Value::Array(
        pieces
            .iter()
            .enumerate()
            .map(|(i, p)| {
                json!({
                    "index": i,
                    "role": p.role.as_str(),
                    "node": p.node,
                    "author": p.author,
                    "origin": origin_str(p.origin),
                    "at_ms": p.at_ms,
                    "first_line": p.first_line,
                    "trusted": p.trusted,
                    "superseded_by": p.superseded_by,
                })
            })
            .collect(),
    )
}

/// "📎 3 pieces", as the task's lines say it.
pub fn clip(pieces: usize) -> String {
    format!(
        "📎 {}",
        crate::narrative::count(pieces as u64, "piece", "pieces")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(mut n: Node, ms: u64) -> (u64, Arc<Node>) {
        n.created_at_ms = ms;
        (ms, Arc::new(n))
    }

    fn reply(text: &str) -> Node {
        Node::assistant(
            "ses_lighthouse",
            "turn_1",
            0,
            Body::AssistantMessage {
                blocks: vec![json!({"type": "text", "text": text})],
                model: "m".into(),
                provider: "p".into(),
                stop_reason: None,
                usage: Default::default(),
                cost_usd: None,
                catalog_version: None,
                request_id: None,
                correlation_id: None,
                compilation_id: None,
                request_digest: None,
            },
        )
    }

    fn result(tool: &str, content: &str) -> Node {
        Node::tool_result(
            "ses_lighthouse",
            None,
            None,
            Body::ToolResult {
                tool_use_id: "tu_1".into(),
                tool: tool.into(),
                status: ResultStatus::Ok,
                is_error: false,
                content: content.into(),
                correlation_id: None,
                bytes_total: 0,
                truncated: false,
                full_ref: None,
                duration_ms: None,
                late: false,
                meta: Value::Null,
                image: None,
                external: None,
            },
        )
    }

    fn input(v: Value) -> Input {
        serde_json::from_value(v).unwrap()
    }

    fn quoting(quote: &str) -> Input {
        input(json!({"pieces": [{"quote": quote, "role": "objective"}]}))
    }

    /// A short transcript: the keeper's ask, its echo in a reply, and a
    /// file the model read.
    fn transcript() -> Vec<(u64, Arc<Node>)> {
        vec![
            at(
                Node::user(
                    "ses_lighthouse",
                    None,
                    "cli",
                    "Replace the  lamp's\nbulb with the brighter one.\nThen log it.",
                ),
                1_790_000_000_000,
            ),
            at(
                reply("I will replace the lamp's bulb today."),
                1_790_000_060_000,
            ),
            at(
                Node::user(
                    "ses_lighthouse",
                    None,
                    "cli",
                    "Then log it in the keeper's book.",
                ),
                1_790_000_120_000,
            ),
            at(
                result("fs.read", "bulb: 40 W, brighter: 60 W"),
                1_790_000_180_000,
            ),
        ]
    }

    /// Exact and unique, with whitespace runs read as one space, the quote's
    /// ends trimmed, and case kept: the piece carries its node's whole text,
    /// author, time, and session.
    #[test]
    fn a_quote_resolves_to_the_one_node_that_holds_it() {
        let t = transcript();
        let got = resolve(&quoting("  lamp's bulb with the brighter one. "), &t, None).unwrap();
        assert_eq!(got.len(), 1);
        let p = &got[0];
        assert_eq!(p.node, t[0].1.id);
        assert_eq!(p.session_id, "ses_lighthouse");
        assert_eq!(p.origin, Origin::Operator);
        assert_eq!(p.author.as_deref(), Some("cli"));
        assert_eq!(p.at_ms, 1_790_000_000_000);
        assert_eq!(
            p.text.as_deref(),
            Some("Replace the  lamp's\nbulb with the brighter one.\nThen log it.")
        );
        assert_eq!(p.first_line, "Replace the  lamp's");
        // A reply's text, and a tool result's content, resolve too.
        let got = resolve(&quoting("brighter: 60 W, and more"), &t, None);
        assert_eq!(got.unwrap_err().class, "no_match");
        let got = resolve(&quoting("bulb: 40 W, brighter: 60 W"), &t, None).unwrap();
        assert_eq!(got[0].node, t[3].1.id);
        // Case counts.
        let got = resolve(&quoting("REPLACE THE LAMP'S BULB WITH"), &t, None);
        assert_eq!(got.unwrap_err().class, "no_match");
        // A node named by its id.
        let by_id = input(json!({"pieces": [{"node": t[2].1.id, "role": "objective"}]}));
        assert_eq!(resolve(&by_id, &t, None).unwrap()[0].node, t[2].1.id);
        let unknown = input(json!({"pieces": [{"node": "msg_gone", "role": "objective"}]}));
        assert_eq!(
            resolve(&unknown, &t, None).unwrap_err().class,
            "unknown_node"
        );
    }

    /// A quote two nodes hold fails, naming each candidate's id, author, and
    /// time, so the model can quote longer or name one.
    #[test]
    fn an_ambiguous_quote_fails_and_names_its_candidates() {
        let t = transcript();
        let err = resolve(&quoting("replace the lamp's bulb"), &t, None);
        // Case keeps the keeper's capital R out: only the reply holds it.
        assert_eq!(err.unwrap()[0].node, t[1].1.id);
        let err = resolve(&quoting("the lamp's bulb with"), &t, None);
        assert_eq!(err.unwrap()[0].node, t[0].1.id);
        let err = resolve(&quoting("lamp's bulb"), &t, None).unwrap_err();
        assert_eq!(err.class, "short");
        let err = resolve(&quoting("Then log it"), &t, None).unwrap_err();
        assert_eq!(err.class, "short", "{}", err.message);
        let err = resolve(&quoting("the lamp's bulb wi"), &t, None).unwrap_err();
        assert_eq!(err.class, "short");
        let err = resolve(&quoting("place the lamp's bulb"), &t, None).unwrap_err();
        assert_eq!(err.class, "ambiguous", "{}", err.message);
        assert!(
            err.message.contains("matches 2 messages"),
            "{}",
            err.message
        );
        for (n, who) in [(&t[0].1, "the operator (cli)"), (&t[1].1, "the model")] {
            assert!(
                err.message
                    .contains(&format!("{} ({who}, {})", n.id, utc(n.created_at_ms))),
                "{}",
                err.message
            );
        }
    }

    /// A short quote says so, and names a node whose whole text it is.
    #[test]
    fn a_short_quote_fails_and_names_a_node_it_is_the_whole_of() {
        let t = vec![at(
            Node::user("ses_lighthouse", None, "cli", "Do it."),
            1_790_000_000_000,
        )];
        let err = resolve(&quoting("Do  it."), &t, None).unwrap_err();
        assert_eq!(err.class, "short");
        assert!(err.message.contains("is 6 characters"), "{}", err.message);
        assert!(err.message.contains(&t[0].1.id), "{}", err.message);
    }

    /// The reply that holds the call is no source, nor is an earlier
    /// `task.create`'s result; and two pieces of one node are refused.
    #[test]
    fn the_calls_own_reply_and_earlier_results_are_no_source() {
        let mut t = transcript();
        let quote = "I will replace the lamp's bulb today.";
        let holder = t[1].1.id.clone();
        let err = resolve(&quoting(quote), &t, Some(&holder)).unwrap_err();
        assert_eq!(err.class, "no_match");
        t.push(at(
            result(
                crate::task::CREATE,
                "- piece 0 (objective): Replace the  lamp's bulb with",
            ),
            1_790_000_240_000,
        ));
        let got = resolve(&quoting("Replace the lamp's bulb"), &t, None).unwrap();
        assert_eq!(got[0].node, t[0].1.id);
        let twice = input(json!({"pieces": [
            {"quote": "with the brighter one", "role": "objective"},
            {"quote": "Then log it.", "role": "acceptance"}]}));
        let err = resolve(&twice, &t, None).unwrap_err();
        assert_eq!(err.class, "short");
        let twice = input(json!({"pieces": [
            {"quote": "with the brighter one", "role": "objective"},
            {"quote": "bulb with the brighter", "role": "acceptance"}]}));
        assert_eq!(resolve(&twice, &t, None).unwrap_err().class, "same_node");
    }

    /// The refusal without a piece that defines the work, and indexes out of
    /// range.
    #[test]
    fn an_arrangement_needs_a_standing_objective_or_design() {
        let check = |v: Value| input(v).check();
        assert!(check(json!({"pieces": []}))
            .unwrap_err()
            .starts_with(REFUSAL));
        let context = json!({"pieces": [{"quote": "q", "role": "context"}]});
        assert!(check(context).unwrap_err().starts_with(REFUSAL));
        let acceptance = json!({"pieces": [{"quote": "q", "role": "acceptance"}]});
        assert!(check(acceptance).unwrap_err().starts_with(REFUSAL));
        assert!(check(json!({"pieces": [{"quote": "q", "role": "design"}]})).is_ok());
        // A superseded objective does not count.
        let gone = json!({"pieces": [{"quote": "q", "role": "objective"},
            {"quote": "r", "role": "context"}], "supersedes": [[0, 1]]});
        assert!(check(gone).unwrap_err().starts_with(REFUSAL));
        let both = json!({"pieces": [{"quote": "q", "node": "n", "role": "objective"}]});
        assert!(check(both).unwrap_err().contains("both"));
        let trust = json!({"pieces": [{"quote": "q", "role": "objective"}], "trust": [1]});
        assert!(check(trust).unwrap_err().contains("`trust` names piece 1"));
        let selfish = json!({"pieces": [{"quote": "q", "role": "objective"}],
            "supersedes": [[0, 0]]});
        assert!(check(selfish).unwrap_err().contains("supersedes itself"));
        assert!(serde_json::from_value::<Input>(
            json!({"pieces": [{"quote": "q", "role": "wish"}]})
        )
        .is_err());
    }

    /// The fidelity check: each of its three conditions, and the ack.
    #[test]
    fn the_fidelity_check_flags_a_short_brief_from_a_long_talk_with_one_piece() {
        let t = transcript();
        let one = resolve(&quoting("lamp's bulb with the brighter one"), &t, None).unwrap();
        let short = "Swap the bulb.";
        let err = fidelity(short, 11, &one, false).unwrap_err();
        assert_eq!(err.class, "fidelity");
        assert!(
            err.message.contains("`fidelity_ack: true`"),
            "{}",
            err.message
        );
        assert!(fidelity(short, 11, &one, true).is_ok(), "the ack");
        assert!(
            fidelity(short, 10, &one, false).is_ok(),
            "ten is not more than ten"
        );
        assert!(fidelity(&"x".repeat(200), 11, &one, false).is_ok());
        let two = resolve(
            &input(json!({"pieces": [
                {"quote": "lamp's bulb with the brighter one", "role": "objective"},
                {"quote": "Then log it in the keeper's book.", "role": "acceptance"}]})),
            &t,
            None,
        )
        .unwrap();
        assert!(fidelity(short, 11, &two, false).is_ok());
    }

    /// The person's messages since the session's last task: after the last
    /// `task.create` that started one.
    #[test]
    fn human_messages_are_counted_since_the_last_task() {
        let mut t = transcript();
        assert_eq!(human_messages_since_last_task(&t), 2);
        t.push(at(result(crate::task::CREATE, "Started task a1b2c3"), 1));
        assert_eq!(human_messages_since_last_task(&t), 0);
        t.push(at(Node::user("ses_lighthouse", None, "cli", "Next."), 2));
        t.push(at(reply("Noted."), 3));
        assert_eq!(human_messages_since_last_task(&t), 1);
    }

    /// The render: each admitted piece whole with its origin and time, a
    /// trusted one marked, and a superseded one by reference only.
    #[test]
    fn a_superseded_piece_renders_by_reference_only() {
        let t = transcript();
        let a = input(json!({"pieces": [
            {"quote": "lamp's bulb with the brighter one", "role": "objective"},
            {"quote": "bulb: 40 W, brighter: 60 W", "role": "context"},
            {"quote": "Then log it in the keeper's book.", "role": "acceptance"}],
            "trust": [1], "supersedes": [[0, 2]]}));
        let pieces = resolve(&a, &t, None).unwrap();
        assert_eq!(pieces[0].superseded_by, Some(2));
        let out = render(&pieces);
        assert!(!out.contains("brighter one"), "{out}");
        assert!(
            out.contains(&format!(
                "--- Piece 1 of 3 (objective): superseded by piece 3. It was node {}, from the \
             operator (cli) in session …thouse, 2026-09-21 14:13 UTC",
                t[0].1.id
            )),
            "{out}"
        );
        assert!(
            out.contains(
                "--- Piece 2 of 3 (context, trusted testimony): from a tool's result in session \
             …thouse, 2026-09-21 14:16 UTC ---\nbulb: 40 W, brighter: 60 W"
            ),
            "{out}"
        );
        assert!(
            out.contains("--- Piece 3 of 3 (acceptance): from the operator (cli)"),
            "{out}"
        );
        assert!(
            out.ends_with("---\nThen log it in the keeper's book."),
            "{out}"
        );
    }
}
