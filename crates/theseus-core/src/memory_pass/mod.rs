//! The memory pass (M6 step 31a; design §2.6): after a turn ends, off its
//! path, it labels the session's new nodes.
//!
//! - **When.** One call beside the judge's `after_turn` hands the session to
//!   the pass's task and returns ([`MemoryPass::after_turn`]); everything
//!   else is the task's. With `[memory] mode = "off"` it does nothing.
//! - **Who is eligible** ([`eligible`]): the operator's and the agent's
//!   messages, and tool results. Never a `Recall` node (§5.2's recursion
//!   exclusion), a harness line, a tool call; judgments, manifests, and
//!   ledger rows are not nodes. (30c's `Summary` joins at its merge.)
//! - **Labels** (`labels`): §2.6's table, deterministic; `about` is the
//!   index's entity field, asked of the tender (`index.entities`), the one
//!   extractor.
//! - **Frames.** The rows and edges go in the pass's own frames: one per
//!   [`MAX_NODES`] nodes, or [`WINDOW`] after the first waiting, whichever
//!   comes first; a node's records never split across frames. A frame waits
//!   for the WAL to be still for [`QUIET`] (at most [`QUIET_BOUND`]), so the
//!   pass yields to turns, and none lands inside one being measured
//!   (theseus-0j2.3's lesson).
//! - **Crash.** What is done is read from the record: a session's
//!   `memory.labeled` rows (scoped `memory:<session>`), read once a session
//!   by this daemon. A node a
//!   crash left unlabeled waits for its session's next pass; nothing scans
//!   on the start path.

pub mod labels;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, RwLock, Weak};
use std::time::Duration;

use theseus_protocol::index::{
    method, IndexEntitiesParams, IndexEntitiesResult, IndexNeighboursParams, IndexNeighboursResult,
};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord};
use tokio::sync::mpsc;

use crate::fact::memory::{scope, MemoryLabeled};
use crate::ledger::LedgerRow;
use crate::node::{Body, Node, Origin};
use crate::recall::Memory;
use crate::store::Store;
use crate::tender::{IndexTender, TenderMiss};
use labels::Shape;

/// A frame holds at most this many nodes' records.
pub const MAX_NODES: usize = 32;
/// A frame is written at most this long after its first node waits.
pub const WINDOW: Duration = Duration::from_secs(2);
/// How long the WAL must be still before the pass writes.
pub const QUIET: Duration = Duration::from_millis(500);
/// The pass writes anyway after waiting this long for a still WAL.
pub const QUIET_BOUND: Duration = Duration::from_secs(60);
/// What the pass asks of the index: the tender, or a test's stand-in.
pub type IndexFuture<T> = Pin<Box<dyn Future<Output = Result<T, TenderMiss>> + Send>>;

pub trait PassIndex: Send + Sync {
    /// `index.neighbours`.
    fn neighbours(&self, p: IndexNeighboursParams) -> IndexFuture<IndexNeighboursResult>;
    /// `index.entities`.
    fn entities(&self, texts: Vec<String>) -> IndexFuture<Vec<Vec<String>>>;
    /// The index's state (`ready`, `bm25_only`, …), read when a neighbour
    /// is refused.
    fn state(&self) -> IndexFuture<String>;
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

    fn state(&self) -> IndexFuture<String> {
        let t = self.0.clone();
        Box::pin(async move { Ok(t.health(crate::tender::STATUS_DEADLINE).await.state) })
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
}

/// One node's records: written together, in one frame.
struct Unit {
    session_id: String,
    records: Vec<NewRecord>,
    /// What the session's `Done` holds for it, released if its frame fails.
    labeled: String,
}

pub struct MemoryPass {
    memory: Arc<Memory>,
    store: Store,
    index: RwLock<Option<Arc<dyn PassIndex>>>,
    tx: OnceLock<mpsc::UnboundedSender<Job>>,
    done: Mutex<BTreeMap<String, Done>>,
    timing: Timing,
    me: Weak<MemoryPass>,
}

/// The pass's clocks (tests shorten them).
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub window: Duration,
    pub quiet: Duration,
    pub quiet_bound: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            window: WINDOW,
            quiet: QUIET,
            quiet_bound: QUIET_BOUND,
        }
    }
}

impl MemoryPass {
    /// The pass, built with the core: nothing read or started until a turn
    /// ends with memory on.
    pub fn new(memory: Arc<Memory>, store: Store, tender: Option<Arc<IndexTender>>) -> Arc<Self> {
        let index = tender.map(|t| Arc::new(Tender(t)) as Arc<dyn PassIndex>);
        Self::with_timing(memory, store, index, Timing::default())
    }

