//! `turn.inspect` (theseus-x9swu): one turn, whole, for the cockpit's
//! inspector. A read: the turn's ledger rows and nodes through the store's
//! tag index, from a little before its `turn.started` row, in one pass; it
//! writes nothing, and no record exists for it. What it reads was written
//! anyway: the assistant nodes hold each loop's content blocks verbatim
//! (thinking included) with their usage and cost, the tool nodes hold the
//! call, its gate record and its result, the `context.compiled` rows hold
//! each loop's compile (digest, sizes, context files), and the `turn.trace`
//! row holds the spans. A request's system text is in no record (a
//! compilation keeps its digest), so the cockpit reads it from
//! `context.explain`, which says whether it is still the bytes the turn saw.
//!
//! Thinking, a prompt, a tool's input and its result go to a private place
//! only (`withheld` says so to any other surface).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use theseus_protocol::inspect::{
    InspectAnswer, InspectCall, InspectLoop, InspectPrompt, InspectRedacted, InspectRow,
    TurnInspectParams, TurnInspectResult,
};
use theseus_protocol::{error_code, ContextCompiled, LedgerKind, Span, Usage};
use theseus_store::pages::{ledger_kind_session, ledger_session};
use theseus_store::{kinds, Page, Store as _};

use super::server::{parse, Conn, RpcFailure};
use super::Core;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node};

/// Positions before the turn's `turn.started` row that are read too: the rows
/// and the input the turn wrote on its way to admission.
const LEAD: u64 = 400;
/// The most rows and nodes of the turn read.
const ROWS: usize = 2000;
const NODES: usize = 3000;
/// The `turn.started` rows read back, newest first, a page at a time, looking
/// for the turn and its neighbours; and the most pages.
const STARTS: usize = 500;
const START_PAGES: usize = 12;
/// The kinds that answer a question.
const ANSWERS: [LedgerKind; 4] = [
    LedgerKind::ActionConfirmAnswered,
    LedgerKind::ActionConfirmed,
    LedgerKind::ActionDeclined,
    LedgerKind::ActionExpired,
];

pub(super) const WITHHELD: &str =
    "thinking, prompts, tool inputs and results go to a private place \
     only (the cockpit, the CLI): this surface reads the shapes, sizes and timings";

impl Core {
    /// `turn.inspect`, off the serving workers.
    pub(super) async fn rpc_inspect(
        self: Arc<Self>,
        params: Value,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let p: TurnInspectParams = parse(params)?;
        let private = conn.surface.reads_private();
        let core = self.clone();
        let r = tokio::task::spawn_blocking(move || core.turn_inspect(&p, private))
            .await
            .map_err(|e| RpcFailure::new(error_code::INTERNAL, e.to_string()))?
            .map_err(RpcFailure::invalid)?;
        serde_json::to_value(r).map_err(|e| RpcFailure::invalid(e.into()))
    }

    /// The session's `turn.started` rows, newest first: (position, turn id).
    fn turn_starts(&self, sid: &str, want: &str) -> Result<Vec<(u64, String)>> {
        let mut out: Vec<(u64, String)> = Vec::new();
        let mut before = None;
        for _ in 0..START_PAGES {
            let page = Page {
                kind: kinds::LEDGER,
                tags: vec![ledger_kind_session(LedgerKind::TurnStarted.as_str(), sid)],
                before,
                limit: STARTS,
                ..Page::default()
            };
            let Some(got) = self.store.inner().page(&page)? else {
                return Err(anyhow!("the index is still being built: try again shortly"));
            };
            let mut older: Vec<(u64, String)> = Vec::new();
            for r in got.records.iter().rev() {
                let row: LedgerRow = r.decode()?;
                if let Some(t) = row.turn_id {
                    older.push((r.position, t));
                }
            }
            let found = older.iter().any(|(_, t)| t == want);
            out.extend(older);
            before = got.first;
            // One start older than the turn is its previous turn.
            if (found && out.last().is_some_and(|(_, t)| t != want)) || !got.more {
                break;
            }
        }
        Ok(out)
    }

