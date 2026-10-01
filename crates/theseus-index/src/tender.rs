//! The tender: follow the WAL, index each node, commit, write the cursor;
//! wait on inotify with a backstop; answer on the socket meanwhile.
//!
//! - **Idempotent by node id**: a node is replaced whole (delete, then add),
//!   and the cursor is written after each commit, so a kill between the two
//!   re-indexes that batch and leaves one copy. A batch without nodes moves
//!   the cursor in memory and writes it lazily (with the next commit, a
//!   changed place, or the backstop): a kill re-reads those frames.
//! - **Rebuilt, never repaired**: another build's index (its schema or its
//!   extractor), a missing or unreadable cursor, a WAL that no longer holds
//!   what the cursor was taken on, or `index.rebuild` each drop the index and
//!   backfill from the WAL's start.
//! - **Stalled, never past a bad frame**: a WAL the follower cannot read on
//!   from (a frame that checks but is wrong) leaves the index where it is,
//!   says so in its status, and tries again at each wake.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use theseus_follow::{Cursor, FollowError, Kicker, Stop, Waker, WalFollower};
use theseus_store::{kinds, wal, Record};

use crate::engine::{self, Engine, Writer, SCHEMA_VERSION, SOURCES};
use crate::extract::{extract, Extract, EXTRACTOR_VERSION};
use crate::proto::{
    Backfill, ChunkRef, EmbedParams, EmbedResult, ForgetParams, ForgetResult, IndexStatus, Lag,
    NeighboursParams, NeighboursResult, QueryParams, QueryResult, WarmResult, Weights,
};
use crate::state::{self, Lock, Paths, Places, Saved, CURSOR_FORMAT};
use crate::vectors::{text_hash, NodeChunks, VectorConfig, Vectors};

/// How long `index.forget` waits for the ingest thread.
const FORGET_WAIT: Duration = Duration::from_secs(60);

/// A queued `index.forget`, and where its answer goes.
type Forget = (ForgetParams, mpsc::Sender<Result<ForgetResult, String>>);

/// The core's META keys that say where a session lives
/// (`theseus_core::outbox`; this crate's tests hold them equal): a Discord
/// place's session, and a task's report target.
pub const PLACE_META_PREFIX: &str = "discord.session.";
pub const TASK_META_PREFIX: &str = "task.place.";

#[derive(Debug, Clone)]
pub struct Config {
    /// The store's directory (its WAL is `wal/` inside).
    pub store_dir: PathBuf,
    /// The index's directory, `<state>/index`.
    pub index_dir: PathBuf,
    /// About this many bytes of WAL per batch, and so per commit.
    pub batch_bytes: usize,
    /// The longest a caught-up tender sleeps without an event.
    pub backstop: Duration,
    /// The vector side: off unless given a weights directory.
    pub vectors: VectorConfig,
    /// The fusion's weights by source, unless a query names its own.
    pub weights: Weights,
}

impl Config {
    pub fn new(store_dir: &Path, index_dir: &Path) -> Self {
        Self {
            store_dir: store_dir.to_path_buf(),
            index_dir: index_dir.to_path_buf(),
            batch_bytes: 4 << 20,
            backstop: Duration::from_secs(60),
            vectors: VectorConfig::off(),
            weights: Weights::default(),
        }
    }

    pub fn wal_dir(&self) -> PathBuf {
        self.store_dir.join("wal")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("another tender holds {0}")]
    Held(PathBuf),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// What the ingest thread, the embedding thread, and the socket's threads
/// share.
pub struct Shared {
    pub engine: Engine,
    pub vectors: Vectors,
    /// The fusion's default weights.
    pub weights: Weights,
    /// `index.forget` calls waiting for the ingest thread, which owns the
    /// index's writer.
    forgets: Mutex<Vec<Forget>>,
    status: Mutex<IndexStatus>,
    /// When the index last caught up with the WAL (ms since the epoch).
    caught_up_ms: Mutex<u64>,
    rebuild: AtomicBool,
    stop: AtomicBool,
    kicker: Kicker,
    wal_dir: PathBuf,
}

impl Shared {
    /// Health's `index` block, with its lag and counts read now.
    pub fn status(&self) -> IndexStatus {
        let mut s = self.status.lock().unwrap().clone();
        s.lag = self.lag(s.segment, s.offset);
        if let Ok((docs, nodes)) = self.engine.counts() {
            s.documents = docs;
            s.nodes = nodes;
        }
        s.mode = self.vectors.mode().into();
        s.vectors = Some(self.vectors.status());
        s.weights = Some(self.weights);
        s.rss_bytes = rss_bytes();
        s
    }