    pub fn with_timing(
        memory: Arc<Memory>,
        store: Store,
        index: Option<Arc<dyn PassIndex>>,
        timing: Timing,
    ) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            memory,
            store,
            index: RwLock::new(index),
            tx: OnceLock::new(),
            done: Mutex::new(BTreeMap::new()),
            timing,
            me: me.clone(),
        })
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
    /// rest. Each frame waits for a still WAL first.
    async fn flush(&self, batch: &mut Vec<Unit>, all: bool) {
        while batch.len() >= MAX_NODES || (all && !batch.is_empty()) {
            let n = batch.len().min(MAX_NODES);
            let units: Vec<Unit> = batch.drain(..n).collect();
            self.quiet().await;
            let records: Vec<NewRecord> = units.iter().flat_map(|u| u.records.clone()).collect();
            let written = theseus_store::blocking(|| self.store.append(&records));
            if let Err(e) = written {
                tracing::warn!(error = %format!("{e:#}"), nodes = units.len(),
                    "memory: the pass's frame was not written; its nodes wait for their sessions' next pass");
                let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
                for u in units {
                    if let Some(d) = done.get_mut(&u.session_id) {
                        d.labeled.remove(&u.labeled);
                    }
                }
            }
        }
    }

    /// Wait for the WAL to be still for `quiet`, at most `quiet_bound`.
    async fn quiet(&self) {
        let t0 = tokio::time::Instant::now();
        loop {
            let at = self.store.last_position();
            tokio::time::sleep(self.timing.quiet).await;
            if self.store.last_position() == at || t0.elapsed() >= self.timing.quiet_bound {
                return;
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
        let r = theseus_store::blocking(|| -> anyhow::Result<()> {
            for r in self.store.scope_after(&scope(session_id), 0)? {
                if r.kind != kinds::LEDGER {
                    continue;
                }
                let row: LedgerRow = r.decode()?;
                if row.kind == LedgerKind::MemoryLabeled.as_str() {
                    let node = row.data["node_id"].as_str().unwrap_or_default();
                    d.labeled.insert(node.to_string());
                }
            }
            Ok(())
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

    /// One session's pass: its unlabeled eligible nodes, labeled.
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
        let todo: Vec<(u64, Arc<Node>)> = {
            let done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(d) = done.get(sid) else {
                return Vec::new();
            };
            nodes
                .iter()
                .filter(|(_, n)| eligible(n) && !d.labeled.contains(&n.id))
                .cloned()
                .collect()
        };
        if todo.is_empty() {
            return Vec::new();
        }
        tracing::debug!(session_id = sid, turn_id = %job.turn_id, nodes = todo.len(), "memory: a pass");
        let index = self.index();
        // Every node's entities, asked of the tender at once.
        let texts: Vec<String> = todo.iter().map(|(_, n)| text_of(n)).collect();
        let (entities, unavailable) = entities(index.as_deref(), texts).await;
        let mut units = Vec::new();
        for (i, (pos, n)) in todo.iter().enumerate() {
            let about = entities.get(i).cloned().unwrap_or_default();
            units.extend(node_unit(*pos, n, about, unavailable.as_deref()));
        }
        let mut done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(d) = done.get_mut(sid) {
            for u in &units {
                d.labeled.insert(u.labeled.clone());
            }
        }
        units
    }
}

/// A node's labels: its unit.
fn node_unit(
    position: u64,
    n: &Node,
    about: Vec<String>,
    unavailable: Option<&str>,
) -> Option<Unit> {
    let (shape, external) = shape_of(n)?;
    let text = text_of(n);
    let l = labels::label(shape, n.origin, &text, about, external);
    let sid = n.session_id.as_str();
    let labeled = MemoryLabeled {
        node_id: &n.id,
        position,
        body: n.kind_str(),
        labels: &l,
        entities_unavailable: unavailable,
    };
    let mut r = crate::fact::row(&labeled, Some(sid), n.turn_id.as_deref())
        .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "memory: a row cannot be encoded"))
        .ok()?;
    r.key = Some(n.id.clone());
    Some(Unit {
        session_id: sid.to_string(),
        records: vec![r.scoped(&scope(sid))],
        labeled: n.id.clone(),
    })
}

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
/// agent's messages, and tool results. Never a recall (the recursion
/// exclusion, whoever wrote it), a harness line, or a tool call.
pub fn eligible(n: &Node) -> bool {
    match &n.body {
        Body::ToolCall { .. } | Body::Recall { .. } => false,
        Body::UserMessage { .. } | Body::AssistantMessage { .. } | Body::ToolResult { .. } => {
            n.origin != Origin::Harness && !text_of(n).trim().is_empty()
        }
    }
}

/// The labeler's shape of an eligible node, and DD5's flag.
fn shape_of(n: &Node) -> Option<(Shape, bool)> {
    match &n.body {
        Body::UserMessage { .. } => Some((Shape::Message, false)),
        Body::AssistantMessage { .. } => Some((Shape::Reply, false)),
        Body::ToolResult { external, .. } => Some((Shape::Result, external.is_some())),
        Body::ToolCall { .. } | Body::Recall { .. } => None,
    }
}

fn text_of(n: &Node) -> String {
    crate::recall::text_of(n)
}
