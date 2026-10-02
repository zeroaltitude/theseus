//! The fixture writer: every item's past, written into a scratch store as the
//! product writes sessions (as `theseus-sim`'s `synth.rs` does for the
//! lifecycle bench), so no model runs to build them.
//!
//! Each past session is written whole, in one frame: its execution, parked on
//! input; its nodes, one turn per operator message (a tool node is the
//! assistant's call, the call, and its result, in one loop); a `turn.ended`
//! row per turn; and its record, with its place as its label (as the Discord
//! binding labels one) and, when it fetched text, the hold that text gives it
//! (T1). Times are the item's, not the writer's. The manifest names each node
//! by its item's key, with the id and position the store gave it: what the
//! oracle renders, and what a test compares with what a daemon serves.
//!
//! exam-v2's generated sessions are ordinary sessions by the time they get
//! here (`generate.rs` expands them when the exam loads), so the writer
//! needs nothing new for them but the background's owner: its sessions are
//! keyed `background/<key>.<n>`. Consecutive sessions share a frame, up to
//! `FRAME_RECORDS` records, since each frame is a sync: exam-v2's 758
//! sessions, one frame each, spent 15 s in them. A session is never split
//! across frames, and the daemon reads records, not frames.

use std::collections::BTreeMap;
use std::path::Path;
use theseus_protocol::LedgerKind;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_core::ledger::LedgerRow;
use theseus_core::node::{Body, Node, ResultStatus};
use theseus_core::session::SessionRecord;
use theseus_core::store::Store;
use theseus_kernel::{new_id, Authority, Budget, ExecState, Execution, SessionKind, Wake, SCHEMA};
use theseus_store::{kinds, NewRecord};

use crate::item::{Exam, Item, PastNode, PastSession, AGENT, TOOL};
use crate::time::parse_local;

/// The model a past assistant message names: the exam's past was "written"
/// by the cheap profile.
pub const PAST_MODEL: &str = "glm-5.3-flash";
pub const PAST_PROVIDER: &str = "zai";

/// One past session, as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEntry {
    /// The session's owner: an item's id, or `background` (exam-v2's
    /// sessions that no item owns).
    pub item: String,
    pub key: String,
    pub session_id: String,
    pub place: String,
    pub turns: u64,
    /// Store nodes, in order (a tool node is three).
    pub nodes: usize,
}

/// One keyed past node (`<item>/<session>.<n>`), as written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeEntry {
    pub node_id: String,
    pub session_id: String,
    pub position: u64,
    pub at_ms: u64,
    /// `user_message`, `assistant_message`, or `tool_result`.
    pub kind: String,
    pub place: String,
    /// As the recall note names it: the operator's name, `theseus`, or
    /// `<tool> result`.
    pub who: String,
    pub text: String,
    #[serde(default)]
    pub volatile: bool,
    #[serde(default)]
    pub external: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub exam: String,
    pub digest: String,
    pub utc_offset_min: i32,
    pub sessions: Vec<SessionEntry>,
    pub nodes: BTreeMap<String, NodeEntry>,
    /// The store's last position after the write.
    pub last_position: u64,
}

impl Manifest {
    pub fn key(item: &str, node: &str) -> String {
        format!("{item}/{node}")
    }

    /// An item's gold, in its order.
    pub fn gold(&self, item: &Item) -> Result<Vec<&NodeEntry>> {
        item.gold
            .iter()
            .map(|g| {
                self.nodes
                    .get(&Manifest::key(&item.id, g))
                    .with_context(|| {
                        format!(
                            "the manifest has no node {}/{g}: rewrite the store",
                            item.id
                        )
                    })
            })
            .collect()
    }

    pub fn read(path: &Path) -> Result<Manifest> {
        let b = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_slice(&b).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, serde_json::to_vec_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }
}

/// An operator's author label for a place: `discord:<name>` on Discord, as the
/// binding labels one; `<place>:<name>` elsewhere.
pub fn author(place: &str, who: &str) -> String {
    if place.starts_with("discord") {
        format!("discord:{who}")
    } else {
        format!("{place}:{who}")
    }
}

fn wire_name(tool: &str) -> String {
    tool.replace('.', "_")
}

