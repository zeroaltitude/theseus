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

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use theseus_follow::{Cursor, FollowError, Kicker, Stop, Waker, WalFollower};
use theseus_store::{kinds, wal, Record};

use crate::engine::{self, Engine, Writer, SCHEMA_VERSION};
use crate::extract::{extract, Extract, EXTRACTOR_VERSION};
use crate::proto::{Backfill, IndexStatus, Lag, QueryParams, QueryResult};
use crate::state::{self, Lock, Paths, Places, Saved, CURSOR_FORMAT};

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
}

impl Config {
    pub fn new(store_dir: &Path, index_dir: &Path) -> Self {
        Self {
            store_dir: store_dir.to_path_buf(),
            index_dir: index_dir.to_path_buf(),
            batch_bytes: 4 << 20,
            backstop: Duration::from_secs(60),
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

/// What the ingest thread and the socket's threads share.
pub struct Shared {
    pub engine: Engine,
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

    pub fn query(&self, p: &QueryParams) -> anyhow::Result<QueryResult> {
        let (hits, timings) = self.engine.query(p)?;
        let (position, segment, offset) = {
            let s = self.status.lock().unwrap();
            (s.position, s.segment, s.offset)
        };
        Ok(QueryResult {
            hits,
            indexed_through: position,
            lag: self.lag(segment, offset),
            timings,
        })
    }

    /// Drop the index and backfill: the ingest thread does it at its next
    /// wake, which this causes.
    pub fn request_rebuild(&self) {
        self.rebuild.store(true, Ordering::SeqCst);
        self.kicker.kick();
    }

    /// End [`Tender::run`] (tests, and an embedding that stops it).
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.kicker.kick();
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
        let waker = Waker::new(&wal_dir).context("watching the WAL")?;
        let total = wal_bytes_after(&wal_dir, cursor.segment, cursor.offset);
        let shared = Arc::new(Shared {
            engine,
            status: Mutex::new(IndexStatus {
                state: "starting".into(),
                mode: "bm25_only".into(),
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
            committed: false,
        };
        let indexed = self.ingest(&batch.records, &mut step).and_then(|()| {
            if step.indexed > 0 {
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
        if step.indexed > 0 {
            #[cfg(test)]
            if self.crash_after_commit {
                anyhow::bail!("killed between the commit and the cursor (test)");
            }
            self.save()?;
            self.shared.engine.reload().context("reloading the index")?;
            step.committed = true;
        } else if (self.places_dirty || self.saved_at.elapsed() >= self.cfg.backstop)
            && *self.follower.cursor() != self.saved
        {
            self.save()?;
        }
        self.after_step(&stop);
        Ok(step)
    }

    fn ingest(&mut self, records: &[Record], step: &mut Step) -> anyhow::Result<()> {
        let (mut skipped, mut undecodable) = (0u64, 0u64);
        for r in records {
            match r.kind {
                kinds::NODE => match extract(&r.payload) {
                    Ok(Extract::Index(e)) => {
                        let place = self.places.get(&e.session_id).map(String::as_str);
                        self.writer
                            .replace(r.position, place, &e)
                            .with_context(|| format!("indexing node {}", e.node_id))?;
                        step.indexed += 1;
                    }
                    Ok(Extract::Skip { .. }) => skipped += 1,
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
            }
        }
        let backfill = self.backfill.as_ref().map(|(from, total)| Backfill {
            done_bytes: bytes_between(&self.cfg.wal_dir(), from, &c),
            total_bytes: *total,
        });
        self.shared.set(|s| {
            s.state = if behind { "backfilling" } else { "ready" }.into();
            s.position = c.position;
            s.segment = c.segment;
            s.offset = c.offset;
            s.backfill = backfill;
            s.last_error = None;
        });
    }

    /// Drop the index, and follow the WAL from its start.
    fn rebuild(&mut self, why: &str) -> anyhow::Result<()> {
        tracing::info!(%why, "index: rebuilding from the WAL's start");
        self.writer.clear().context("clearing the index")?;
        self.shared.engine.reload().context("reloading the index")?;
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