    /// The WAL's bytes after the cursor, and how long the index has been
    /// behind them.
    fn lag(&self, segment: u32, offset: u64) -> Lag {
        let bytes = wal_bytes_after(&self.wal_dir, segment, offset);
        let ms = if bytes == 0 {
            0
        } else {
            now_ms().saturating_sub(*self.caught_up_ms.lock().unwrap())
        };
        Lag { bytes, ms }
    }

    /// `index.query`: the sources it names (by default BM25 and entities,
    /// and vectors in `hybrid` mode), fused with the query's weights over the
    /// tender's. A vector source that cannot answer (the model loading, no
    /// weights) is left out and named in `skipped`; the others still answer.
    pub fn query(&self, p: &QueryParams) -> anyhow::Result<QueryResult> {
        let t0 = Instant::now();
        let weights = self.weights.with(&p.weights).map_err(anyhow::Error::msg)?;
        let mut wanted: Vec<&str> = if p.sources.is_empty() {
            let mut v = vec!["bm25", "entity"];
            if self.vectors.mode() == "hybrid" {
                v.push("vector");
            }
            v
        } else {
            p.sources.iter().map(String::as_str).collect()
        };
        if let Some(bad) = wanted.iter().find(|s| !SOURCES.contains(s)) {
            anyhow::bail!(
                "no source {bad:?}: this tender answers {}",
                SOURCES.join(", ")
            );
        }
        wanted.dedup();
        let mut skipped = BTreeMap::new();
        let mut vector = None;
        let (mut embed_ms, mut scan_ms) = (0.0, 0.0);
        if wanted.contains(&"vector") {
            match self.vectors.search(
                &p.text,
                p.as_of,
                &p.exclude_sessions,
                &p.filters,
                engine::fetch_for(p.k),
                Duration::from_millis(p.wait_ms),
            ) {
                Ok((hits, e, s)) => {
                    vector = Some(hits);
                    (embed_ms, scan_ms) = (e, s);
                }
                Err(why) => {
                    skipped.insert("vector".to_string(), why);
                }
            }
        }
        let (hits, mut timings) = self.engine.query(p, &wanted, vector.as_deref(), &weights)?;
        timings.embed_ms = embed_ms;
        timings.vector_ms += scan_ms;
        timings.total_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let (position, segment, offset) = {
            let s = self.status.lock().unwrap();
            (s.position, s.segment, s.offset)
        };
        let fused: Vec<&str> = wanted
            .iter()
            .copied()
            .filter(|s| !skipped.contains_key(*s))
            .collect();
        Ok(QueryResult {
            hits,
            indexed_through: position,
            lag: self.lag(segment, offset),
            timings,
            skipped,
            weights: weights.of(&fused),
        })
    }

    /// `index.neighbours`.
    pub fn neighbours(&self, p: &NeighboursParams) -> anyhow::Result<NeighboursResult> {
        let (neighbours, vector_ms) = self
            .vectors
            .neighbours(&p.node_id, p.k, p.as_of)
            .map_err(anyhow::Error::msg)?;
        Ok(NeighboursResult {
            node_id: p.node_id.clone(),
            neighbours,
            vector_ms,
        })
    }

    /// `index.embed`.
    pub fn embed(&self, p: &EmbedParams) -> anyhow::Result<EmbedResult> {
        self.vectors.embed(p)
    }

    /// `index.warm`.
    pub fn warm(&self) -> WarmResult {
        WarmResult {
            model: self.vectors.warm(),
            mode: self.vectors.mode().into(),
        }
    }

