//! The memory pass's tests (M6 step 31a, §3.2): eligibility and the
//! recursion exclusion, the labels' rows, the frame rule on tokio's paused
//! clock, and nothing at all with memory off.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::index::{IndexNeighbour, IndexNeighboursParams, IndexNeighboursResult};
use theseus_store::kinds;

use super::{eligible, IndexFuture, MemoryPass, PassIndex, Timing};
use crate::config::{MemoryConfig, MemoryMode};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, Origin, RecalledRef, ResultStatus};
use crate::recall::Memory;
use crate::store::Store;
use crate::tender::TenderMiss;
use theseus_protocol::Usage;

/// A stand-in index: fixed neighbours by node, refusals until a node's
/// vector "lands", a state, and entities by a toy rule (a token with a
/// slash is a path, seven or more hex digits a commit).
#[derive(Default)]
pub(crate) struct Fixed {
    pub near: Mutex<BTreeMap<String, Vec<(String, f64)>>>,
    /// Refusals left before a node's neighbours answer.
    pub not_yet: Mutex<BTreeMap<String, u32>>,
    pub state: Mutex<String>,
    pub down: Mutex<Option<String>>,
    pub asked: Mutex<Vec<String>>,
}

pub(crate) fn toy_entities(text: &str) -> Vec<String> {
    let mut out: Vec<String> = text
        .split(|c: char| c.is_whitespace() || ",;()\"'`".contains(c))
        .filter_map(|t| {
            let t = t.trim_end_matches(['.', ':']);
            if t.contains('/') {
                Some(format!("path:{t}"))
            } else if t.len() >= 7
                && t.chars().all(|c| c.is_ascii_hexdigit())
                && t.chars().any(|c| c.is_ascii_digit())
                && t.chars().any(|c| c.is_ascii_alphabetic())
            {
                Some(format!("commit:{t}"))
            } else {
                None
            }
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

impl PassIndex for Fixed {
    fn neighbours(&self, p: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult> {
        self.asked.lock().unwrap().push(p.node_id.clone());
        let r = if let Some(why) = self.down.lock().unwrap().clone() {
            Err(TenderMiss::Down(why))
        } else {
            let mut left = self.not_yet.lock().unwrap();
            match left.get_mut(&p.node_id) {
                Some(n) if *n > 0 => {
                    *n -= 1;
                    Err(TenderMiss::Refused(format!(
                        "node {} has no vector yet",
                        p.node_id
                    )))
                }
                _ => {
                    let near = self
                        .near
                        .lock()
                        .unwrap()
                        .get(&p.node_id)
                        .cloned()
                        .unwrap_or_default();
                    Ok(IndexNeighboursResult {
                        node_id: p.node_id.clone(),
                        neighbours: near
                            .into_iter()
                            .map(|(node_id, score)| IndexNeighbour {
                                node_id,
                                chunk: 0,
                                score,
                                position: 1,
                                session_id: "ses_other".into(),
                                kind: "user_message".into(),
                            })
                            .collect(),
                        vector_ms: 0.1,
                    })
                }
            }
        };
        Box::pin(async move { r })
    }

    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        let r = match self.down.lock().unwrap().clone() {
            Some(why) => Err(TenderMiss::Down(why)),
            None => Ok(texts.iter().map(|t| toy_entities(t)).collect()),
        };
        Box::pin(async move { r })
    }

    fn state(&self) -> IndexFuture<String> {
        let s = self.state.lock().unwrap().clone();
        Box::pin(async move { Ok(s) })
    }
}

pub(crate) struct Rig {
    pub store: Store,
    pub pass: Arc<MemoryPass>,
    pub index: Arc<Fixed>,
    _dir: tempfile::TempDir,
}

/// Short clocks: a 2 s window, and a short quiet.
pub(crate) fn timing() -> Timing {
    Timing {
        window: Duration::from_secs(2),
        quiet: Duration::from_millis(20),
        quiet_bound: Duration::from_secs(1),
    }
}

pub(crate) fn rig(mode: MemoryMode) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store")).unwrap();
    let cfg = MemoryConfig {
        mode,
        ..MemoryConfig::default()
    };
    let memory = Arc::new(Memory::new(cfg, None));
    let index = Arc::new(Fixed {
        state: Mutex::new("ready".into()),
        ..Fixed::default()
    });
    let pass = MemoryPass::with_timing(memory, store.clone(), Some(index.clone()), timing());
    Rig {
        store,
        pass,
        index,
        _dir: dir,
    }
}

impl Rig {
    pub fn put(&self, nodes: &[&Node]) {
        let records: Vec<_> = nodes.iter().map(|n| n.record().unwrap()).collect();
        self.store.append(&records).unwrap();
    }

    /// The rows of `kind` in `scope`.
    pub fn rows(&self, scope: &str, kind: &str) -> Vec<LedgerRow> {
        self.store
            .scope_after(scope, 0)
            .unwrap()
            .into_iter()
            .filter(|r| r.kind == kinds::LEDGER)
            .map(|r| r.decode::<LedgerRow>().unwrap())
            .filter(|r| r.kind == kind)
            .collect()
    }

    pub fn labeled(&self, sid: &str) -> Vec<LedgerRow> {
        self.rows(&crate::fact::memory::scope(sid), "memory.labeled")
    }

    /// A pass of `sid` after `turn`, and its frames written: the window
    /// run out on the paused clock.
    pub async fn pass(&self, sid: &str, turn: &str) {
        self.pass.ended(sid, turn);
        settle().await;
    }

    pub fn frames(&self) -> u64 {
        self.store.stats().unwrap().frames_appended
    }
}

/// Let the pass's task run, and its window and quiet pass.
pub(crate) async fn settle() {
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }
}

pub(crate) fn reply(sid: &str, turn: &str, text: &str) -> Node {
    Node::assistant(
        sid,
        turn,
        0,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": text})],
            model: "fake".into(),
            provider: "fake".into(),
            stop_reason: Some("end_turn".into()),
            usage: Usage::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
        },
    )
}