    /// `p`'s turn, whole.
    pub(crate) fn turn_inspect(
        &self,
        p: &TurnInspectParams,
        private: bool,
    ) -> Result<TurnInspectResult> {
        let t0 = Instant::now();
        let (sid, tid) = (p.session_id.as_str(), p.turn_id.as_str());
        let session: crate::session::SessionRecord = self
            .store
            .get_session(sid)?
            .ok_or_else(|| anyhow!("no session is named {sid}"))?;
        let starts = self.turn_starts(sid, tid)?;
        let at = starts
            .iter()
            .position(|(_, t)| t == tid)
            .ok_or_else(|| anyhow!("{sid} has no turn {tid} among its newest turns"))?;
        let start_pos = starts[at].0;
        let from = start_pos.saturating_sub(LEAD);
        let prev_turn = starts.get(at + 1).map(|(_, t)| t.clone());

        let mut rows = self.turn_rows(sid, tid, from)?;
        let nodes = self.turn_nodes(sid, tid, from)?;
        self.answers_of(sid, from, &mut rows)?;
        // The next turn: the oldest start after this one; the walk is
        // newest first, so it is the one before in the list.
        let next_turn = at.checked_sub(1).map(|i| starts[i].1.clone());

        let mut out = TurnInspectResult {
            session_id: sid.into(),
            turn_id: tid.into(),
            url: self.cfg.cockpit.turn_link(sid, tid).unwrap_or_default(),
            prev_turn,
            next_turn,
            withheld: (!private).then(|| WITHHELD.to_string()),
            data: session.data.filter(|_| private).map(|d| d.line()),
            ..TurnInspectResult::default()
        };
        assemble(&mut out, rows, nodes, private);
        out.ms = t0.elapsed().as_secs_f64() * 1e3;
        Ok(out)
    }

    /// The turn's ledger rows, oldest first.
    fn turn_rows(&self, sid: &str, tid: &str, from: u64) -> Result<Vec<(u64, LedgerRow)>> {
        let page = Page {
            kind: kinds::LEDGER,
            tags: vec![ledger_session(sid)],
            after: Some(from),
            limit: ROWS,
            ..Page::default()
        };
        let got = self
            .store
            .inner()
            .page(&page)?
            .ok_or_else(|| anyhow!("the index is still being built: try again shortly"))?;
        let mut rows = Vec::new();
        for r in &got.records {
            let row: LedgerRow = r.decode()?;
            if row.turn_id.as_deref() == Some(tid) {
                rows.push((r.position, row));
            }
        }
        Ok(rows)
    }

    /// The answers to the turn's questions, which are written with no turn:
    /// `action.confirm_answered`, `.confirmed`, `.declined` and `.expired`,
    /// found by the correlation id of the question's `tool.confirm_requested`
    /// row, however long after the turn the answer came.
    fn answers_of(&self, sid: &str, from: u64, rows: &mut Vec<(u64, LedgerRow)>) -> Result<()> {
        let asked: Vec<String> = rows
            .iter()
            .filter(|(_, r)| r.kind == LedgerKind::ToolConfirmRequested.as_str())
            .filter_map(|(_, r)| r.data["correlation_id"].as_str().map(str::to_string))
            .collect();
        if asked.is_empty() {
            return Ok(());
        }
        for kind in ANSWERS {
            let page = Page {
                kind: kinds::LEDGER,
                tags: vec![ledger_kind_session(kind.as_str(), sid)],
                after: Some(from),
                limit: ROWS,
                ..Page::default()
            };
            let Some(got) = self.store.inner().page(&page)? else {
                continue;
            };
            for r in &got.records {
                let row: LedgerRow = r.decode()?;
                let ours = row.data["correlation_id"]
                    .as_str()
                    .is_some_and(|c| asked.iter().any(|a| a == c));
                if ours && row.turn_id.is_none() {
                    rows.push((r.position, row));
                }
            }
        }
        rows.sort_by_key(|(p, _)| *p);
        Ok(())
    }