    /// `index.forget` (theseus-64x): handed to the ingest thread, which owns
    /// the index's writer, and answered when it has done it (or after
    /// `FORGET_WAIT`, still queued).
    pub fn forget(&self, p: ForgetParams) -> anyhow::Result<ForgetResult> {
        let (tx, rx) = mpsc::channel();
        self.forgets.lock().unwrap().push((p, tx));
        self.kicker.kick();
        match rx.recv_timeout(FORGET_WAIT) {
            Ok(r) => r.map_err(anyhow::Error::msg),
            Err(_) => anyhow::bail!(
                "the ingest thread has not done the forget in {} s; it is still queued",
                FORGET_WAIT.as_secs()
            ),
        }
    }

    /// Drop the index and backfill: the ingest thread does it at its next
    /// wake, which this causes.
    pub fn request_rebuild(&self) {
        self.rebuild.store(true, Ordering::SeqCst);
        self.kicker.kick();
    }

    /// End [`Tender::run`] and the embedding thread (tests, and an
    /// embedding that stops them).
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.kicker.kick();
        self.vectors.request_stop();
    }

    fn set(&self, f: impl FnOnce(&mut IndexStatus)) {
        f(&mut self.status.lock().unwrap());
    }
}

pub struct Tender {
    cfg: Config,
    paths: Paths,
    _lock: Lock,
    shared: Arc<Shared>,
    writer: Writer,
    follower: WalFollower,
    waker: Waker,
    places: Places,
    places_dirty: bool,
    /// The cursor as last written.
    saved: Cursor,
    saved_at: Instant,
    /// Where the current backfill began, and the bytes it had to read.
    backfill: Option<(Cursor, u64)>,
    /// Test hook: fail right after a commit, before the cursor is written.
    #[cfg(test)]
    pub(crate) crash_after_commit: bool,
}

/// What one step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub stop: Stop,
    pub records: usize,
    pub indexed: usize,
    /// Nodes the index held that a re-ingest skipped, so they left it.
    pub removed: usize,
    pub committed: bool,
}

