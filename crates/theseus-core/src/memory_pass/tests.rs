//! The memory pass's tests (M6 step 31a, §3.2): eligibility and the
//! recursion exclusion, the labels' rows, the gate's thresholds making the
//! right edges over a stand-in index with fixed neighbours, a node not yet
//! embedded waiting for the next pass, an index without vectors said and
//! no edge written, attribution and its outcome, the frame rule on tokio's
//! paused clock, and nothing at all with memory off.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_protocol::index::{IndexNeighbour, IndexNeighboursParams, IndexNeighboursResult};
use theseus_store::kinds;

use super::{eligible, IndexFuture, MemoryPass, PassIndex, Timing};
use crate::config::{MemoryConfig, MemoryMode};
use crate::graph::Edge;
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
    /// A node with no `near` entry has no vector yet, as before the
    /// tender embeds it (else it has no neighbours).
    pub unknown_not_yet: Mutex<bool>,
}

impl Fixed {
    pub fn near(&self, node: &str, near: &[(&str, f64)]) {
        self.near.lock().unwrap().insert(
            node.into(),
            near.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        );
    }
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
                _ if *self.unknown_not_yet.lock().unwrap()
                    && !self.near.lock().unwrap().contains_key(&p.node_id) =>
                {
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

/// Short clocks: a 2 s window, a short quiet, a short wait for vectors.
pub(crate) fn timing() -> Timing {
    Timing {
        window: Duration::from_secs(2),
        quiet: Duration::from_millis(20),
        quiet_bound: Duration::from_secs(1),
        wait_first: Duration::from_millis(10),
        wait: Duration::from_millis(100),
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
    let pass = MemoryPass::with_timing(memory, store.clone(), Some(index.clone()), None, timing());
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

    pub fn gated(&self, sid: &str) -> BTreeMap<String, Value> {
        self.rows(&crate::fact::memory::scope(sid), "memory.gated")
            .into_iter()
            .map(|r| (r.data["node_id"].as_str().unwrap().to_string(), r.data))
            .collect()
    }

    pub fn used(&self, sid: &str) -> Vec<Value> {
        self.rows(&crate::fact::recall::scope(sid), "memory.used")
            .into_iter()
            .map(|r| r.data)
            .collect()
    }

    /// The edges into `to`.
    pub fn edges_into(&self, to: &str) -> Vec<Edge> {
        self.store
            .scope_after(&Edge::scope_into(to), 0)
            .unwrap()
            .into_iter()
            .filter(|r| r.kind == kinds::EDGE)
            .map(|r| r.decode::<Edge>().unwrap())
            .collect()
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
    // Each is gated beside its labels, and the index was asked only of them.
    assert_eq!(r.gated(sid).len(), 3);
    assert!(!r.index.asked.lock().unwrap().contains(&rc.id));
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

/// §2.3's thresholds over fixed neighbours: a near-duplicate (0.92 or
/// more) is `same_entity`, an operator's correction whose top neighbour
/// reaches 0.75 is `supersedes` from the newer node to the older, and the
/// rest store. Each edge is `via = "memory"`, scoped into the older node;
/// each row names the neighbours it saw.
#[tokio::test(start_paused = true)]
async fn the_gates_thresholds_make_the_right_edges() {
    let r = rig(MemoryMode::Shadow);
    let sid = "ses_larkspur";
    let dup = user(
        sid,
        "trn_1",
        "The staging port of the Larkspur service is 8081, as configured.",
    );
    let fix = user(
        sid,
        "trn_2",
        "Correction: Larkspur's staging port is 8082, not 8081.",
    );
    let far = user(
        sid,
        "trn_3",
        "Correction: lunch moved to the second floor this week.",
    );
    let plain = user(
        sid,
        "trn_4",
        "The Larkspur dashboards live on the staging host.",
    );
    r.index
        .near(&dup.id, &[("msg_old_a", 0.95), ("msg_old_b", 0.40)]);
    r.index.near(&fix.id, &[("msg_old_a", 0.80)]);
    r.index.near(&far.id, &[("msg_old_c", 0.70)]);
    r.index.near(&plain.id, &[("msg_old_a", 0.80)]);
    r.put(&[&dup, &fix, &far, &plain]);
    r.pass(sid, "trn_4").await;
    let g = r.gated(sid);
    assert_eq!(g[&dup.id]["decision"], "same_entity");
    assert_eq!(g[&dup.id]["to"], "msg_old_a");
    assert_eq!(g[&dup.id]["neighbours"].as_array().unwrap().len(), 2);
    assert_eq!(g[&fix.id]["decision"], "supersedes");
    assert_eq!(g[&fix.id]["correction"], true);
    assert_eq!(g[&far.id]["decision"], "store", "a correction below 0.75");
    assert_eq!(
        g[&plain.id]["decision"], "store",
        "0.80 without a correction"
    );
    assert_eq!(g[&dup.id]["merge_cosine"].as_f64().unwrap(), 0.92f32 as f64);
    let edges = r.edges_into("msg_old_a");
    let mut seen: Vec<(String, String, String)> = edges
        .iter()
        .map(|e| (e.kind.clone(), e.from.clone(), e.via.clone()))
        .collect();
    seen.sort();
    let mut want = vec![
        (
            "same_entity".to_string(),
            dup.id.clone(),
            "memory".to_string(),
        ),
        (
            "supersedes".to_string(),
            fix.id.clone(),
            "memory".to_string(),
        ),
    ];
    want.sort();
    assert_eq!(seen, want);
    assert!(r.edges_into("msg_old_c").is_empty());
    // The gate asked as of each node's position: only nodes before it.
}

/// The tender embeds a node a little after its frame: the gate asks again
/// within its bound, and a node still without a vector waits whole (no
/// labels either) for the session's next pass. An index with no vectors at
/// all (`bm25_only`), or none, is said in the row, with no edge.
#[tokio::test(start_paused = true)]
async fn a_node_not_yet_embedded_waits_and_an_index_without_vectors_is_said() {
    let r = rig(MemoryMode::Shadow);
    let sid = "ses_wren";
    let soon = user(
        sid,
        "trn_1",
        "The wren build cache lives on the scratch volume.",
    );
    let late = user(
        sid,
        "trn_1",
        "The wren release train leaves every second Tuesday.",
    );
    r.index.not_yet.lock().unwrap().insert(soon.id.clone(), 2);
    r.index
        .not_yet
        .lock()
        .unwrap()
        .insert(late.id.clone(), 1000);
    r.index.near(&soon.id, &[("msg_twin", 0.97)]);
    r.put(&[&soon, &late]);
    r.pass(sid, "trn_1").await;
    let g = r.gated(sid);
    assert_eq!(
        g[&soon.id]["decision"], "same_entity",
        "asked again, and answered"
    );
    assert!(
        !g.contains_key(&late.id),
        "still no vector: left for the next pass"
    );
    assert_eq!(r.labeled(sid).len(), 1);
    // Its vector lands; the session's next pass takes it.
    r.index.not_yet.lock().unwrap().insert(late.id.clone(), 0);
    r.pass(sid, "trn_2").await;
    assert_eq!(r.gated(sid)[&late.id]["decision"], "store");

    // No model files: the gate says so, and writes no edge.
    let r = rig(MemoryMode::Shadow);
    *r.index.state.lock().unwrap() = "bm25_only".into();
    let n = user(
        sid,
        "trn_1",
        "The wren build cache lives on the scratch volume.",
    );
    r.index.not_yet.lock().unwrap().insert(n.id.clone(), 1000);
    r.put(&[&n]);
    r.pass(sid, "trn_1").await;
    let g = r.gated(sid);
    assert_eq!(g[&n.id]["decision"], "unavailable");
    assert!(g[&n.id]["why"].as_str().unwrap().contains("BM25 alone"));
    assert_eq!(r.labeled(sid).len(), 1, "labeled all the same");

    // No tender at all: the row says why, and the entities are unavailable.
    let r = rig(MemoryMode::Shadow);
    *r.index.down.lock().unwrap() = Some("the index is off: [index] enabled = false".into());
    r.put(&[&n]);
    r.pass(sid, "trn_1").await;
    assert_eq!(r.gated(sid)[&n.id]["decision"], "unavailable");
    let l = r.labeled(sid);
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
    assert_eq!(r.gated(sid).len(), 70, "a node's rows ride together");
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
    assert!(r.index.asked.lock().unwrap().is_empty());
}

/// Attribution through a session's `Recall` node: an item the reply used
/// (an 8-word run of its excerpt) waits for the session's next input, and
/// is `ok` after a plain message; an item a tool call named (its path) is
/// used; an item nothing used is written at once, with no outcome. A
/// correction that overlaps a used item makes it `corrected`.
#[tokio::test(start_paused = true)]
async fn recalled_items_are_attributed_and_their_outcome_follows() {
    let r = rig(MemoryMode::Live);
    let src = "ses_source";
    let a = user(
        src,
        "trn_0",
        "The staging port of the Larkspur service is 8082 since the move.",
    );
    let b = user(
        src,
        "trn_0",
        "The heron notes are in docs/heron/notes.md for everyone.",
    );
    let c = user(src, "trn_0", "Lunch is at noon on Fridays in the big room.");
    r.put(&[&a, &b, &c]);
    let item = |store: &Store, n: &Node| RecalledRef {
        node_id: n.id.clone(),
        session_id: src.into(),
        position: store.get_node(&n.id).unwrap().unwrap().0,
        chunk: (0, crate::recall::text_of(n).len() as u32),
        header: "a note".into(),
        tokens: 10,
    };
    let sid = "ses_asker";
    let q = user(sid, "trn_1", "What is Larkspur's staging port?");
    let rc = Node::recall(
        sid,
        "trn_1",
        "rcl_1",
        "baseline",
        vec![item(&r.store, &a), item(&r.store, &b), item(&r.store, &c)],
    );
    let read = call(sid, "trn_1", json!({"path": "docs/heron/notes.md"}));
    let ans = reply(
        sid,
        "trn_1",
        "The staging port of the Larkspur service is 8082 since the move.",
    );
    r.put(&[&q, &rc, &read, &ans]);
    r.pass(sid, "trn_1").await;
    let used = r.used(sid);
    assert_eq!(used.len(), 1, "only the unused item is known yet: {used:?}");
    assert_eq!(used[0]["node_id"], c.id.as_str());
    assert_eq!(used[0]["used"], false);
    assert_eq!(used[0]["outcome"], Value::Null);
    // The next message goes on: both used items are `ok`.
    let next = user(sid, "trn_2", "Thanks. And who owns the dashboards?");
    r.put(&[&next]);
    r.pass(sid, "trn_2").await;
    let used = r.used(sid);
    assert_eq!(used.len(), 3, "{used:?}");
    let of = |id: &str| used.iter().find(|u| u["node_id"] == id).unwrap().clone();
    assert_eq!(of(&a.id)["used"], true);
    assert_eq!(of(&a.id)["by"], json!(["run"]));
    assert_eq!(of(&a.id)["outcome"], "ok");
    assert_eq!(
        of(&b.id)["by"],
        json!(["entity:path:docs/heron/notes.md in a call"])
    );
    assert_eq!(of(&b.id)["outcome"], "ok");
    assert_eq!(of(&a.id)["recall_id"], "rcl_1");

    // A correction that overlaps the used item: `corrected`.
    let r = rig(MemoryMode::Live);
    r.put(&[&a]);
    let rc = Node::recall(sid, "trn_1", "rcl_2", "baseline", vec![item(&r.store, &a)]);
    let ans = reply(
        sid,
        "trn_1",
        "The staging port of the Larkspur service is 8082 since the move.",
    );
    let fix = user(
        sid,
        "trn_2",
        "Correction: the Larkspur staging port is 8083 now.",
    );
    r.put(&[&q, &rc, &ans, &fix]);
    r.pass(sid, "trn_2").await;
    let used = r.used(sid);
    assert_eq!(used.len(), 1);
    assert_eq!(used[0]["outcome"], "corrected");
}

/// Wait, on the real clock, until `f` holds (the core's pass runs on its
/// own clocks: a 2 s window and a still WAL).
async fn until(what: &str, f: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(std::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn mscope(sid: &str) -> String {
    crate::fact::memory::scope(sid)
}

/// A core's rows of `kind` in `scope`, their data.
fn core_rows(c: &crate::Core, scope: &str, kind: &str) -> Vec<Value> {
    c.store
        .scope_after(scope, 0)
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == kinds::LEDGER)
        .map(|r| r.decode::<LedgerRow>().unwrap())
        .filter(|r| r.kind == kind)
        .map(|r| r.data)
        .collect()
}

/// The node of `sid` that says `said`.
fn said_in(c: &crate::Core, sid: &str, said: &str) -> Node {
    c.store
        .session_nodes(sid)
        .unwrap()
        .into_iter()
        .find(|(_, n)| matches!(&n.body, Body::UserMessage { text, .. } if text == said))
        .unwrap()
        .1
}

/// The live check's story through a whole core, offline (§3.2's 31a live
/// check, with stand-in indexes): the operator corrects an earlier fact in
/// another session, and the pass after that session's turn writes a
/// `supersedes` edge from the correction to the fact; a later question's
/// recall (live, `baseline`) admits the correction and drops the fact as
/// `superseded`; the reply uses the note, and after the session's next
/// message its `memory.used` row says `used`, outcome `ok`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_correction_supersedes_the_fact_and_recall_prefers_it() {
    use crate::provider::Scripted;
    use crate::tests_recall::{index_of, recalls, rig_with, session, turn};
    let r = rig_with(MemoryMode::Live, |c| c.memory.recall_deadline_ms = 2000);
    let c = &r.core;
    let index = Arc::new(Fixed {
        state: Mutex::new("ready".into()),
        ..Fixed::default()
    });
    c.runner.pass.set_index(index.clone());
    let a = session(
        c,
        None,
        &["The staging port of the Larkspur service is 8081."],
    );
    c.runner.memory.set_ask(index_of(c, vec![]));
    let fact = c.store.session_nodes(&a).unwrap()[0].1.clone();
    let b = session(c, None, &[]);
    let said = "Correction: Larkspur's staging port is 8082, not 8081.";
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text("Noted: 8082."));
    // B's nodes have no vectors until the test gives them: the gate waits.
    *index.unknown_not_yet.lock().unwrap() = true;
    let res = turn(c, &b, said).await;
    let fix = said_in(c, &b, said);
    for (_, n) in c.store.session_nodes(&b).unwrap() {
        if n.id != fix.id {
            index.near(&n.id, &[]);
        }
    }
    index.near(&fix.id, &[(&fact.id, 0.84)]);
    // The turn's end handed B to the pass; the pass wrote its rows.
    until("B's nodes gated", || {
        core_rows(c, &mscope(&b), "memory.gated").len() == 2
    })
    .await;
    let g = core_rows(c, &mscope(&b), "memory.gated");
    let row = g.iter().find(|g| g["node_id"] == fix.id.as_str()).unwrap();
    assert_eq!(row["decision"], "supersedes", "{row}");
    assert_eq!(row["to"], fact.id.as_str());
    let into = c.store.scope_after(&Edge::scope_into(&fact.id), 0).unwrap();
    let e = into
        .iter()
        .map(|r| r.decode::<Edge>().unwrap())
        .find(|e| e.kind == "supersedes")
        .expect("the edge");
    assert_eq!(
        (e.from.as_str(), e.via.as_str()),
        (fix.id.as_str(), "memory")
    );
    assert_eq!(res.session_id, b);

    // C asks: recall admits the correction, and drops the fact.
    let q = session(c, None, &[]);
    c.runner
        .memory
        .set_ask(index_of(c, vec![a.clone(), b.clone()]));
    r.model.script.lock().unwrap().push_back(Scripted::text(
        "As you said: Correction: Larkspur's staging port is 8082, not 8081.",
    ));
    let res = turn(c, &q, "What is Larkspur's staging port?").await;
    // The correction, and B's reply ("Noted: 8082."): never the fact.
    assert_eq!(res.recalled, 2);
    let m = recalls(c, &q).pop().unwrap();
    assert!(m.science.starts_with("baseline@"), "{}", m.science);
    let admitted: Vec<&str> = m.admitted.iter().map(|i| i.node_id.as_str()).collect();
    assert!(admitted.contains(&fix.id.as_str()), "{admitted:?}");
    assert!(!admitted.contains(&fact.id.as_str()), "{admitted:?}");
    let dropped: Vec<(&str, &str)> = m
        .dropped
        .iter()
        .map(|d| (d.node_id.as_str(), d.reason.as_str()))
        .collect();
    assert!(
        dropped.contains(&(fact.id.as_str(), "superseded")),
        "{dropped:?}"
    );

    // The next message: the used note's row, `ok`.
    r.model
        .script
        .lock()
        .unwrap()
        .push_back(Scripted::text("You're welcome."));
    turn(c, &q, "Thanks, that is what I needed.").await;
    let used = || core_rows(c, &crate::fact::recall::scope(&q), "memory.used");
    until("both items attributed", || used().len() == 2).await;
    let u = used();
    let of = |id: &str| u.iter().find(|x| x["node_id"] == id).cloned().unwrap();
    assert_eq!(of(&fix.id)["used"], true, "{u:?}");
    assert_eq!(of(&fix.id)["outcome"], "ok");
    assert_eq!(of(&fix.id)["by"], json!(["run"]));
    let other = u.iter().find(|x| x["node_id"] != fix.id.as_str()).unwrap();
    assert_eq!(
        (other["used"].clone(), other["outcome"].clone()),
        (json!(false), Value::Null)
    );
}