fn assistant(
    sid: &str,
    turn: &str,
    lp: u32,
    blocks: Vec<Value>,
    stop: &str,
    at: u64,
) -> Result<Node> {
    let body: Body = serde_json::from_value(json!({
        "kind": "assistant_message",
        "blocks": blocks,
        "model": PAST_MODEL,
        "provider": PAST_PROVIDER,
        "stop_reason": stop,
        "usage": {"input_tokens": 0, "output_tokens": 0},
        "cost_usd": 0.0,
    }))?;
    let mut n = Node::assistant(sid, turn, lp, body);
    n.created_at_ms = at;
    Ok(n)
}

/// One session's records, and its keyed nodes (index into the frame).
struct Built {
    records: Vec<NewRecord>,
    keyed: Vec<(String, usize, NodeEntry)>,
    entry: SessionEntry,
}

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn build(exam: &Exam, owner: &str, s: &PastSession) -> Result<Built> {
    let off = exam.file.utc_offset_min;
    let times: Vec<u64> = s
        .nodes
        .iter()
        .map(|n| parse_local(&n.at, off))
        .collect::<Result<_>>()?;
    let (start, end) = (times[0], *times.last().expect("a session has nodes"));
    let mut rec = SessionRecord::new(SessionKind::Conversation, Some(s.place.clone()));
    let sid = rec.session_id.clone();
    rec.created_at_unix_ms = start;
    rec.last_active_ms = end;
    let exec = Execution {
        id: new_id("exe"),
        schema: SCHEMA,
        session_id: sid.clone(),
        kind: SessionKind::Conversation,
        state: ExecState::Waiting,
        authority: Authority {
            principal: theseus_core::turn::OPERATOR.to_string(),
            ..Default::default()
        },
        budget: Budget::new(100_000_000),
        wake: Some(Wake::Input),
        outstanding: vec![],
        queued_results: vec![],
        parent: None,
        reports_to: None,
        reports: vec![],
        wakes: vec![],
        wake_parent: false,
        report_wakes: vec![],
        stopped: None,
        turns: 0,
        interrupted: 0,
        resume_pending: false,
        cancel: None,
        ended_reason: None,
        created_at_ms: start,
        updated_at_ms: end,
    };
    let ledger =
        |kind: LedgerKind, turn: Option<&str>, at: u64, data: Value| -> Result<NewRecord> {
            let mut row = LedgerRow::new(kind, Some(&sid), turn, data);
            row.at_unix_ms = at;
            Ok(NewRecord::json(kinds::LEDGER, None, &row)?.scoped(&sid))
        };
    let mut records = vec![
        NewRecord::json(kinds::EXECUTION, Some(&exec.id), &exec)?.scoped(&sid),
        ledger(
            LedgerKind::ExecutionOpened,
            None,
            start,
            json!({"execution_id": exec.id, "kind": "conversation", "limit_usd": 100.0}),
        )?,
        ledger(
            LedgerKind::SessionOpened,
            None,
            start,
            json!({"execution_id": exec.id}),
        )?,
    ];
    let mut keyed = Vec::new();
    let mut turn: Option<String> = None;
    let mut lp = 0u32;
    let mut turns = 0u64;
    let mut store_nodes = 0usize;
    let mut hold: Option<(Value, String)> = None;
    let mut push_node = |records: &mut Vec<NewRecord>, n: &Node| -> Result<usize> {
        records.push(n.record()?);
        store_nodes += 1;
        Ok(records.len() - 1)
    };
    for (k, (p, &at)) in s.nodes.iter().zip(&times).enumerate() {
        let key = format!("{}.{}", s.key, k + 1);
        let entry = |node: &Node, kind: &str, who: String, n: &PastNode| NodeEntry {
            node_id: node.id.clone(),
            session_id: sid.clone(),
            position: 0,
            at_ms: at,
            kind: kind.into(),
            place: s.place.clone(),
            who,
            text: n.text.clone(),
            volatile: n.volatile,
            external: n.external.is_some(),
        };
        if p.is_operator() {
            if let Some(t) = turn.take() {
                records.push(ledger(
                    LedgerKind::TurnEnded,
                    Some(&t),
                    at,
                    json!({"execution_id": exec.id, "outcome": "waiting", "loops": lp + 1, "cost_usd": 0.0}),
                )?);
            }
            let t = new_id("trn");
            turns += 1;
            lp = 0;
            let mut n = Node::user(&sid, Some(&t), &author(&s.place, &p.who), &p.text);
            n.created_at_ms = at;
            if rec.title.is_none() {
                rec.title = Some(theseus_core::session::title_from(&p.text));
            }
            let i = push_node(&mut records, &n)?;
            keyed.push((key, i, entry(&n, "user_message", p.who.clone(), p)));
            turn = Some(t);
        } else if p.who == AGENT {
            let t = turn.as_deref().expect("a session starts with the operator");
            let n = assistant(
                &sid,
                t,
                lp,
                vec![json!({"type": "text", "text": p.text})],
                "end_turn",
                at,
            )?;
            let i = push_node(&mut records, &n)?;
            keyed.push((key, i, entry(&n, "assistant_message", AGENT.into(), p)));
        } else {
            debug_assert_eq!(p.who, TOOL);
            let t = turn.clone().expect("a session starts with the operator");
            let tool = p.tool.clone().expect("validated");
            let input: Value = serde_json::from_str(p.input.as_deref().unwrap_or("{}"))?;
            let tuid = format!("toolu_{}", new_id("t").trim_start_matches("t_"));
            let call =
                json!({"type": "tool_use", "id": tuid, "name": wire_name(&tool), "input": input});
            let a = assistant(&sid, &t, lp, vec![call], "tool_use", at)?;
            push_node(&mut records, &a)?;
            let mut c = Node::tool_call(
                &sid,
                Some(&t),
                Some(lp),
                Body::ToolCall {
                    tool_use_id: tuid.clone(),
                    tool: tool.clone(),
                    wire_name: wire_name(&tool),
                    input,
                    assistant_node: a.id.clone(),
                    correlation_id: None,
                    gate: Some(Box::new(theseus_protocol::GateRecord {
                        result: theseus_protocol::GateResult {
                            gate: "allow".into(),
                            ..Default::default()
                        },
                        validated: true,
                        decision: Some(theseus_protocol::GateDecision {
                            posture: Some("open".into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    })),
                },
            );
            c.created_at_ms = at;
            push_node(&mut records, &c)?;
            let error = p.status.as_deref() == Some("error");
            let body: Body = serde_json::from_value(json!({
                "kind": "tool_result",
                "tool_use_id": tuid,
                "tool": tool,
                "status": if error { ResultStatus::Error } else { ResultStatus::Ok },
                "is_error": error,
                "content": p.text,
                "bytes_total": p.text.len(),
                "external": p.external.as_ref().map(|u| json!({"url": u})),
            }))?;
            let mut r = Node::tool_result(&sid, Some(&t), Some(lp), body);
            r.created_at_ms = at;
            let i = push_node(&mut records, &r)?;
            keyed.push((
                key,
                i,
                entry(&r, "tool_result", format!("{tool} result"), p),
            ));
            if let (Some(url), None) = (&p.external, &hold) {
                hold = Some((
                    json!({"since_ms": at, "tool": tool, "url": url, "node_id": r.id}),
                    t.clone(),
                ));
            }
            lp += 1;
        }
    }
    if let Some(t) = turn.take() {
        records.push(ledger(
            LedgerKind::TurnEnded,
            Some(&t),
            end,
            json!({"execution_id": exec.id, "outcome": "waiting", "loops": lp + 1, "cost_usd": 0.0}),
        )?);
        rec.last_turn_id = Some(t);
    }
    rec.turns = turns;
    rec.execution_id = Some(exec.id.clone());
    // The execution's own count of turns, as a turn's end leaves it.
    let mut exec = exec;
    exec.turns = turns;
    records[0] = NewRecord::json(kinds::EXECUTION, Some(&exec.id), &exec)?.scoped(&sid);
    match hold {
        Some((h, t)) => {
            let h: theseus_protocol::ExternalText = serde_json::from_value(h)?;
            let mut held = theseus_core::external::hold(rec, h, Some(&t))?
                .expect("a fresh record holds nothing yet");
            if let Some(row) = held.first_mut() {
                // The product's own records, with the row at the past's time.
                let mut r: LedgerRow = serde_json::from_slice(&row.payload)?;
                r.at_unix_ms = end;
                row.payload = serde_json::to_vec(&r)?;
            }
            records.extend(held);
        }
        None => records.push(NewRecord::json(kinds::SESSION, Some(&sid), &rec)?),
    }
    Ok(Built {
        records,
        keyed,
        entry: SessionEntry {
            item: owner.to_string(),
            key: s.key.clone(),
            session_id: sid,
            place: s.place.clone(),
            turns,
            nodes: store_nodes,
        },
    })
}

/// The records a frame holds before the next session starts another.
pub const FRAME_RECORDS: usize = 512;

/// Sessions built and waiting for their frame: each with its owner and where
/// its records start in the frame.
struct Frame<'a> {
    records: Vec<NewRecord>,
    sessions: Vec<(&'a str, usize, Built)>,
}

impl Frame<'_> {
    /// Append the frame, and put its sessions and keyed nodes, with the
    /// positions the store gave them, into the manifest's lists.
    fn flush(
        &mut self,
        store: &Store,
        nodes: &mut BTreeMap<String, NodeEntry>,
        sessions: &mut Vec<SessionEntry>,
    ) -> Result<()> {
        if self.records.is_empty() {
            return Ok(());
        }
        let positions = store.append(&self.records)?;
        for (owner, start, b) in self.sessions.drain(..) {
            for (key, i, mut e) in b.keyed {
                e.position = positions[start + i];
                nodes.insert(Manifest::key(owner, &key), e);
            }
            sessions.push(b.entry);
        }
        self.records.clear();
        Ok(())
    }
}

/// Write every item's past into the store at `dir` (created, or opened if it
/// exists), each session whole in one frame, then checkpoint, so the store
/// opens with no tail to replay.
pub fn write(exam: &Exam, dir: &Path) -> Result<Manifest> {
    let store =
        Store::open(dir).with_context(|| format!("opening the store at {}", dir.display()))?;
    let mut sessions = Vec::new();
    let mut nodes = BTreeMap::new();
    let mut frame = Frame {
        records: Vec::new(),
        sessions: Vec::new(),
    };
    for (owner, s) in exam.pasts() {
        let mut b = build(exam, owner, s).with_context(|| format!("{owner} session {}", s.key))?;
        if frame.records.len() + b.records.len() > FRAME_RECORDS {
            frame.flush(&store, &mut nodes, &mut sessions)?;
        }
        let start = frame.records.len();
        frame.records.append(&mut b.records);
        frame.sessions.push((owner, start, b));
    }
    frame.flush(&store, &mut nodes, &mut sessions)?;
    store.checkpoint()?;
    Ok(Manifest {
        exam: exam.file.version.clone(),
        digest: exam.digest.clone(),
        utc_offset_min: exam.file.utc_offset_min,
        sessions,
        nodes,
        last_position: store.last_position(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::{BACKGROUND, EXAM_V1, EXAM_V2};
    use theseus_core::rpc::Core;

    /// The store reads back, through the product's own readers, exactly as
    /// the manifest says it was written: every session with its place, turns,
    /// and title; every keyed node at its position, with its time, author,
    /// and text; and the fetched text marked, and held by its session.
    #[test]
    fn the_written_store_reads_back_as_the_manifest_says() {
        let exam = Exam::parse(EXAM_V1).unwrap();
        let with_past = exam
            .file
            .items
            .iter()
            .filter(|i| !i.sessions.is_empty())
            .count();
        assert_eq!(with_past, 36);
        reads_back(&exam);
    }

    /// exam-v2's store, the same way: its generated sessions, months apart,
    /// and its background, which no item owns.
    #[test]
    fn the_v2_store_reads_back_as_the_manifest_says() {
        let exam = Exam::parse(EXAM_V2).unwrap();
        let m = reads_back(&exam);
        let bg = m.sessions.iter().filter(|s| s.item == BACKGROUND).count();
        assert_eq!(bg, exam.background.len());
        assert!(
            bg > 0 && m.sessions.len() >= 500,
            "{} sessions",
            m.sessions.len()
        );
        let span: Vec<u64> = m.nodes.values().map(|n| n.at_ms).collect();
        let days = (span.iter().max().unwrap() - span.iter().min().unwrap()) / 86_400_000;
        assert!(days >= 180, "{days} days");
    }

    fn reads_back(exam: &Exam) -> Manifest {
        let d = tempfile::tempdir().unwrap();
        let m = write(exam, d.path()).unwrap();
        assert_eq!(m.sessions.len(), exam.pasts().count());
        let store = Store::open(d.path()).unwrap();
        let sessions: Vec<SessionRecord> = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), m.sessions.len());
        let kernel = theseus_kernel::Kernel::new(
            store.shared(),
            std::sync::Arc::new(theseus_kernel::RealClock),
            theseus_kernel::KernelConfig::default(),
        );
        let execs = kernel.open_executions().unwrap();
        assert_eq!(execs.len(), m.sessions.len());
        assert!(execs
            .iter()
            .all(|e| e.state == ExecState::Waiting && e.wake == Some(Wake::Input)));
        for e in &m.sessions {
            let rec = sessions
                .iter()
                .find(|s| s.session_id == e.session_id)
                .unwrap();
            let past = exam.session(&e.item, &e.key).unwrap();
            assert_eq!(rec.label.as_deref(), Some(past.place.as_str()));
            assert_eq!(rec.turns, e.turns);
            assert_eq!(
                rec.title.as_deref(),
                Some(theseus_core::session::title_from(&past.nodes[0].text).as_str())
            );
            let nodes = store.session_nodes(&e.session_id).unwrap();
            assert_eq!(nodes.len(), e.nodes);
            let fetched = past.nodes.iter().any(|n| n.external.is_some());
            assert_eq!(rec.external.is_some(), fetched, "{}/{}", e.item, e.key);
            for (k, p) in past.nodes.iter().enumerate() {
                let entry = &m.nodes[&Manifest::key(&e.item, &format!("{}.{}", e.key, k + 1))];
                let (pos, node) = nodes.iter().find(|(_, n)| n.id == entry.node_id).unwrap();
                assert_eq!(*pos, entry.position);
                assert_eq!(node.created_at_ms, entry.at_ms);
                assert_eq!(node.created_at_ms, parse_local(&p.at, -420).unwrap());
                let info = Core::node_info(*pos, node);
                assert_eq!(info.kind, entry.kind);
                assert_eq!(info.text, p.text, "{}/{}.{}", e.item, e.key, k + 1);
                if p.is_operator() {
                    assert_eq!(
                        node.author.as_deref(),
                        Some(author(&past.place, &p.who).as_str())
                    );
                }
                assert_eq!(info.detail["external"].is_object(), p.external.is_some());
            }
        }
        // Positions grow, and the manifest's last one is the store's.
        assert_eq!(m.last_position, store.last_position());
        m
    }

    /// A tool node is the assistant's call, the call, and its result, in one
    /// loop, and the agent's next message is the next loop.
    #[test]
    fn a_tool_node_is_one_loop_of_three_nodes() {
        let exam = Exam::parse(EXAM_V1).unwrap();
        let d = tempfile::tempdir().unwrap();
        let m = write(&exam, d.path()).unwrap();
        let store = Store::open(d.path()).unwrap();
        let e = m.sessions.iter().find(|s| s.item == "episode-1").unwrap();
        let nodes = store.session_nodes(&e.session_id).unwrap();
        let kinds: Vec<(&str, Option<u32>)> = nodes
            .iter()
            .map(|(_, n)| (n.kind_str(), n.loop_index))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("user_message", None),
                ("assistant_message", Some(0)),
                ("tool_call", Some(0)),
                ("tool_result", Some(0)),
                ("assistant_message", Some(1)),
            ]
        );
        let turn = nodes[0].1.turn_id.clone();
        assert!(turn.is_some() && nodes.iter().all(|(_, n)| n.turn_id == turn));
        let Body::ToolResult {
            status, is_error, ..
        } = &nodes[3].1.body
        else {
            panic!("a result");
        };
        assert_eq!((*status, *is_error), (ResultStatus::Error, true));
        let Body::ToolCall {
            assistant_node,
            wire_name,
            ..
        } = &nodes[2].1.body
        else {
            panic!("a call");
        };
        assert_eq!(
            (assistant_node.as_str(), wire_name.as_str()),
            (nodes[1].1.id.as_str(), "proc_run")
        );
    }
}