impl Tender {
    /// Take the index directory's lock, open (or rebuild) the index, and
    /// open the follower at its cursor. Nothing is read from the WAL yet.
    pub fn open(cfg: Config) -> Result<Self, OpenError> {
        let paths = Paths::new(&cfg.index_dir);
        paths.create().context("creating the index directory")?;
        let lock = Lock::take(&paths.lock())
            .context("taking the index's lock")?
            .ok_or_else(|| OpenError::Held(paths.dir.clone()))?;

        // The saved cursor holds only for an index this build would write.
        let had_index = paths.bm25().join("meta.json").exists();
        let saved: Option<Saved> = state::load(&paths.cursor()).filter(|s: &Saved| {
            s.format == CURSOR_FORMAT
                && s.schema == SCHEMA_VERSION
                && s.extractor == EXTRACTOR_VERSION
                && had_index
        });
        let mut why_rebuild = None;
        if saved.is_none() && (had_index || paths.cursor().exists()) {
            why_rebuild = Some("its cursor is missing or from another build".to_string());
        }
        if saved.is_none() {
            let _ = fs::remove_dir_all(paths.bm25());
            let _ = fs::remove_file(paths.places());
        }
        let (index, fields) = match engine::open_or_create(&paths.bm25()) {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "index: unreadable, so rebuilt");
                fs::remove_dir_all(paths.bm25()).ok();
                why_rebuild = Some(format!("its index did not open: {e}"));
                engine::open_or_create(&paths.bm25()).context("creating the index")?
            }
        };
        let mut cursor = match (&saved, &why_rebuild) {
            (Some(s), None) => s.cursor.clone(),
            _ => Cursor::start(),
        };
        let mut places: Places = if cursor.at_start() {
            Places::new()
        } else {
            state::load(&paths.places()).unwrap_or_default()
        };
        let mut writer = Writer::new(&index, fields).context("opening the index's writer")?;
        let engine = Engine::new(index, fields).context("opening the index's reader")?;
        let wal_dir = cfg.wal_dir();
        let follower = match WalFollower::open(&wal_dir, cursor.clone()) {
            Ok(f) => f,
            Err(FollowError::Rewound(why)) => {
                tracing::warn!(%why, "index: the WAL is not the one the cursor was taken on; rebuilding");
                writer.clear().context("clearing the index")?;
                engine.reload().ok();
                places.clear();
                cursor = Cursor::start();
                why_rebuild = Some(format!("the WAL was rewound or replaced: {why}"));
                WalFollower::open(&wal_dir, cursor.clone()).context("opening the WAL")?
            }
            Err(e) => return Err(anyhow::Error::from(e).context("opening the WAL").into()),
        };
        if cursor.at_start() {
            // An older cursor must not outlive the index it described: were
            // this index recreated empty and the tender killed before its
            // first commit, that cursor would skip every node before it.
            match fs::remove_file(paths.cursor()) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(anyhow::Error::from(e)
                        .context("removing an old cursor")
                        .into())
                }
            }
        }
        let waker = Waker::new(&wal_dir).context("watching the WAL")?;
        let total = wal_bytes_after(&wal_dir, cursor.segment, cursor.offset);
        let vectors = Vectors::new(cfg.vectors.clone(), &paths.dir);
        if cursor.at_start() {
            // Built from the WAL's start: the vector files are a rebuild's,
            // compacted to what the index holds once it has caught up.
            vectors.on_clear();
        }
        let shared = Arc::new(Shared {
            engine,
            weights: cfg.weights,
            forgets: Mutex::new(Vec::new()),
            status: Mutex::new(IndexStatus {
                mode: vectors.mode().into(),
                state: "starting".into(),
                pid: std::process::id(),
                index_dir: paths.dir.display().to_string(),
                wal_dir: wal_dir.display().to_string(),
                position: cursor.position,
                segment: cursor.segment,
                offset: cursor.offset,
                extractor: EXTRACTOR_VERSION,
                schema: SCHEMA_VERSION,
                started_at_ms: now_ms(),
                rebuilds: u64::from(why_rebuild.is_some()),
                ..IndexStatus::default()
            }),
            caught_up_ms: Mutex::new(now_ms()),
            rebuild: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            kicker: waker.kicker(),
            wal_dir,
            vectors,
        });
        match &why_rebuild {
            Some(why) => tracing::info!(%why, "index: rebuilding from the WAL's start"),
            None if cursor.at_start() => tracing::info!("index: building from the WAL's start"),
            None => tracing::info!(
                position = cursor.position,
                "index: resuming from its cursor"
            ),
        }
        Ok(Self {
            backfill: Some((cursor.clone(), total)),
            saved: cursor,
            saved_at: Instant::now(),
            cfg,
            paths,
            _lock: lock,
            shared,
            writer,
            follower,
            waker,
            places,
            places_dirty: false,
            #[cfg(test)]
            crash_after_commit: false,
        })
    }

    pub fn shared(&self) -> Arc<Shared> {
        self.shared.clone()
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn cursor(&self) -> &Cursor {
        self.follower.cursor()
    }

    /// Each session's place, as learned so far.
    pub fn places(&self) -> &Places {
        &self.places
    }

    /// Read one batch, index its nodes, commit, and write the cursor.
    pub fn step(&mut self) -> anyhow::Result<Step> {
        let batch = match self.follower.read(self.cfg.batch_bytes) {
            Ok(b) => b,
            Err(FollowError::Rewound(why)) => {
                self.rebuild(&format!("the WAL was rewound or replaced: {why}"))?;
                return Ok(Step {
                    stop: Stop::Budget,
                    records: 0,
                    indexed: 0,
                    removed: 0,
                    committed: true,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let stop = self.follower.stop().clone();
        let mut step = Step {
            stop: stop.clone(),
            records: batch.records.len(),
            indexed: 0,
            removed: 0,
            committed: false,
        };
        let mut nodes = Vec::new();
        let mut removed = Vec::new();
        let indexed = self
            .ingest(&batch.records, &mut step, &mut nodes, &mut removed)
            .and_then(|()| {
                if step.indexed + step.removed > 0 {
                    self.writer.commit().context("committing the index")?;
                }
                Ok(())
            });
        if let Err(e) = indexed {
            // Nothing of the batch counts: read it again from the cursor
            // last written.
            self.writer.rollback().ok();
            self.follower = WalFollower::open(&self.cfg.wal_dir(), self.saved.clone())?;
            return Err(e);
        }
        if step.indexed + step.removed > 0 {
            #[cfg(test)]
            if self.crash_after_commit {
                anyhow::bail!("killed between the commit and the cursor (test)");
            }
            self.save()?;
            self.shared.engine.reload().context("reloading the index")?;
            // Rows only for what a reader can now see, and none for what it
            // no longer can.
            self.shared.vectors.on_remove(&removed);
            self.shared.vectors.on_commit(&nodes);
            step.committed = true;
            self.shared.set(|s| {
                s.commits += 1;
                s.last_commit_ms = now_ms();
            });
        } else if (self.places_dirty || self.saved_at.elapsed() >= self.cfg.backstop)
            && *self.follower.cursor() != self.saved
        {
            self.save()?;
        }
        self.after_step(&stop);
        Ok(step)
    }

    fn ingest(
        &mut self,
        records: &[Record],
        step: &mut Step,
        nodes: &mut Vec<NodeChunks>,
        removed: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        let (mut skipped, mut undecodable) = (0u64, 0u64);
        for r in records {
            match r.kind {
                kinds::NODE => match extract(&r.payload) {
                    Ok(Extract::Index(e)) => {
                        let place = self.places.get(&e.session_id).map(String::as_str);
                        let chunks = self
                            .writer
                            .replace(r.position, place, &e)
                            .with_context(|| format!("indexing node {}", e.node_id))?;
                        if self.shared.vectors.enabled() {
                            nodes.retain(|n| n.node_id != e.node_id);
                            nodes.push(NodeChunks {
                                node_id: e.node_id,
                                position: r.position,
                                session: e.session_id,
                                kind: e.kind,
                                external: e.external,
                                chunks,
                            });
                        }
                        step.indexed += 1;
                    }
                    Ok(Extract::Skip { node_id, .. }) => {
                        skipped += 1;
                        // A node the index holds, written again with nothing
                        // to index (an erased payload, say), leaves it: its
                        // earlier copy is deleted, and its rows go.
                        let earlier = nodes.iter().position(|n| n.node_id == node_id);
                        if let Some(i) = earlier {
                            nodes.remove(i);
                        }
                        if earlier.is_some() || self.shared.engine.holds(&node_id)? {
                            self.writer.delete_node(&node_id);
                            removed.push(node_id);
                            step.removed += 1;
                        }
                    }
                    Err(err) => {
                        undecodable += 1;
                        tracing::warn!(position = r.position, error = %err, "index: a node record it could not read");
                    }
                },
                kinds::META => {
                    if let Some((session, place)) = place_of(r) {
                        if self.places.get(&session) != Some(&place) {
                            self.places.insert(session, place);
                            self.places_dirty = true;
                        }
                    }
                }
                _ => {}
            }
        }
        let indexed = step.indexed as u64;
        let records = records.len() as u64;
        self.shared.set(|s| {
            s.records_read += records;
            s.nodes_indexed += indexed;
            s.nodes_skipped += skipped;
            s.undecodable += undecodable;
        });
        Ok(())
    }

    /// The places (when changed), then the cursor: places.json always holds
    /// every place learned before the cursor it is saved with.
    fn save(&mut self) -> anyhow::Result<()> {
        if self.places_dirty {
            state::save(&self.paths.places(), &self.places).context("writing places.json")?;
            self.places_dirty = false;
        }
        let cursor = self.follower.cursor().clone();
        state::save(
            &self.paths.cursor(),
            &Saved {
                format: CURSOR_FORMAT,
                schema: SCHEMA_VERSION,
                extractor: EXTRACTOR_VERSION,
                cursor: cursor.clone(),
                saved_at_ms: now_ms(),
            },
        )
        .context("writing cursor.json")?;
        self.saved = cursor;
        self.saved_at = Instant::now();
        Ok(())
    }

    fn after_step(&mut self, stop: &Stop) {
        let c = self.follower.cursor().clone();
        let behind = *stop == Stop::Budget;
        if !behind {
            *self.shared.caught_up_ms.lock().unwrap() = now_ms();
            if self.backfill.take().is_some() {
                tracing::info!(position = c.position, "index: caught up with the WAL");
                // The rows are every chunk now: dead vectors can be counted.
                self.shared.vectors.on_caught_up();
            }
        }
        let backfill = self.backfill.as_ref().map(|(from, total)| Backfill {
            done_bytes: bytes_between(&self.cfg.wal_dir(), from, &c),
            total_bytes: *total,
        });
        let waiting = match stop {
            Stop::Partial {
                segment,
                offset,
                reason,
            } => Some(format!(
                "a frame not yet whole at segment {segment}, offset {offset} ({reason})"
            )),
            _ => None,
        };
        self.shared.set(|s| {
            s.state = if behind { "backfilling" } else { "ready" }.into();
            s.position = c.position;
            s.segment = c.segment;
            s.offset = c.offset;
            s.backfill = backfill;
            s.waiting = waiting;
            s.last_error = None;
        });
    }

    /// Drop the index, and follow the WAL from its start.
    fn rebuild(&mut self, why: &str) -> anyhow::Result<()> {
        tracing::info!(%why, "index: rebuilding from the WAL's start");
        self.writer.clear().context("clearing the index")?;
        self.shared.engine.reload().context("reloading the index")?;
        self.shared.vectors.on_clear();
        self.places.clear();
        self.places_dirty = true;
        self.follower = WalFollower::open(&self.cfg.wal_dir(), Cursor::start())?;
        self.save()?;
        let total = wal_bytes_after(&self.cfg.wal_dir(), 0, 0);
        self.backfill = Some((Cursor::start(), total));
        self.shared.set(|s| {
            s.rebuilds += 1;
            s.state = "backfilling".into();
            s.position = 0;
            s.segment = 0;
            s.offset = 0;
        });
        Ok(())
    }

    /// `index.forget` (theseus-64x): the named nodes leave the index whole,
    /// and every chunk holding a named text leaves it, committed at once, so
    /// no query or neighbour finds them from the answer on; then the vectors
    /// of the texts they held leave every vector file, rewritten atomically.
    /// A text another chunk still holds keeps its vector (`still_held` says
    /// where). The WAL is not touched: until the core's `Suppression` and
    /// `Redaction` records exist and the follower obeys them, a rebuild
    /// brings a forgotten node back (a redaction's erased payload does not:
    /// the follower skips it, and a skip removes what was indexed).
    pub fn forget(&mut self, p: &ForgetParams) -> anyhow::Result<ForgetResult> {
        let t0 = Instant::now();
        let engine = &self.shared.engine;
        let mut asked: HashSet<u128> = HashSet::new();
        let (mut nodes, mut chunks) = (Vec::<String>::new(), Vec::<(String, u64)>::new());
        let mut chunk_count = 0u64;
        for id in &p.nodes {
            if nodes.contains(id) {
                continue;
            }
            let held = engine.node_chunks(id).context("reading a node's chunks")?;
            if held.is_empty() {
                continue;
            }
            chunk_count += held.len() as u64;
            asked.extend(held.iter().map(|(_, text)| text_hash(text)));
            nodes.push(id.clone());
        }
        for text in &p.texts {
            asked.insert(text_hash(text));
            for (node, chunk) in engine
                .chunks_holding(text)
                .context("finding a text's chunks")?
            {
                if !nodes.contains(&node) && !chunks.contains(&(node.clone(), chunk)) {
                    chunks.push((node, chunk));
                }
            }
        }
        chunk_count += chunks.len() as u64;
        if !nodes.is_empty() || !chunks.is_empty() {
            self.delete_now(&nodes, &chunks)?;
        }
        let keys: Vec<(String, u32)> = chunks.iter().map(|(n, c)| (n.clone(), *c as u32)).collect();
        let v = self.shared.vectors.forget(&nodes, &keys, &asked)?;
        tracing::info!(
            nodes = nodes.len(),
            chunks = chunk_count,
            vectors_dropped = v.compacted.dropped,
            still_held = v.held.len(),
            "index: forgotten"
        );
        Ok(ForgetResult {
            nodes: nodes.len() as u64,
            chunks: chunk_count,
            vectors_dropped: v.compacted.dropped,
            still_held: v
                .held
                .into_iter()
                .map(|(node_id, chunk)| ChunkRef {
                    node_id,
                    chunk: u64::from(chunk),
                })
                .collect(),
            files: v.compacted.files,
            bytes_before: v.compacted.bytes_before,
            bytes_after: v.compacted.bytes_after,
            ms: t0.elapsed().as_secs_f64() * 1e3,
        })
    }

    /// Delete `nodes` whole and `chunks` one by one, and commit, so a reader
    /// reloaded after no longer sees them; or, on any error, none of it: the
    /// writer is rolled back, so no delete waits for the next batch's commit.
    fn delete_now(&mut self, nodes: &[String], chunks: &[(String, u64)]) -> anyhow::Result<()> {
        let done = (|| -> anyhow::Result<()> {
            for id in nodes {
                self.writer.delete_node(id);
            }
            for (node, chunk) in chunks {
                self.writer
                    .delete_chunk(node, *chunk)
                    .context("deleting a chunk")?;
            }
            self.writer.commit().context("committing the forget")?;
            Ok(())
        })();
        if let Err(e) = done {
            self.writer.rollback().ok();
            return Err(e);
        }
        self.shared.engine.reload().context("reloading the index")?;
        Ok(())
    }

    /// Do the `index.forget` calls waiting, and answer them.
    pub fn serve_forgets(&mut self) {
        let waiting = std::mem::take(&mut *self.shared.forgets.lock().unwrap());
        for (p, reply) in waiting {
            let r = self.forget(&p).map_err(|e| format!("{e:#}"));
            let _ = reply.send(r);
        }
    }

    /// Follow until [`Shared::request_stop`]: read while there is more, then
    /// wait for the WAL to change, a kick, or the backstop.
    pub fn run(mut self) -> anyhow::Result<()> {
        loop {
            if self.shared.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            if self.shared.rebuild.swap(false, Ordering::SeqCst) {
                if let Err(e) = self.rebuild("asked") {
                    self.stalled(&e);
                }
            }
            self.serve_forgets();
            let more = match self.step() {
                Ok(s) => s.stop == Stop::Budget,
                Err(e) => {
                    self.stalled(&e);
                    false
                }
            };
            if !more {
                self.waker
                    .wait(self.cfg.backstop)
                    .context("waiting on the WAL")?;
            }
        }
    }

    fn stalled(&self, e: &anyhow::Error) {
        tracing::error!(error = %format!("{e:#}"), "index: stalled; trying again at the next change or the backstop");
        self.shared.set(|s| {
            s.state = "stalled".into();
            s.last_error = Some(format!("{e:#}"));
            s.last_error_ms = now_ms();
        });
    }
}

/// A META record that says where a session lives: `(session, place)`.
fn place_of(r: &Record) -> Option<(String, String)> {
    let key = r.key.as_deref()?;
    let value: String = serde_json::from_slice(&r.payload).ok()?;
    if let Some(place) = key.strip_prefix(PLACE_META_PREFIX) {
        return Some((value, format!("discord:{place}")));
    }
    if let Some(session) = key.strip_prefix(TASK_META_PREFIX) {
        return Some((session.to_string(), value));
    }
    None
}

/// The WAL's bytes after (segment, offset): the segment's rest, and every
/// later segment. Segment 0 is before the first.
pub fn wal_bytes_after(wal_dir: &Path, segment: u32, offset: u64) -> u64 {
    let Ok(segments) = wal::list_segments(wal_dir) else {
        return 0;
    };
    let mut total = 0u64;
    for s in segments.into_iter().filter(|s| *s >= segment) {
        let len = fs::metadata(wal::segment_path(wal_dir, s)).map_or(0, |m| m.len());
        total += if s == segment {
            len.saturating_sub(offset)
        } else {
            len
        };
    }
    total
}

/// The WAL's bytes from one cursor to a later one.
fn bytes_between(wal_dir: &Path, from: &Cursor, to: &Cursor) -> u64 {
    wal_bytes_after(wal_dir, from.segment, from.offset)
        .saturating_sub(wal_bytes_after(wal_dir, to.segment, to.offset))
}

pub fn now_ms() -> u64 {
    theseus_store::record::now_unix_ms()
}

/// This process's resident memory, from `/proc/self/statm`.
fn rss_bytes() -> u64 {
    let pages = fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1)?.parse::<u64>().ok())
        .unwrap_or(0);
    // SAFETY: no pointers.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    pages * u64::try_from(page).unwrap_or(4096)
}

impl std::fmt::Debug for Tender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tender")
            .field("index", &self.paths.dir)
            .field("cursor", self.follower.cursor())
            .finish()
    }
}