fn call(sid: &str, turn: &str, input: Value) -> Node {
    Node::tool_call(
        sid,
        Some(turn),
        Some(0),
        Body::ToolCall {
            tool_use_id: "tu_1".into(),
            tool: "fs.read".into(),
            wire_name: "fs_read".into(),
            input,
            assistant_node: "msg_x".into(),
            correlation_id: None,
            gate: None,
        },
    )
}

fn result(sid: &str, turn: &str, text: &str) -> Node {
    Node::tool_result(
        sid,
        Some(turn),
        Some(0),
        Body::ToolResult {
            tool_use_id: "tu_1".into(),
            tool: "fs.read".into(),
            status: ResultStatus::Ok,
            is_error: false,
            content: text.into(),
            correlation_id: None,
            bytes_total: text.len() as u64,
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

fn user(sid: &str, turn: &str, text: &str) -> Node {
    Node::user(sid, Some(turn), "cli", text)
}

fn harness(sid: &str, turn: &str, text: &str) -> Node {
    Node::relayed(sid, Some(turn), Origin::Harness, "harness", text)
}

/// Eligibility (§2.6, §5.2): the operator's and the agent's messages and
/// tool results; never a recall, whoever wrote it, a harness line, or a
/// tool call. Through the pass, only the eligible are labeled.
#[tokio::test(start_paused = true)]
async fn only_the_eligible_are_labeled_and_a_recall_never_is() {
    let r = rig(MemoryMode::Shadow);
    let sid = "ses_heron";
    let u = user(
        sid,
        "trn_1",
        "The heron nests by the old weir at Millbrook.",
    );
    let c = call(sid, "trn_1", json!({"path": "notes/heron.md"}));
    let t = result(sid, "trn_1", "heron: weir, Millbrook");
    let a = reply(sid, "trn_1", "It nests by the old weir.");
    let h = harness(sid, "trn_1", "[the harness repaired a call]");
    let mut rc = Node::recall(
        sid,
        "trn_1",
        "rcl_1",
        "baseline",
        vec![RecalledRef {
            node_id: u.id.clone(),
            session_id: sid.into(),
            position: 1,
            chunk: (0, 10),
            header: "a note".into(),
            tokens: 3,
        }],
    );
    let empty = reply(sid, "trn_1", "");
    assert!(eligible(&u) && eligible(&t) && eligible(&a));
    assert!(!eligible(&c) && !eligible(&h) && !eligible(&empty));
    assert!(!eligible(&rc), "a recall is never labeled");
    // Not even one a non-harness origin wrote: the exclusion is the body's.
    rc.origin = Origin::Agent;
    assert!(
        !eligible(&rc),
        "a recall is never labeled, whoever wrote it"
    );
    r.put(&[&u, &c, &t, &a, &h, &rc, &empty]);
    r.pass(sid, "trn_1").await;
    let labeled: Vec<String> = r
        .labeled(sid)
        .iter()
        .map(|l| l.data["node_id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(labeled, [u.id.clone(), t.id.clone(), a.id.clone()]);
    // A second pass labels nothing again.
    let frames = r.frames();
    r.pass(sid, "trn_1").await;
    assert_eq!(r.frames(), frames);
    assert_eq!(r.labeled(sid).len(), 3);
}

/// A node's row: its labels, its entities (from the index), and its trust.
#[tokio::test(start_paused = true)]
async fn a_labeled_row_carries_the_tables_labels_and_the_indexs_entities() {
    let r = rig(MemoryMode::Shadow);
    let sid = "ses_kestrel";
    let u = user(
        sid,
        "trn_1",
        "Always run the kestrel tests from crates/kestrel/tests first.",
    );
    r.put(&[&u]);
    r.pass(sid, "trn_1").await;
    let rows = r.labeled(sid);
    assert_eq!(rows.len(), 1);
    let d = &rows[0].data;
    assert_eq!(d["kind"], "preference");
    assert_eq!(d["durability"], "high");
    assert_eq!(d["trust"], "own");
    assert_eq!(d["about"], json!(["path:crates/kestrel/tests"]));
    assert_eq!(d["by"], "rules");
    assert_eq!(rows[0].session_id.as_deref(), Some(sid));

    // No tender to ask: labeled all the same, and the row says why there
    // are no entities.
    let r = rig(MemoryMode::Shadow);
    *r.index.down.lock().unwrap() = Some("the index is off: [index] enabled = false".into());
    r.put(&[&u]);
    r.pass(sid, "trn_1").await;
    let l = r.labeled(sid);
    assert_eq!(l[0].data["kind"], "preference");
    assert!(l[0].data["entities_unavailable"]
        .as_str()
        .unwrap()
        .contains("enabled = false"));
}

/// The frame rule (on tokio's paused clock): nothing before the window,
/// one frame when it runs out; and a backlog is one frame per 32 nodes,
/// never a node's records split between two.
#[tokio::test(start_paused = true)]
async fn at_most_one_frame_per_32_nodes_or_2_seconds() {
    let r = rig(MemoryMode::Shadow);
    let sid = "ses_plover";
    let few: Vec<Node> = (0..5)
        .map(|i| {
            user(
                sid,
                "trn_1",
                &format!("The plover count at dune {i} was eleven today."),
            )
        })
        .collect();
    r.put(&few.iter().collect::<Vec<_>>());
    let before = r.frames();
    r.pass.ended(sid, "trn_1");
    tokio::time::sleep(Duration::from_millis(1900)).await;
    assert_eq!(r.frames(), before, "nothing within the window");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(r.frames(), before + 1, "one frame once it ran out");
    assert_eq!(r.labeled(sid).len(), 5);

    let r = rig(MemoryMode::Shadow);
    let many: Vec<Node> = (0..70)
        .map(|i| {
            user(
                sid,
                "trn_1",
                &format!("The plover count at dune {i} was eleven today."),
            )
        })
        .collect();
    r.put(&many.iter().collect::<Vec<_>>());
    let before = r.frames();
    r.pass.ended(sid, "trn_1");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(r.frames(), before + 2, "two full frames at once");
    assert_eq!(r.labeled(sid).len(), 64);
    settle().await;
    assert_eq!(
        r.frames(),
        before + 3,
        "and the 6 left when the window ran out"
    );
    assert_eq!(r.labeled(sid).len(), 70);
}

/// With memory off the pass reads nothing and writes nothing.
#[tokio::test(start_paused = true)]
async fn with_memory_off_the_pass_does_nothing() {
    let r = rig(MemoryMode::Off);
    let sid = "ses_tern";
    let u = user(sid, "trn_1", "The tern colony moved to the north spit.");
    r.put(&[&u]);
    let before = r.frames();
    r.pass(sid, "trn_1").await;
    assert_eq!(r.frames(), before);
    assert!(r.labeled(sid).is_empty());
}
