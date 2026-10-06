//! The memory pass (M6 step 31a; design §2.6): after a turn ends, off its
//! path, it labels the session's new nodes, gates each against its nearest
//! neighbours, and attributes the items its recalls admitted.
//!
//! - **When.** One call beside the judge's `after_turn` hands the session to
//!   the pass's task and returns ([`MemoryPass::after_turn`]); everything
//!   else is the task's. With `[memory] mode = "off"` it does nothing.
//! - **Who is eligible** ([`eligible`]): the operator's and the agent's
//!   messages, tool results, and compaction's summaries (30c; Eddie's
//!   decision 10), though the harness writes them: a summary stands for its
//!   range's messages, and is labeled as they are, its trust its range's.
//!   Never a `Recall` node (§5.2's recursion exclusion), a task's
//!   arrangement, another harness line, a tool call; judgments, manifests,
//!   and ledger rows are not nodes.
//! - **Labels** (`labels`): §2.6's table, deterministic; `about` is the
//!   index's entity field, asked of the tender (`index.entities`), the one
//!   extractor.
//! - **The gate**: the node's neighbours by the 768-d vector
//!   (`index.neighbours`, only nodes written before it), and the science's
//!   thresholds (`MemoryScience::gate`): an operator's correction close
//!   enough to its top neighbour is a `supersedes` edge from the newer node
//!   to the older, however close (that rule comes first: theseus-lx3x), and
//!   any other near-duplicate a `same_entity` edge, both `via = "memory"`.
//!   The tender embeds a node a little after its frame: the gate
//!   asks again, waiting [`NEIGHBOUR_WAIT_FIRST`] and doubling, for at most
//!   [`NEIGHBOUR_WAIT`] in a pass, and a node still without its vector is
//!   left whole for the session's next pass. An index that has no vectors
//!   at all (no model files: `bm25_only`), or no tender, is said in the
//!   `memory.gated` row, with no edge.
//! - **Attribution** (`attribution`), of each item a `Recall` node admitted
//!   (canary and live): an item not used is written at once; a used one
//!   when the session's next input has come, with its outcome.
//! - **Jev, in shadow** (`judge::memory`): each node labeled goes to
//!   `memory.v1`, and each recall, once its turn has its reply, to
//!   `attribution.v1`, through `JudgeService` (its sampling, its shadow
//!   budget, its sink's frames); with `[judge]` off, nothing is asked and
//!   the deterministic half stands alone.
//! - **Frames.** The rows and edges go in the pass's own frames: one per
//!   [`MAX_NODES`] nodes, or [`WINDOW`] after the first waiting, whichever
//!   comes first; a node's records never split across frames. A frame is
//!   written only between turns (`turns`; theseus-ms5m, Eddie's decision
//!   10): when no turn runs in the daemon, any session's, and none has for
//!   [`QUIET`]; a turn that begins meanwhile waits for that frame at its
//!   start. So none lands inside a turn (theseus-0j2.3's lesson), however
//!   long a turn's own pauses: a still WAL inside a turn is not between
//!   turns. The bounds: after [`QUIET_BOUND`] a frame takes any moment no
//!   turn runs; after [`BUSY_BOUND`], a daemon with a turn running all that
//!   while, it is written beside them.
//! - **Crash.** What is done is read from the record: a session's
//!   `memory.labeled` rows (scoped `memory:<session>`) and its `memory.used`
//!   rows (with its recalls), read once a session by this daemon. A node a
//!   crash left unlabeled waits for its session's next pass; nothing scans
//!   on the start path.

pub mod attribution;
pub mod labels;
mod recalls;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_jev;
pub mod turns;

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock, Weak};
use std::time::Duration;

use theseus_memory::science::{Fresh, GateDecision, Neighbour};
use theseus_protocol::index::{
    method, IndexEntitiesParams, IndexEntitiesResult, IndexNeighboursParams, IndexNeighboursResult,
};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};
use tokio::sync::mpsc;