    /// The turn's nodes, oldest first.
    fn turn_nodes(&self, sid: &str, tid: &str, from: u64) -> Result<Vec<(u64, Node)>> {
        let page = Page {
            kind: kinds::NODE,
            tags: vec![ledger_session(sid)],
            after: Some(from),
            limit: NODES,
            ..Page::default()
        };
        let got = self
            .store
            .inner()
            .page(&page)?
            .ok_or_else(|| anyhow!("the index is still being built: try again shortly"))?;
        let mut nodes = Vec::new();
        for r in &got.records {
            let n: Node = r.decode()?;
            if n.turn_id.as_deref() == Some(tid) {
                nodes.push((r.position, n));
            }
        }
        Ok(nodes)
    }
}

/// A block as a surface that reads no private text sees it: its type and its
/// length.
fn shape_of(b: &Value) -> Value {
    let len = |k: &str| {
        b.get(k)
            .and_then(Value::as_str)
            .map_or(0, |s| s.chars().count())
    };
    match b.get("type").and_then(Value::as_str) {
        Some("thinking") => json!({"type": "thinking", "chars": len("thinking")}),
        Some("text") => json!({"type": "text", "chars": len("text")}),
        Some("tool_use") => json!({"type": "tool_use", "id": b["id"], "name": b["name"]}),
        other => json!({"type": other}),
    }
}

/// A span's attributes name commands and paths: a surface that reads no
/// private text gets the spans and their times alone.
fn bare(s: &mut Span) {
    s.attrs = Value::Null;
    s.children.iter_mut().for_each(bare);
}

fn thinking_chars(blocks: &[Value]) -> u64 {
    blocks
        .iter()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("thinking"))
        .map(|b| {
            b.get("thinking")
                .and_then(Value::as_str)
                .map_or(0, |s| s.chars().count() as u64)
        })
        .sum()
}

/// What reading the rows found besides the rows themselves.
struct Read {
    errors: u32,
    compiled: BTreeMap<u32, ContextCompiled>,
    /// Each loop's `request.redacted` rows, in order.
    redacted: BTreeMap<u32, Vec<InspectRedacted>>,
    started_on: Value,
}

/// The turn's rows into the answer: its times, outcome, trace, route and
/// compiles, and every row as the surface may see it.
fn read_rows(out: &mut TurnInspectResult, rows: &[(u64, LedgerRow)], private: bool) -> Read {
    let mut r = Read {
        errors: 0,
        compiled: BTreeMap::new(),
        redacted: BTreeMap::new(),
        started_on: Value::Null,
    };
    for (pos, row) in rows {
        match row.kind.as_str() {
            "turn.started" => {
                out.started_ms = row.at_unix_ms;
                r.started_on = json!({
                    "profile": row.data["profile"], "provider": row.data["provider"],
                    "model": row.data["model"], "reason": "the live profile",
                });
            }
            "turn.ended" => {
                out.outcome = "ended".into();
                out.ended_ms = Some(row.at_unix_ms);
                out.stop_reason = row.data["stop_reason"].as_str().map(str::to_string);
                out.totals.wall_ms = row.data["elapsed_ms"].as_u64().unwrap_or(0);
            }
            "turn.failed" | "turn.refused" if out.outcome.is_empty() => {
                out.outcome = row.kind.trim_start_matches("turn.").into();
                out.ended_ms = Some(row.at_unix_ms);
                r.errors += 1;
            }
            "turn.trace" => {
                out.trace = serde_json::from_value::<Span>(row.data.clone())
                    .ok()
                    .map(|mut t| {
                        if !private {
                            bare(&mut t);
                        }
                        t
                    });
            }
            "route.decided" => out.route = row.data.clone(),
            "context.compiled" => {
                if let Ok(c) = serde_json::from_value::<ContextCompiled>(row.data.clone()) {
                    r.compiled.insert(c.loop_index, c);
                }
            }
            "request.redacted" => {
                // Only a model call's: a token count is no loop's request.
                if let (Some(i), Some("stream")) =
                    (row.data["loop"].as_u64(), row.data["op"].as_str())
                {
                    let names = row.data["names"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|n| n.as_str().map(str::to_string));
                    r.redacted
                        .entry(i as u32)
                        .or_default()
                        .push(InspectRedacted {
                            names: if private { names.collect() } else { vec![] },
                            count: row.data["count"].as_u64().unwrap_or(0),
                            substituted: row.data["substituted"].as_bool().unwrap_or(false),
                            correlation_id: row.data["correlation_id"].as_str().map(str::to_string),
                        });
                }
            }
            "provider.error" | "provider.refusal" | "tool.invalid_input" => r.errors += 1,
            _ => {}
        }
        out.rows.push(InspectRow {
            position: *pos,
            at_ms: row.at_unix_ms,
            kind: row.kind.clone(),
            data: if private {
                row.data.clone()
            } else {
                Value::Null
            },
        });
    }
    r
}

