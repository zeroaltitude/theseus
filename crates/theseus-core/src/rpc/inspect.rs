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
    InspectAnswer, InspectCall, InspectLoop, InspectPrompt, InspectRow,
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

pub(super) const WITHHELD: &str = "thinking, prompts, tool inputs and results go to a private place \
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
        let _: crate::session::SessionRecord = self
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

        let rows = self.turn_rows(sid, tid, from)?;
        let nodes = self.turn_nodes(sid, tid, from)?;
        // The next turn: the oldest start after this one; the walk is
        // newest first, so it is the one before in the list.
        let next_turn = at.checked_sub(1).map(|i| starts[i].1.clone());

        let mut out = TurnInspectResult {
            session_id: sid.into(),
            turn_id: tid.into(),
            url: self.cfg.cockpit.turn_url(sid, tid),
            prev_turn,
            next_turn,
            withheld: (!private).then(|| WITHHELD.to_string()),
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
    let len = |k: &str| b.get(k).and_then(Value::as_str).map_or(0, |s| s.chars().count());
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

/// Builds the answer from the turn's rows and nodes (pure, so a test feeds it).
pub(crate) fn assemble(
    out: &mut TurnInspectResult,
    rows: Vec<(u64, LedgerRow)>,
    nodes: Vec<(u64, Node)>,
    private: bool,
) {
    let mut errors = 0u32;
    let mut compiled: BTreeMap<u32, ContextCompiled> = BTreeMap::new();
    for (pos, row) in &rows {
        match row.kind.as_str() {
            "turn.started" => out.started_ms = row.at_unix_ms,
            "turn.ended" => {
                out.outcome = "ended".into();
                out.ended_ms = Some(row.at_unix_ms);
                out.stop_reason = row.data["stop_reason"].as_str().map(str::to_string);
                out.totals.wall_ms = row.data["elapsed_ms"].as_u64().unwrap_or(0);
            }
            "turn.failed" | "turn.refused" if out.outcome.is_empty() => {
                out.outcome = row.kind.trim_start_matches("turn.").into();
                out.ended_ms = Some(row.at_unix_ms);
                errors += 1;
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
                    compiled.insert(c.loop_index, c);
                }
            }
            "provider.error" | "provider.refusal" | "tool.invalid_input" => errors += 1,
            _ => {}
        }
        out.rows.push(InspectRow {
            position: *pos,
            at_ms: row.at_unix_ms,
            kind: row.kind.clone(),
            data: if private { row.data.clone() } else { Value::Null },
        });
    }
    if out.outcome.is_empty() {
        out.outcome = "running".into();
    }
    if out.started_ms == 0 {
        out.started_ms = rows.first().map_or(0, |(_, r)| r.at_unix_ms);
    }
    if out.totals.wall_ms == 0 {
        out.totals.wall_ms = out
            .ended_ms
            .map_or(0, |e| e.saturating_sub(out.started_ms));
    }

    // The nodes: the prompt, each loop's answer, each call and its result.
    let mut loops: BTreeMap<u32, InspectLoop> = BTreeMap::new();
    let mut loop_of_answer: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_use: BTreeMap<String, (u32, usize)> = BTreeMap::new();
    let mut order: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for (_, n) in &nodes {
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
            Body::AssistantMessage {
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
            } => {
                let i = n.loop_index.unwrap_or(0);
                loop_of_answer.insert(n.id.clone(), i);
                add(&mut out.totals.usage, usage);
                out.totals.cost_usd += cost_usd.unwrap_or(0.0);
                out.totals.model_calls += 1;
                order.entry(i).or_default().push(n.id.clone());
                loops.entry(i).or_default().answer = Some(InspectAnswer {
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
                });
            }
            Body::ToolCall {
                tool_use_id,
                tool,
                input,
                assistant_node,
                gate,
                ..
            } => {
                let i = loop_of_answer
                    .get(assistant_node)
                    .copied()
                    .or(n.loop_index)
                    .unwrap_or(0);
                out.totals.tool_calls += 1;
                order.entry(i).or_default().push(n.id.clone());
                let l = loops.entry(i).or_default();
                by_use.insert(tool_use_id.clone(), (i, l.calls.len()));
                l.calls.push(InspectCall {
                    tool_use_id: tool_use_id.clone(),
                    tool: tool.clone(),
                    call_node: n.id.clone(),
                    called_ms: n.created_at_ms,
                    input: if private { input.clone() } else { Value::Null },
                    gate: if private { gate.clone() } else { None },
                    ..InspectCall::default()
                });
            }
            Body::ToolResult {
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
            } => {
                if let Some(&(i, k)) = by_use.get(tool_use_id) {
                    order.entry(i).or_default().push(n.id.clone());
                    // A late result replaces its placeholder in the view; the
                    // placeholder stays in the node history.
                    let c = &mut loops.get_mut(&i).unwrap().calls[k];
                    c.result_node = Some(n.id.clone());
                    c.resulted_ms = Some(n.created_at_ms);
                    c.status = Some(format!("{status:?}").to_lowercase());
                    c.is_error = *is_error;
                    c.content = if private { content.clone() } else { String::new() };
                    c.bytes_total = *bytes_total;
                    c.truncated = *truncated;
                    c.full_ref = full_ref.clone();
                    c.duration_ms = *duration_ms;
                    c.late = *late;
                    errors += u32::from(*is_error);
                }
            }
            _ => {}
        }
    }
    for (i, c) in compiled {
        loops.entry(i).or_default().compiled = Some(c);
    }
    let mut prior: Vec<String> = Vec::new();
    for (i, l) in &mut loops {
        l.loop_index = *i;
        l.added = std::mem::take(&mut prior);
        prior = order.remove(i).unwrap_or_default();
    }
    out.totals.loops = loops.len() as u32;
    out.totals.errors = errors;
    out.loops = loops.into_values().collect();
}

fn add(t: &mut Usage, u: &Usage) {
    t.input_tokens += u.input_tokens;
    t.output_tokens += u.output_tokens;
    t.cache_read_input_tokens += u.cache_read_input_tokens;
    t.cache_creation_input_tokens += u.cache_creation_input_tokens;
    t.cache_creation_1h_input_tokens += u.cache_creation_1h_input_tokens;
}