use crate::fact::memory::{scope, MemoryGated, MemoryLabeled, Seen};
use crate::graph::{Edge, EdgeKind, VIA_MEMORY};
use crate::judge::memory::MemoryAsk;
use crate::judge::JudgeService;
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, Origin};
use crate::recall::Memory;
use crate::store::{Store, Transcript};
use crate::tender::{IndexTender, TenderMiss};
use labels::Shape;

/// A frame holds at most this many nodes' records.
pub const MAX_NODES: usize = 32;
/// A frame is due at most this long after its first node waits.
pub const WINDOW: Duration = Duration::from_secs(2);
/// A frame waits until no turn has run for this long.
pub const QUIET: Duration = Duration::from_millis(500);
/// After waiting this long for a quiet stretch, a frame takes any moment
/// with no turn running. Longer than a turn bench's measured turns take
/// on a loaded machine (about 25 s at a load of 25), whose gaps it would
/// otherwise take.
pub const QUIET_BOUND: Duration = Duration::from_secs(120);
/// After waiting this long with a turn always running, a frame is written
/// beside the running turns: a daemon never between turns cannot starve the
/// pass, and its labels are at most this late.
pub const BUSY_BOUND: Duration = Duration::from_secs(600);
/// The neighbours a gate asks for.
pub const K: usize = 10;
/// The first wait for a node the tender has not embedded yet.
pub const NEIGHBOUR_WAIT_FIRST: Duration = Duration::from_millis(250);
/// The most a pass waits for the tender's vectors, all its nodes together.
pub const NEIGHBOUR_WAIT: Duration = Duration::from_secs(4);

/// What the pass asks of the index: the tender, or a test's stand-in.
pub type IndexFuture<T> = Pin<Box<dyn Future<Output = Result<T, TenderMiss>> + Send>>;

pub trait PassIndex: Send + Sync {
    /// `index.neighbours`.
    fn neighbours(&self, p: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult>;
    /// `index.entities`.
    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>>;
    /// What the index answers with (`hybrid`, or `bm25_only`: no vectors,
    /// ever), read when a neighbour is refused.
    fn mode(&self) -> IndexFuture<String>;
}

/// The index tender, as the pass asks it.
pub struct Tender(pub Arc<IndexTender>);

impl PassIndex for Tender {
    fn neighbours(&self, p: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult> {
        let t = self.0.clone();
        Box::pin(async move { t.ask(method::NEIGHBOURS, p).await })
    }

    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>> {
        let t = self.0.clone();
        Box::pin(async move {
            let r: IndexEntitiesResult = t
                .ask(method::ENTITIES, IndexEntitiesParams { texts })
                .await?;
            Ok(r.entities)
        })
    }

    fn mode(&self) -> IndexFuture<String> {
        let t = self.0.clone();
        Box::pin(async move {
            let h = t.health(crate::tender::STATUS_DEADLINE).await;
            Ok(h.status.map(|s| s.mode).unwrap_or_default())
        })
    }
}

/// A turn that ended, for the pass.
#[derive(Debug, Clone)]
struct Job {
    session_id: String,
    turn_id: String,
}

/// What a session's pass has done, read once from the record.
#[derive(Default)]
struct Done {
    /// Nodes labeled, or waiting in a batch.
    labeled: BTreeSet<String>,
    /// `(recall, node)` items attributed, or waiting in a batch.
    used: BTreeSet<(String, String)>,
}

/// One node's records, or one item's: written together, in one frame.
struct Unit {
    session_id: String,
    records: Vec<NewRecord>,
    /// What the session's `Done` holds for it, released if its frame fails.
    labeled: Option<String>,
    used: Option<(String, String)>,
}

pub struct MemoryPass {
    memory: Arc<Memory>,
    store: Store,
    index: RwLock<Option<Arc<dyn PassIndex>>>,
    /// `memory.v1` and `attribution.v1`, in shadow.
    judge: Option<Arc<JudgeService>>,
    tx: OnceLock<mpsc::UnboundedSender<Job>>,
    done: Mutex<BTreeMap<String, Done>>,
    /// The daemon's running turns, which the pass's frames wait out.
    turns: Arc<turns::Turns>,
    timing: Timing,
    me: Weak<MemoryPass>,
}

/// The pass's clocks (tests shorten them).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub window: Duration,
    pub quiet: Duration,
    pub quiet_bound: Duration,
    pub busy_bound: Duration,
    pub wait_first: Duration,
    pub wait: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            window: WINDOW,
            quiet: QUIET,
            quiet_bound: QUIET_BOUND,
            busy_bound: BUSY_BOUND,
            wait_first: NEIGHBOUR_WAIT_FIRST,
            wait: NEIGHBOUR_WAIT,
        }
    }
}