/// The loops as the nodes build them: each answer, call and result in its loop.
#[derive(Default)]
struct Loops {
    loops: BTreeMap<u32, InspectLoop>,
    /// The loop of each answer node, and of each tool use's call: (loop, index among the loop's calls).
    of_answer: BTreeMap<String, u32>,
    by_use: BTreeMap<String, (u32, usize)>,
    /// The ids of the nodes each loop wrote, in order: what the next request adds.
    order: BTreeMap<u32, Vec<String>>,
    errors: u32,
}

impl Loops {
    fn answer(&mut self, out: &mut TurnInspectResult, n: &Node, private: bool) {
        let Body::AssistantMessage {
            blocks,
            model,
            provider,
            stop_reason,
            usage,
            cost_usd,
            request_id,
            compilation_id,
            request_digest,
            effort,
            ..
        } = &n.body
        else {
            return;
        };
        let i = n.loop_index.unwrap_or(0);
        self.of_answer.insert(n.id.clone(), i);
        add(&mut out.totals.usage, usage);
        out.totals.cost_usd += cost_usd.unwrap_or(0.0);
        out.totals.model_calls += 1;
        self.order.entry(i).or_default().push(n.id.clone());
        self.loops.entry(i).or_default().answer = Some(InspectAnswer {
            node_id: n.id.clone(),
            at_ms: n.created_at_ms,
            model: model.clone(),
            provider: provider.clone(),
            stop_reason: stop_reason.clone(),
            usage: usage.clone(),
            cost_usd: *cost_usd,
            request_id: request_id.clone(),
            compilation_id: compilation_id.clone(),
            request_digest: request_digest.clone(),
            effort: effort.as_ref().map(|e| format!("{e:?}").to_lowercase()),
            blocks: if private {
                blocks.clone()
            } else {
                blocks.iter().map(shape_of).collect()
            },
            thinking_chars: thinking_chars(blocks),
            redacted: vec![],
        });
    }

    fn call(&mut self, out: &mut TurnInspectResult, n: &Node, private: bool) {
        let Body::ToolCall {
            tool_use_id,
            tool,
            input,
            assistant_node,
            gate,
            correlation_id,
            ..
        } = &n.body
        else {
            return;
        };
        let i = self
            .of_answer
            .get(assistant_node)
            .copied()
            .or(n.loop_index)
            .unwrap_or(0);
        out.totals.tool_calls += 1;
        self.order.entry(i).or_default().push(n.id.clone());
        let l = self.loops.entry(i).or_default();
        self.by_use.insert(tool_use_id.clone(), (i, l.calls.len()));
        l.calls.push(InspectCall {
            tool_use_id: tool_use_id.clone(),
            tool: tool.clone(),
            call_node: n.id.clone(),
            correlation_id: correlation_id.clone(),
            called_ms: n.created_at_ms,
            input: if private { input.clone() } else { Value::Null },
            gate: if private { gate.clone() } else { None },
            ..InspectCall::default()
        });
    }