impl MemoryPass {
    /// The pass, built with the core: nothing read or started until a turn
    /// ends with memory on.
    pub fn new(
        memory: Arc<Memory>,
        store: Store,
        tender: Option<Arc<IndexTender>>,
        judge: Option<Arc<JudgeService>>,
    ) -> Arc<Self> {
        let index = tender.map(|t| Arc::new(Tender(t)) as Arc<dyn PassIndex>);
        Self::with_timing(memory, store, index, judge, Timing::default())
    }

    pub fn with_timing(
        memory: Arc<Memory>,
        store: Store,
        index: Option<Arc<dyn PassIndex>>,
        judge: Option<Arc<JudgeService>>,
        timing: Timing,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            memory,
            store,
            index: RwLock::new(index),
            judge,
            tx: OnceLock::new(),
            done: Mutex::new(BTreeMap::new()),
            turns: Arc::default(),
            timing,
            me: me.clone(),
        })
    }

    /// The daemon's running turns: every turn counts itself here
    /// (`TurnRunner::run`), so that the pass writes only between them.
    pub fn turns(&self) -> &Arc<turns::Turns> {
        &self.turns
    }

    /// A moment between turns for one frame of another writer's
    /// (consolidation, 31b), by the pass's clocks: the frame is written
    /// while the guard lives.
    pub async fn writing(&self) -> turns::Writing {
        self.turns
            .between(tokio::time::Instant::now(), &self.timing)
            .await
    }

    /// Ask `index` instead of the tender (tests: a stand-in).
    pub fn set_index(&self, index: Arc<dyn PassIndex>) {
        *self.index.write().unwrap_or_else(PoisonError::into_inner) = Some(index);
    }

    /// A turn ended: its session goes to the pass's task. Returns at once.
    pub fn after_turn(&self, res: &theseus_protocol::TurnSubmitResult) {
        self.ended(&res.session_id, &res.turn_id);
    }

    /// `turn_id` of `session_id` ended.
    pub fn ended(&self, session_id: &str, turn_id: &str) {
        if !self.memory.on() {
            return;
        }
        let Some(tx) = self.sender() else { return };
        let _ = tx.send(Job {
            session_id: session_id.into(),
            turn_id: turn_id.into(),
        });
    }

    /// The task's queue, started by the first turn that ends.
    fn sender(&self) -> Option<&mpsc::UnboundedSender<Job>> {
        if let Some(tx) = self.tx.get() {
            return Some(tx);
        }
        let rt = tokio::runtime::Handle::try_current().ok()?;
        let (tx, rx) = mpsc::unbounded_channel();
        if self.tx.set(tx).is_ok() {
            rt.spawn(run(rx, self.me.clone()));
        }
        self.tx.get()
    }

    fn index(&self) -> Option<Arc<dyn PassIndex>> {
        self.index
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// The pass's task: each ended turn's session, and a frame per batch.
async fn run(mut rx: mpsc::UnboundedReceiver<Job>, me: Weak<MemoryPass>) {
    let mut batch: Vec<Unit> = Vec::new();
    let mut since: Option<tokio::time::Instant> = None;
    loop {
        let job = match since {
            None => rx.recv().await,
            Some(t0) => {
                let window = match me.upgrade() {
                    Some(p) => p.timing.window,
                    None => return,
                };
                match tokio::time::timeout_at(t0 + window, rx.recv()).await {
                    Ok(j) => j,
                    Err(_) => {
                        let Some(p) = me.upgrade() else { return };
                        p.flush(&mut batch, true).await;
                        since = None;
                        continue;
                    }
                }
            }
        };
        let Some(job) = job else {
            if let Some(p) = me.upgrade() {
                p.flush(&mut batch, true).await;
            }
            return;
        };
        let Some(p) = me.upgrade() else { return };
        let units = p.session(&job).await;
        if !units.is_empty() && since.is_none() {
            since = Some(tokio::time::Instant::now());
        }
        batch.extend(units);
        if batch.len() >= MAX_NODES {
            p.flush(&mut batch, false).await;
            since = (!batch.is_empty()).then(tokio::time::Instant::now);
        }
    }
}

impl MemoryPass {
    /// Write the batch: full frames of [`MAX_NODES`], and, when `all`, the
    /// rest. Each frame is written between turns (`turns`), the bounds
    /// counted from the first frame's wait.
    async fn flush(&self, batch: &mut Vec<Unit>, all: bool) {
        let since = tokio::time::Instant::now();
        while batch.len() >= MAX_NODES || (all && !batch.is_empty()) {
            let n = batch.len().min(MAX_NODES);
            let units: Vec<Unit> = batch.drain(..n).collect();
            let records: Vec<NewRecord> = units.iter().flat_map(|u| u.records.clone()).collect();
            let between = self.turns.between(since, &self.timing).await;
            let written = theseus_store::blocking(|| self.store.append(&records));
            drop(between);
            if let Ok(positions) = &written {
                self.memory.retention_written(&records, positions);
            }
            if let Err(e) = written {
                tracing::warn!(error = %format!("{e:#}"), nodes = units.len(),
                    "memory: the pass's frame was not written; its nodes wait for their sessions' next pass");
                let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
                for u in units {
                    if let Some(d) = done.get_mut(&u.session_id) {
                        if let Some(n) = &u.labeled {
                            d.labeled.remove(n);
                        }
                        if let Some(k) = &u.used {
                            d.used.remove(k);
                        }
                    }
                }
            }
        }
    }

    /// What a session's pass has done, read once from its scopes.
    fn load_done(&self, session_id: &str) {
        if self
            .done
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(session_id)
        {
            return;
        }
        let mut d = Done::default();
        let read = |scope: &str, kind: LedgerKind, d: &mut Done| -> anyhow::Result<()> {
            for r in self.store.scope_after(scope, 0)? {
                if r.kind != kinds::LEDGER {
                    continue;
                }
                let row: LedgerRow = r.decode()?;
                if row.kind != kind.as_str() {
                    continue;
                }
                let node = row.data["node_id"].as_str().unwrap_or_default().to_string();
                match kind {
                    LedgerKind::MemoryLabeled => {
                        d.labeled.insert(node);
                    }
                    _ => {
                        let recall = row.data["recall_id"].as_str().unwrap_or_default();
                        d.used.insert((recall.to_string(), node));
                    }
                }
            }
            Ok(())
        };
        let r = theseus_store::blocking(|| {
            read(&scope(session_id), LedgerKind::MemoryLabeled, &mut d)?;
            read(
                &crate::fact::recall::scope(session_id),
                LedgerKind::MemoryUsed,
                &mut d,
            )
        });
        if let Err(e) = r {
            tracing::warn!(session_id, error = %format!("{e:#}"), "memory: what the pass did cannot be read; it waits for the next turn");
            return;
        }
        self.done
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(session_id.to_string())
            .or_insert(d);
    }

    /// One session's pass: its unlabeled eligible nodes, labeled and gated,
    /// and its recalls' items attributed.
    async fn session(&self, job: &Job) -> Vec<Unit> {
        let sid = job.session_id.as_str();
        self.load_done(sid);
        let nodes = match theseus_store::blocking(|| self.store.transcript(sid)) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(session_id = sid, error = %format!("{e:#}"), "memory: the session cannot be read; it waits for its next pass");
                return Vec::new();
            }
        };
        let (todo, waiting) = {
            let done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(d) = done.get(sid) else {
                return Vec::new();
            };
            let todo: Vec<(u64, crate::stub::Stub)> = nodes
                .iter()
                .filter(|(_, n)| !d.labeled.contains(&n.id) && kept_kind(n.kind) && eligible(n))
                .cloned()
                .collect();
            (todo, recalls::waiting(&nodes, &job.turn_id, d))
        };
        let recalls = self.resolve(&nodes, waiting);
        if todo.is_empty() && recalls.is_empty() {
            return Vec::new();
        }
        tracing::debug!(session_id = sid, turn_id = %job.turn_id, nodes = todo.len(), "memory: a pass");
        let index = self.index();
        // Every text whose entities the pass needs, asked of the tender at
        // once: the nodes', then each recall's.
        let mut texts: Vec<String> = todo.iter().map(|(_, n)| text_of(n)).collect();
        for r in &recalls {
            r.texts(&mut texts);
        }
        let (entities, unavailable) = entities(index.as_deref(), texts).await;
        let mut units = Vec::new();
        let mut gate = Gate::new(self, index.clone());
        for (i, (pos, n)) in todo.iter().enumerate() {
            let about = entities.get(i).cloned().unwrap_or_default();
            if let Some(u) = self
                .node_unit(&mut gate, &nodes, *pos, n, about, unavailable.as_deref())
                .await
            {
                units.push(u);
            }
        }
        let mut at = todo.len();
        for r in &recalls {
            if r.turn_id.as_deref() == Some(job.turn_id.as_str()) {
                self.ask_attribution(r);
            }
            units.extend(recalls::units(
                r,
                &entities,
                &mut at,
                unavailable.as_deref(),
            ));
        }
        self.mark(sid, &units);
        units
    }

    /// The session's `Done` holds `units` from now: a later pass leaves them.
    fn mark(&self, sid: &str, units: &[Unit]) {
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(d) = done.get_mut(sid) else { return };
        for u in units {
            if let Some(n) = &u.labeled {
                d.labeled.insert(n.clone());
            }
            if let Some(k) = &u.used {
                d.used.insert(k.clone());
            }
        }
    }

    /// A node's labels, its gate's decision, and its edge: its unit. `None`:
    /// the tender has not embedded it yet; it waits for the next pass.
    async fn node_unit(
        &self,
        gate: &mut Gate<'_>,
        nodes: &Transcript,
        position: u64,
        n: &Node,
        about: Vec<String>,
        unavailable: Option<&str>,
    ) -> Option<Unit> {
        let (shape, external) = shape_of(n, nodes)?;
        let text = text_of(n);
        let l = labels::label(shape, n.origin, &text, about, external);
        let same_turn: BTreeSet<&str> = nodes
            .iter()
            .filter(|(_, m)| m.turn_id.is_some() && m.turn_id == n.turn_id)
            .map(|(_, m)| m.id.as_str())
            .collect();
        let gated = gate.of(n, position, l.correction, &same_turn).await?;
        self.ask_memory(nodes, n, &text);
        let sid = n.session_id.as_str();
        let turn = n.turn_id.as_deref();
        let body = n.kind_str();
        let mut records = Vec::new();
        let labeled = MemoryLabeled {
            node_id: &n.id,
            position,
            body,
            labels: &l,
            entities_unavailable: unavailable,
        };
        let science = self.memory.science();
        let id = science.id().to_string();
        let (merge, supersede) = science.gate_thresholds();
        let (decision, to, why) = match &gated {
            Gated::Decided(d, _) => match d {
                GateDecision::Store => ("store", None, None),
                GateDecision::MergeInto(t) => ("same_entity", Some(t.as_str()), None),
                GateDecision::Supersedes(t) => ("supersedes", Some(t.as_str()), None),
            },
            Gated::Unavailable(why) => ("unavailable", None, Some(why.as_str())),
        };
        let seen = match &gated {
            Gated::Decided(_, s) => s.as_slice(),
            Gated::Unavailable(_) => &[],
        };
        let g = MemoryGated {
            node_id: &n.id,
            decision,
            to,
            correction: l.correction,
            neighbours: seen,
            science: &id,
            merge_cosine: merge,
            supersede_cosine: supersede,
            why,
        };
        for r in [
            crate::fact::row(&labeled, Some(sid), turn),
            crate::fact::row(&g, Some(sid), turn),
        ] {
            match r {
                Ok(mut r) => {
                    r.key = Some(n.id.clone());
                    records.push(r.scoped(&scope(sid)));
                }
                Err(e) => {
                    tracing::warn!(error = %format!("{e:#}"), "memory: a row cannot be encoded");
                    return None;
                }
            }
        }
        if let Gated::Decided(d, _) = &gated {
            let edge = match d {
                GateDecision::Store => None,
                GateDecision::MergeInto(t) => {
                    Some(Edge::new(EdgeKind::SameEntity, &n.id, t, VIA_MEMORY))
                }
                GateDecision::Supersedes(t) => {
                    Some(Edge::new(EdgeKind::Supersedes, &n.id, t, VIA_MEMORY))
                }
            };
            if let Some(e) = edge {
                records.push(e.record().ok()?);
            }
        }
        Some(Unit {
            session_id: sid.to_string(),
            records,
            labeled: Some(n.id.clone()),
            used: None,
        })
    }
}

impl MemoryPass {
    /// `memory.v1`, in shadow, of a node the pass labels: its text as the
    /// model saw it, who wrote it, and the message before it.
    fn ask_memory(&self, nodes: &Transcript, n: &Node, text: &str) {
        let Some(judge) = &self.judge else { return };
        let role = match (&n.body, n.origin) {
            (Body::UserMessage { .. }, Origin::Operator) => "operator",
            (Body::UserMessage { .. }, _) => "relay",
            (Body::AssistantMessage { .. }, _) => "assistant",
            (Body::Summary { .. }, _) => "summary",
            _ => "tool",
        };
        let tool = match &n.body {
            Body::ToolResult { tool, .. } => Some(tool.clone()),
            _ => None,
        };
        let at = nodes.iter().position(|(_, m)| m.id == n.id).unwrap_or(0);
        let previous = nodes[..at].iter().rev().find_map(|(_, m)| match &m.body {
            Body::UserMessage { .. } | Body::AssistantMessage { .. } if eligible(m) => {
                Some(text_of(m))
            }
            _ => None,
        });
        judge.at_memory_pass(MemoryAsk::Node {
            session_id: n.session_id.clone(),
            turn_id: n.turn_id.clone(),
            node_id: n.id.clone(),
            input: theseus_judge::builders::MemoryInput {
                role: role.into(),
                tool,
                text: text.to_string(),
                previous,
            },
        });
    }