    fn result(&mut self, n: &Node, private: bool) {
        let Body::ToolResult {
            tool_use_id,
            status,
            is_error,
            content,
            bytes_total,
            truncated,
            full_ref,
            duration_ms,
            late,
            ..
        } = &n.body
        else {
            return;
        };
        let Some(&(i, k)) = self.by_use.get(tool_use_id) else {
            return;
        };
        self.order.entry(i).or_default().push(n.id.clone());
        // A late result replaces its placeholder in the view; the
        // placeholder stays in the node history.
        let Some(c) = self.loops.get_mut(&i).and_then(|l| l.calls.get_mut(k)) else {
            return;
        };
        c.result_node = Some(n.id.clone());
        c.resulted_ms = Some(n.created_at_ms);
        c.status = Some(format!("{status:?}").to_lowercase());
        c.is_error = *is_error;
        c.content = if private {
            content.clone()
        } else {
            String::new()
        };
        c.bytes_total = *bytes_total;
        c.truncated = *truncated;
        c.full_ref = full_ref.clone();
        c.duration_ms = *duration_ms;
        c.late = *late;
        self.errors += u32::from(*is_error);
    }
}

/// The nodes into the answer: the prompt, and each loop's answer and calls.
fn read_nodes(out: &mut TurnInspectResult, nodes: &[(u64, Node)], private: bool) -> Loops {
    let mut l = Loops::default();
    for (_, n) in nodes {
        match &n.body {
            Body::UserMessage { text, .. } if out.prompt.node_id.is_none() => {
                out.prompt = InspectPrompt {
                    node_id: Some(n.id.clone()),
                    author: n.author.clone(),
                    at_ms: n.created_at_ms,
                    text: if private { text.clone() } else { String::new() },
                    chars: text.chars().count() as u64,
                };
            }
            Body::AssistantMessage { .. } => l.answer(out, n, private),
            Body::ToolCall { .. } => l.call(out, n, private),
            Body::ToolResult { .. } => l.result(n, private),
            _ => {}
        }
    }
    l
}

/// Builds the answer from the turn's rows and nodes (pure, so a test feeds it).
pub(crate) fn assemble(
    out: &mut TurnInspectResult,
    rows: Vec<(u64, LedgerRow)>,
    nodes: Vec<(u64, Node)>,
    private: bool,
) {
    let read = read_rows(out, &rows, private);
    if out.route.is_null() {
        out.route = read.started_on;
    }
    if out.outcome.is_empty() {
        out.outcome = "running".into();
    }
    if out.started_ms == 0 {
        out.started_ms = rows.first().map_or(0, |(_, r)| r.at_unix_ms);
    }
    if out.totals.wall_ms == 0 {
        out.totals.wall_ms = out.ended_ms.map_or(0, |e| e.saturating_sub(out.started_ms));
    }
    let mut l = read_nodes(out, &nodes, private);
    for (i, c) in read.compiled {
        l.loops.entry(i).or_default().compiled = Some(c);
    }
    for (i, redacted) in read.redacted {
        if let Some(a) = l.loops.get_mut(&i).and_then(|lp| lp.answer.as_mut()) {
            a.redacted = redacted;
        }
    }
    let mut prior: Vec<String> = Vec::new();
    for (i, lp) in &mut l.loops {
        lp.loop_index = *i;
        lp.added = std::mem::take(&mut prior);
        prior = l.order.remove(i).unwrap_or_default();
    }
    out.totals.loops = l.loops.len() as u32;
    out.totals.errors = read.errors + l.errors;
    out.loops = l.loops.into_values().collect();
}

fn add(t: &mut Usage, u: &Usage) {
    t.input_tokens += u.input_tokens;
    t.output_tokens += u.output_tokens;
    t.cache_read_input_tokens += u.cache_read_input_tokens;
    t.cache_creation_input_tokens += u.cache_creation_input_tokens;
    t.cache_creation_1h_input_tokens += u.cache_creation_1h_input_tokens;
}