    /// `attribution.v1`, in shadow, of a recall whose turn has its reply:
    /// the operator's message, the reply, and each note's excerpt.
    fn ask_attribution(&self, r: &recalls::Pending) {
        let Some(judge) = &self.judge else { return };
        if r.reply.trim().is_empty() {
            return;
        }
        judge.at_memory_pass(MemoryAsk::Recall {
            session_id: r.session_id.clone(),
            turn_id: r.turn_id.clone(),
            recall_id: r.recall_id.clone(),
            input: theseus_judge::builders::AttributionInput {
                ask: r.ask.clone(),
                reply: r.reply.clone(),
                notes: r
                    .items
                    .iter()
                    .map(|(item, excerpt)| theseus_judge::builders::NoteInput {
                        id: item.node_id.clone(),
                        excerpt: excerpt.clone(),
                    })
                    .collect(),
            },
        });
    }
}

/// What the gate made of a node.
enum Gated {
    /// The science's decision, and the neighbours it saw.
    Decided(GateDecision, Vec<Seen>),
    /// No vectors to ask, and why: the row says it, and no edge is written.
    Unavailable(String),
}

/// The gate of one pass: its wait budget, and what the index's state is.
struct Gate<'a> {
    pass: &'a MemoryPass,
    index: Option<Arc<dyn PassIndex>>,
    /// The pass's remaining wait for vectors.
    left: Duration,
    /// The index's mode, once read.
    mode: Option<String>,
}

impl<'a> Gate<'a> {
    fn new(pass: &'a MemoryPass, index: Option<Arc<dyn PassIndex>>) -> Self {
        Self {
            pass,
            index,
            left: pass.timing.wait,
            mode: None,
        }
    }

    /// The decision for `n`, at `position`; `None`: its vector is not there
    /// yet, and the pass's wait is spent.
    async fn of(
        &mut self,
        n: &Node,
        position: u64,
        correction: bool,
        same_turn: &BTreeSet<&str>,
    ) -> Option<Gated> {
        let Some(index) = self.index.clone() else {
            return Some(Gated::Unavailable("no index is configured".into()));
        };
        let p = IndexNeighboursParams {
            node_id: n.id.clone(),
            k: K,
            as_of: Some(position),
        };
        let mut wait = self.pass.timing.wait_first;
        loop {
            match index.neighbours(p.clone()).await {
                Ok(r) => {
                    let seen: Vec<Seen> = r
                        .neighbours
                        .into_iter()
                        .filter(|x| GATED_KINDS.contains(&x.kind.as_str()))
                        .filter(|x| !same_turn.contains(x.node_id.as_str()))
                        .map(|x| Seen {
                            node_id: x.node_id,
                            session_id: x.session_id,
                            kind: x.kind,
                            cosine: x.score,
                        })
                        .collect();
                    let near: Vec<Neighbour> = seen
                        .iter()
                        .map(|s| Neighbour {
                            node_id: s.node_id.clone(),
                            cosine: s.cosine as f32,
                        })
                        .collect();
                    let fresh = Fresh {
                        node_id: n.id.clone(),
                        correction,
                    };
                    let d = self.pass.memory.science().gate(&fresh, &near);
                    return Some(Gated::Decided(d, seen));
                }
                Err(TenderMiss::Down(why)) => {
                    return Some(Gated::Unavailable(format!("no index answered: {why}")))
                }
                Err(TenderMiss::Refused(why)) => {
                    if self.mode.is_none() {
                        self.mode = Some(index.mode().await.unwrap_or_default());
                    }
                    if self.mode.as_deref() == Some("bm25_only") {
                        return Some(Gated::Unavailable(
                            "the index answers BM25 alone: it has no model files ([index] weights_dir), so no vectors"
                                .into(),
                        ));
                    }
                    if wait > self.left {
                        tracing::debug!(node_id = %n.id, why, "memory: no vector yet; the node waits for the next pass");
                        return None;
                    }
                    tokio::time::sleep(wait).await;
                    self.left -= wait;
                    wait *= 2;
                }
            }
        }
    }
}

/// The kinds the gate weighs as neighbours: those the pass labels.
const GATED_KINDS: &[&str] = &[
    "user_message",
    "assistant_message",
    "tool_result",
    "summary",
];

/// Each text's entities from the tender, in order, in requests of at most
/// its limit; with why there are none, if the tender could not name them.
async fn entities(
    index: Option<&dyn PassIndex>,
    texts: Vec<String>,
) -> (Vec<Vec<String>>, Option<String>) {
    let Some(index) = index else {
        return (Vec::new(), Some("no index is configured".into()));
    };
    let mut out = Vec::with_capacity(texts.len());
    for part in texts.chunks(IndexEntitiesParams::MAX_TEXTS) {
        match index.entities(part.to_vec()).await {
            Ok(e) => out.extend(e),
            Err(TenderMiss::Down(why) | TenderMiss::Refused(why)) => {
                return (Vec::new(), Some(why))
            }
        }
    }
    (out, None)
}

/// Whether the pass labels `n` (§2.6, §5.2): the operator's and the
/// agent's messages, tool results, and compaction's summaries, which the
/// harness writes (decision 10). Never a recall (the recursion exclusion,
/// whoever wrote it), a task's arrangement (its sources' text), another
/// harness line, or a tool call.
pub fn eligible(n: &Node) -> bool {
    match &n.body {
        // A synthesis is left unlabeled (31b): its gate would mark it
        // `same_entity` with its sources, and `baseline` would drop them for it.
        Body::ToolCall { .. }
        | Body::Recall { .. }
        | Body::Arrangement { .. }
        | Body::Synthesis { .. }
        | Body::Erased { .. } => false,
        // The operator's past history (theseus-0lrr.6): labeled as any
        // message is, when the pass reads an imported session.
        Body::Summary { .. } | Body::Imported { .. } | Body::ImportedSummary { .. } => {
            !text_of(n).trim().is_empty()
        }
        Body::UserMessage { .. } | Body::AssistantMessage { .. } | Body::ToolResult { .. } => {
            n.origin != Origin::Harness && !text_of(n).trim().is_empty()
        }
    }
}

/// Whether a node of `kind` may be eligible, read from its stub before its
/// body is (step 33): what `eligible` never keeps stays undecoded.
fn kept_kind(kind: crate::stub::Kind) -> bool {
    use crate::stub::Kind;
    !matches!(
        kind,
        Kind::ToolCall | Kind::Recall | Kind::Arrangement | Kind::Synthesis | Kind::Erased
    )
}

/// The labeler's shape of an eligible node, and DD5's flag: a summary's is
/// its range's, whose external text it may restate.
fn shape_of(n: &Node, nodes: &Transcript) -> Option<(Shape, bool)> {
    match &n.body {
        Body::UserMessage { .. } => Some((Shape::Message, false)),
        Body::AssistantMessage { .. } => Some((Shape::Reply, false)),
        Body::ToolResult { external, .. } => Some((Shape::Result, external.is_some())),
        Body::Summary { first, last, .. } => {
            Some((Shape::Summary, external_in(nodes, *first, *last)))
        }
        Body::Imported { integrity, .. } => Some((
            Shape::Message,
            *integrity == crate::import::Integrity::Outside,
        )),
        Body::ImportedSummary { .. } => Some((Shape::Message, false)),
        Body::ToolCall { .. }
        | Body::Recall { .. }
        | Body::Arrangement { .. }
        | Body::Synthesis { .. }
        | Body::Erased { .. } => None,
    }
}

/// Whether a tool result at a WAL position from `first` to `last` came from
/// outside (DD5). A folded summary's range holds its own range too.
fn external_in(nodes: &Transcript, first: u64, last: u64) -> bool {
    let in_range = nodes
        .iter()
        .filter(|(p, m)| (first..=last).contains(p) && m.kind == crate::stub::Kind::ToolResult);
    in_range.into_iter().any(|(_, m)| match &m.body {
        Body::ToolResult { external, .. } => external.is_some(),
        _ => false,
    })
}

fn text_of(n: &Node) -> String {
    crate::recall::text_of(n)
}
