//! The kernel's view of storage: sessions, ledger rows, and small runtime
//! state, written through `theseus_store::WalStore` (spec §6, M1 Keel).
//!
//! Every write is a WAL frame, durable when the call returns. Sessions and
//! meta are "latest by key"; the ledger is an append-only kind. The index is
//! rebuilt from the WAL on open if it lost anything, so this module never
//! has to think about recovery.
//!
//! A turn writes through its own handle (`for_turn`), and so does the kernel
//! for it (`shared`, into `Kernel::view`). Such a handle keeps the turn's
//! observability rows (`defer`) and puts them at the front of the next frame
//! it commits, whoever commits it (theseus-qa0: every frame is an fdatasync).
//! A state transition or a node is never deferred: it is durable when the
//! call that wrote it returns. A crash can lose only rows still waiting.
//!
//! The handle also keeps the turn's view of its session's transcript
//! (`transcript`): read once, at the turn's first reader, then extended with
//! every node the turn's frames write, so a turn decodes it once, not once
//! per reader and per loop (theseus-qa0). Nodes never change, so the view
//! stays exact.
//!
//! A session's record has more than one writer: its turns, a
//! `session.recompile`, and a task's end. Each change is a read, a change,
//! and a write, under the session's lock from the read until the write is
//! indexed (`update_session`, theseus-xeo), as K1 does for executions. So a
//! writer never puts back a copy it read before another's write.

use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use theseus_store::{
    kinds, NewRecord, Projection, Record, RecordKind, Store as _, StoreStats, Sums, WalConfig,
    WalStore,
};

use crate::node::Node;

/// The index's projection for a daemon's store (theseus-lv2): the kernel's
/// terms for each execution and action, and for each session the numbers
/// health adds up (sessions, turns, the five token counts, the cost) and the
/// term `e` while it holds external text. A change to what these say renames
/// it, and every store builds them again once, after serving.
pub static PROJECTION: Projection = Projection {
    name: "projection.core.1",
    kinds: &[kinds::EXECUTION, kinds::ACTION, kinds::SESSION],
    terms: terms_of,
    sums: sums_of,
};

/// The term of a session that holds external text (health's list of them).
pub const EXTERNAL: &str = "e";

fn terms_of(kind: RecordKind, payload: &[u8]) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct Held {
        #[serde(default)]
        external: Option<serde::de::IgnoredAny>,
    }
    match kind {
        kinds::SESSION => match serde_json::from_slice::<Held>(payload) {
            Ok(Held { external: Some(_) }) => vec![EXTERNAL.to_string()],
            _ => Vec::new(),
        },
        _ => theseus_kernel::terms::of(kind, payload),
    }
}

/// A session's numbers: [1, turns, input, output, cache read, cache
/// creation, of it with the 1-hour TTL, cost in `COST_ONE`ths of a dollar].
fn sums_of(kind: RecordKind, payload: &[u8]) -> Option<Sums> {
    if kind != kinds::SESSION {
        return None;
    }
    let s: crate::session::SessionRecord = serde_json::from_slice(payload).ok()?;
    let u = &s.usage;
    Some([
        1,
        s.turns.into(),
        u.input_tokens.into(),
        u.output_tokens.into(),
        u.cache_read_input_tokens.into(),
        u.cache_creation_input_tokens.into(),
        u.cache_creation_1h_input_tokens.into(),
        cost_fixed(s.cost_usd),
    ])
}

/// The fixed point a session's cost is added up in: 2⁻⁸⁰ of a dollar. A
/// cost of 2⁻²⁸ dollars or more converts exactly, so the total is the exact
/// sum, rounded once when it is read, and one session's total is its cost.
const COST_ONE: f64 = (1u128 << 80) as f64;

/// A cost in the fixed point; a negative or not-finite one is none.
pub fn cost_fixed(usd: f64) -> u128 {
    if usd.is_finite() && usd > 0.0 {
        (usd * COST_ONE) as u128
    } else {
        0
    }
}

/// A cost from the fixed point, in dollars.
pub fn cost_usd(fixed: u128) -> f64 {
    fixed as f64 / COST_ONE
}

#[derive(Clone)]
pub struct Store {
    inner: Arc<WalStore>,
    dir: std::path::PathBuf,
    /// Image bytes beside the WAL, by digest (theseus-9g2).
    blobs: Arc<crate::blobs::Blobs>,
    /// A turn's handle: its waiting rows and its transcript.
    turn: Option<Arc<TurnState>>,
    /// The session records being written now (theseus-xeo), shared by every
    /// handle on this store.
    sessions: Arc<SessionLocks>,
    /// Full transcript reads, by this store and its turn handles.
    #[cfg(test)]
    reads: Arc<std::sync::atomic::AtomicU64>,
    /// A turn frame a test makes fail (`fail_turn_frame`).
    #[cfg(test)]
    faults: Arc<Faults>,
}

/// One writer at a time per session record (theseus-xeo): the ids being
/// written, each with the thread that writes it, and a condvar for the
/// writers that wait. An id is in the map exactly while it is held, so there
/// is nothing to prune. A second lock of one id on one thread would wait on
/// itself forever, so it panics instead.
#[derive(Default)]
struct SessionLocks {
    held: Mutex<HashMap<String, std::thread::ThreadId>>,
    freed: Condvar,
    /// Writers waiting now (a test's way to see one blocked).
    waiting: std::sync::atomic::AtomicUsize,
}

/// A session record locked by one writer; released when dropped. It is its
/// thread's, so it is `!Send` (Review 2's R7): held across an `.await` in a
/// spawned future, it is a compile error.
#[must_use = "the lock is released when this is dropped"]
pub struct SessionLock<'a> {
    locks: &'a SessionLocks,
    id: String,
    _thread: PhantomData<*const ()>,
}

/// A session record's lock, owned: a turn keeps it from its session write,
/// which waits for the turn's last frame, until that frame is committed
/// (theseus-l6y). Released when dropped, after writing whatever still waits
/// on the turn's handle: the record is never written without its lock, even
/// when the turn fails before its last frame.
///
/// It is its thread's, so it is `!Send` (Review 2's R7): a spawned future
/// that holds one across an `.await` doesn't compile.
///
/// ```compile_fail
/// fn spawned<F: std::future::Future + Send + 'static>(_: F) {}
/// fn turn(store: theseus_core::store::Store) {
///     spawned(async move {
///         let hold = store.defer_session("ses_x", |_| {}).unwrap();
///         std::future::ready(()).await;
///         drop(hold);
///     });
/// }
/// ```
#[must_use = "the lock is released when this is dropped"]
pub struct SessionHold {
    /// The turn's handle, whose waiting rows carry the record.
    store: Store,
    id: String,
    _thread: PhantomData<*const ()>,
}

// R7, held at build time: a session lock that became `Send` fails to compile
// here (the gate runs no doctests). Two impls apply to a `Send` type, so the
// trait's parameter is ambiguous for it.
const _: fn() = || {
    trait AmbiguousIfSend<A> {
        fn some_item() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    #[allow(dead_code)]
    struct Invalid;
    impl<T: ?Sized + Send> AmbiguousIfSend<Invalid> for T {}
    let _ = <SessionHold as AmbiguousIfSend<_>>::some_item;
    let _ = <SessionLock<'static> as AmbiguousIfSend<_>>::some_item;
};

impl SessionLocks {
    fn lock(&self, id: &str) -> SessionLock<'_> {
        self.acquire(id);
        SessionLock {
            locks: self,
            id: id.to_string(),
            _thread: PhantomData,
        }
    }

    fn acquire(&self, id: &str) {
        use std::sync::atomic::Ordering::SeqCst;
        let me = std::thread::current().id();
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match held.get(id) {
                None => break,
                Some(t) if *t == me => {
                    drop(held);
                    panic!("store: session {id} locked twice on one thread");
                }
                Some(_) => {
                    self.waiting.fetch_add(1, SeqCst);
                    // Its holder may be in an fsync: the wait holds no
                    // runtime worker (theseus-vni9).
                    held = theseus_store::blocking(|| {
                        self.freed
                            .wait(held)
                            .unwrap_or_else(PoisonError::into_inner)
                    });
                    self.waiting.fetch_sub(1, SeqCst);
                }
            }
        }
        held.insert(id.to_string(), me);
    }

    fn release(&self, id: &str) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.remove(id);
        drop(held);
        self.freed.notify_all();
    }
}

impl Drop for SessionLock<'_> {
    fn drop(&mut self) {
        self.locks.release(&self.id);
    }
}

impl Drop for SessionHold {
    fn drop(&mut self) {
        // Nothing waits once the turn's last frame is written: a no-op then.
        if let Err(e) = self.store.flush() {
            tracing::warn!(error = %format!("{e:#}"), session_id = %self.id, "a turn's session write failed");
        }
        self.store.sessions.release(&self.id);
    }
}

/// A frame a test makes fail, once, as a full disk would (theseus-l6y's
/// fault injection): the first turn frame whose records the check matches.
#[cfg(test)]
type FaultCheck = Box<dyn Fn(&[NewRecord]) -> bool + Send>;

#[cfg(test)]
#[derive(Default)]
struct Faults(Mutex<Option<FaultCheck>>);

#[cfg(test)]
impl Faults {
    /// Whether this frame is the one to fail; the check is spent if so.
    fn hit(&self, records: &[NewRecord]) -> bool {
        let mut g = self.0.lock().unwrap();
        if g.as_ref().is_some_and(|f| f(records)) {
            *g = None;
            return true;
        }
        false
    }
}

/// A session's nodes with their WAL positions, in order (§4.1).
pub type Transcript = Vec<(u64, Arc<Node>)>;

/// What a turn's handle keeps (theseus-qa0).
#[derive(Default)]
struct TurnState {
    /// Rows that are no state transition, waiting for the turn's next frame,
    /// and the turn's session record at its end (`defer_session`).
    waiting: Mutex<Vec<NewRecord>>,
    /// The session's transcript as the turn knows it, once a reader asked.
    transcript: Mutex<Option<(String, Transcript)>>,
    #[cfg(test)]
    faults: Arc<Faults>,
}

impl TurnState {
    /// Commit `records` with the waiting rows in front, as one frame, and
    /// return the positions of `records`. A frame that fails is not written,
    /// so its rows wait again for the next.
    fn commit(&self, inner: &WalStore, records: &[NewRecord]) -> Result<Vec<u64>> {
        #[cfg(test)]
        if self.faults.hit(records) {
            anyhow::bail!("an injected fault: this frame was not written");
        }
        let rows = std::mem::take(&mut *self.waiting.lock().unwrap());
        if rows.is_empty() && records.is_empty() {
            return Ok(vec![]);
        }
        let n = rows.len();
        let mut frame = rows;
        frame.extend_from_slice(records);
        match inner.append(&frame) {
            Ok(mut positions) => {
                let positions = positions.split_off(n);
                self.wrote(records, &positions);
                Ok(positions)
            }
            Err(e) => {
                frame.truncate(n);
                let mut waiting = self.waiting.lock().unwrap();
                frame.append(&mut waiting);
                *waiting = frame;
                Err(e)
            }
        }
    }

    /// The nodes a committed frame wrote join the turn's transcript, from
    /// the bytes just written, at their positions: two frames committed at
    /// once may return in the other order (theseus-a60). A node that does not
    /// decode drops the transcript, so the next reader reads it again from
    /// the store.
    fn wrote(&self, records: &[NewRecord], positions: &[u64]) {
        let mut t = self.transcript.lock().unwrap();
        let Some((session, _)) = t.as_ref() else {
            return;
        };
        let new: Result<Transcript, _> = records
            .iter()
            .zip(positions)
            .filter(|(r, _)| r.kind == kinds::NODE && r.scope.as_deref() == Some(session))
            .map(|(r, p)| serde_json::from_slice::<Node>(&r.payload).map(|n| (*p, Arc::new(n))))
            .collect();
        match (new, t.as_mut()) {
            (Ok(new), Some((_, nodes))) => {
                for (p, n) in new {
                    // Nearly always at the end: then this is a push.
                    let at = nodes.partition_point(|(q, _)| *q < p);
                    nodes.insert(at, (p, n));
                }
            }
            (Err(e), _) => {
                tracing::warn!(error = %e, "a node the turn wrote did not decode; its transcript is read again");
                *t = None;
            }
            (Ok(_), None) => {}
        }
    }
}

/// The kernel's handle on a turn's store: every frame it commits carries the
/// turn's waiting rows, and its nodes join the turn's transcript
/// (`Kernel::view`).
struct TurnFrames {
    inner: Arc<WalStore>,
    turn: Arc<TurnState>,
}

impl theseus_store::Store for TurnFrames {
    fn append(&self, batch: &[NewRecord]) -> Result<Vec<u64>> {
        self.turn.commit(&self.inner, batch)
    }
    fn get(&self, position: u64) -> Result<Option<Record>> {
        self.inner.get(position)
    }
    fn scan(&self, from: u64, to: Option<u64>, limit: usize) -> Result<Vec<Record>> {
        self.inner.scan(from, to, limit)
    }
    fn latest_by_key(&self, kind: u16, key: &str) -> Result<Option<Record>> {
        self.inner.latest_by_key(kind, key)
    }
    fn latest_of_kind(&self, kind: u16) -> Result<Vec<Record>> {
        self.inner.latest_of_kind(kind)
    }
    fn tail_of_kind(&self, kind: u16, n: usize) -> Result<Vec<Record>> {
        self.inner.tail_of_kind(kind, n)
    }
    fn count_of_kind(&self, kind: u16) -> Result<u64> {
        self.inner.count_of_kind(kind)
    }
    fn scan_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<Record>> {
        self.inner.scan_scope(scope, after, limit)
    }
    fn count_in_scope(&self, scope: &str) -> Result<u64> {
        self.inner.count_in_scope(scope)
    }
    fn last_position(&self) -> u64 {
        self.inner.last_position()
    }
    fn checkpoint(&self) -> Result<u64> {
        self.inner.checkpoint()
    }
    fn stats(&self) -> Result<StoreStats> {
        self.inner.stats()
    }
    fn latest_by_terms(&self, kind: u16, lo: &str, hi: &str) -> Result<Option<Vec<Record>>> {
        self.inner.latest_by_terms(kind, lo, hi)
    }
    fn count_by_terms(&self, kind: u16, lo: &str, hi: &str) -> Result<Option<u64>> {
        self.inner.count_by_terms(kind, lo, hi)
    }
    fn latest_with_prefix(&self, kind: u16, prefix: &str) -> Result<Vec<Record>> {
        self.inner.latest_with_prefix(kind, prefix)
    }
    fn count_keys(&self, kind: u16) -> Result<u64> {
        self.inner.count_keys(kind)
    }
    fn totals(&self, kind: u16) -> Result<Option<Sums>> {
        self.inner.totals(kind)
    }
}

impl Store {
    /// Open the store directory. A store whose manifest names another format
    /// or engine is refused.
    pub fn open(dir: &Path) -> Result<Self> {
        Self::open_with(dir, WalConfig::default())
    }

    /// The store at `dir` with no fsync per frame: for a test that writes
    /// thousands of frames and times nothing (theseus-in3's lag prove), whose
    /// store is a temp dir thrown away when the run ends.
    pub fn open_unsynced(dir: &Path) -> Result<Self> {
        Self::open_with(
            dir,
            WalConfig {
                fsync: false,
                ..WalConfig::default()
            },
        )
    }

    fn open_with(dir: &Path, cfg: WalConfig) -> Result<Self> {
        // The index keeps the kernel's terms, so the kernel's readers ask by
        // state, and each session's numbers, so health adds up none
        // (theseus-lv2).
        let inner = WalStore::open_projected(dir, cfg, &PROJECTION)
            .with_context(|| format!("opening store {}", dir.display()))?;
        let st = inner.stats()?;
        if st.truncated_bytes > 0 || st.replayed_into_index > 0 {
            tracing::warn!(
                truncated_bytes = st.truncated_bytes,
                replayed = st.replayed_into_index,
                "store recovered on open"
            );
        }
        Ok(Self {
            inner: Arc::new(inner),
            dir: dir.to_path_buf(),
            blobs: Arc::new(crate::blobs::Blobs::new(dir)),
            turn: None,
            sessions: Arc::default(),
            #[cfg(test)]
            reads: Default::default(),
            #[cfg(test)]
            faults: Default::default(),
        })
    }

    /// A handle for one turn: the same store, with the turn's own waiting
    /// rows and transcript (theseus-qa0).
    pub fn for_turn(&self) -> Store {
        #[cfg(test)]
        let state = TurnState {
            faults: self.faults.clone(),
            ..TurnState::default()
        };
        #[cfg(not(test))]
        let state = TurnState::default();
        Store {
            turn: Some(Arc::new(state)),
            ..self.clone()
        }
    }

    /// Make the first turn frame whose records `check` matches fail, once,
    /// as a full disk would (theseus-l6y's fault injection).
    #[cfg(test)]
    pub fn fail_turn_frame(&self, check: impl Fn(&[NewRecord]) -> bool + Send + 'static) {
        *self.faults.0.lock().unwrap() = Some(Box::new(check));
    }

    /// A turn's session write, which rides in the turn's next frame
    /// (theseus-l6y): its end's, with the rows that wait for it. Under the
    /// record's lock, read the latest record, let `f` apply the turn's
    /// fields, and put the record with the waiting rows, where the write
    /// stood when it was a frame of its own. The returned hold keeps the lock
    /// until the caller has committed that frame (or `flush`ed it), so no
    /// other writer's change is lost to this copy. None, and nothing done,
    /// when there is no such session. A handle that is not a turn's writes
    /// the record at once.
    pub fn defer_session(
        &self,
        id: &str,
        f: impl FnOnce(&mut crate::session::SessionRecord),
    ) -> Result<Option<SessionHold>> {
        self.sessions.acquire(id);
        let deferred = (|| -> Result<bool> {
            let Some(mut rec) = self.get_session::<crate::session::SessionRecord>(id)? else {
                return Ok(false);
            };
            f(&mut rec);
            self.defer(NewRecord::json(kinds::SESSION, Some(id), &rec)?)?;
            Ok(true)
        })();
        match deferred {
            Ok(true) => Ok(Some(SessionHold {
                store: self.clone(),
                id: id.to_string(),
                _thread: PhantomData,
            })),
            // Nothing of this write waits: the lock goes, and the rows that
            // do wait keep waiting for the turn's next frame.
            other => {
                self.sessions.release(id);
                other.map(|_| None)
            }
        }
    }

    /// An observability row that is no state transition: on a turn's handle
    /// it waits for the turn's next frame; otherwise it is written now.
    pub fn defer(&self, record: NewRecord) -> Result<()> {
        match &self.turn {
            Some(t) => {
                t.waiting.lock().unwrap().push(record);
                Ok(())
            }
            None => self.inner.append(&[record]).map(|_| ()),
        }
    }

    /// Write the rows still waiting, in a frame of their own. A turn does
    /// this at its end, when its last frame failed or wrote none.
    pub fn flush(&self) -> Result<()> {
        self.commit(&[]).map(|_| ())
    }

    /// Every write: one frame, with a turn's waiting rows in front.
    fn commit(&self, records: &[NewRecord]) -> Result<Vec<u64>> {
        match &self.turn {
            Some(t) => t.commit(&self.inner, records),
            None if records.is_empty() => Ok(vec![]),
            None => self.inner.append(records),
        }
    }

    /// The store's image blobs (theseus-9g2).
    pub fn blobs(&self) -> &crate::blobs::Blobs {
        &self.blobs
    }

    /// The same store as the kernel's `Store` trait object (one WAL, one
    /// index). On a turn's handle, the kernel's frames carry the turn's
    /// waiting rows.
    pub fn shared(&self) -> Arc<dyn theseus_store::Store> {
        match &self.turn {
            Some(t) => Arc::new(TurnFrames {
                inner: self.inner.clone(),
                turn: t.clone(),
            }),
            None => self.inner.clone(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn stats(&self) -> Result<StoreStats> {
        self.inner.stats()
    }

    pub fn checkpoint(&self) -> Result<u64> {
        self.inner.checkpoint()
    }

    pub fn put_meta<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.commit(&[NewRecord::json(kinds::META, Some(key), value)?])?;
        Ok(())
    }

    pub fn get_meta<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.inner.latest_by_key(kinds::META, key)? {
            Some(r) => Ok(Some(r.decode()?)),
            None => Ok(None),
        }
    }

    pub fn put_session<T: Serialize>(&self, id: &str, value: &T) -> Result<()> {
        self.commit(&[NewRecord::json(kinds::SESSION, Some(id), value)?])?;
        Ok(())
    }

    pub fn get_session<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        match self.inner.latest_by_key(kinds::SESSION, id)? {
            Some(r) => Ok(Some(r.decode()?)),
            None => Ok(None),
        }
    }

    /// Change a session's record (theseus-xeo): under its lock, read the
    /// latest record, let `f` change it and name the other records of the
    /// same frame, and write them with it, the record last. The lock is held
    /// until the frame is indexed, so no other writer's change is lost to a
    /// copy read before it. Returns the record as written; None, and nothing
    /// written, when there is no such session. A writer that holds a copy
    /// for long (a turn) applies only the fields it owns.
    pub fn update_session(
        &self,
        id: &str,
        f: impl FnOnce(&mut crate::session::SessionRecord) -> Result<Vec<NewRecord>>,
    ) -> Result<Option<crate::session::SessionRecord>> {
        let _held = self.sessions.lock(id);
        let Some(mut rec) = self.get_session::<crate::session::SessionRecord>(id)? else {
            return Ok(None);
        };
        let mut frame = f(&mut rec)?;
        frame.push(NewRecord::json(kinds::SESSION, Some(id), &rec)?);
        self.commit(&frame)?;
        Ok(Some(rec))
    }

    /// Hold a session record's lock while `f` writes a frame that carries a
    /// change to it (theseus-9bp): `f` gets the latest record, and a frame it
    /// writes through this store, or through the kernel's view of it, is
    /// indexed before the lock is released. So a change can ride in another
    /// writer's frame (a completion's, a task's reports') and lose no other
    /// writer's change. None, and `f` not called, when there is no such
    /// session. `f` must not take the lock again; a kernel lock inside it is
    /// fine, since no kernel transition takes a session's.
    pub fn with_session<R>(
        &self,
        id: &str,
        f: impl FnOnce(crate::session::SessionRecord) -> Result<R>,
    ) -> Result<Option<R>> {
        let _held = self.sessions.lock(id);
        let Some(rec) = self.get_session::<crate::session::SessionRecord>(id)? else {
            return Ok(None);
        };
        f(rec).map(Some)
    }

    /// Hold a session record's lock, as a writer between its read and its
    /// write does (a test's way to stop one there).
    #[cfg(test)]
    pub fn lock_session(&self, id: &str) -> SessionLock<'_> {
        self.sessions.lock(id)
    }

    /// Writers waiting for a session record's lock now.
    #[cfg(test)]
    pub fn session_writers_waiting(&self) -> usize {
        self.sessions
            .waiting
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn list_sessions<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        self.inner
            .latest_of_kind(kinds::SESSION)?
            .iter()
            .map(|r| r.decode())
            .collect()
    }

    /// How many sessions there are, from the index's keys alone: no record
    /// is read (theseus-byu).
    pub fn session_count(&self) -> Result<u64> {
        self.inner.count_keys(kinds::SESSION)
    }

    /// Append a ledger row; returns its position in the WAL.
    pub fn append_ledger<T: Serialize>(&self, row: &T) -> Result<u64> {
        let p = self.commit(&[NewRecord::json(kinds::LEDGER, None, row)?])?;
        Ok(p[0])
    }

    pub fn ledger_len(&self) -> Result<u64> {
        self.inner.count_of_kind(kinds::LEDGER)
    }

    /// Newest `n` ledger rows, oldest first, as (position, row).
    pub fn ledger_tail<T: DeserializeOwned>(&self, n: usize) -> Result<Vec<(u64, T)>> {
        self.inner
            .tail_of_kind(kinds::LEDGER, n)?
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect()
    }

    /// The first `n` ledger rows after position `after`, oldest first, as
    /// (position, row): one page of a walk from the start (theseus-xo0m).
    pub fn ledger_after<T: DeserializeOwned>(&self, after: u64, n: usize) -> Result<Vec<(u64, T)>> {
        self.inner
            .of_kind_after(kinds::LEDGER, after, n)?
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect()
    }

    /// The WAL's last position: everything the kernel has ever written.
    pub fn last_position(&self) -> u64 {
        self.inner.last_position()
    }

    /// Append records as one atomic frame.
    pub fn append(&self, records: &[NewRecord]) -> Result<Vec<u64>> {
        self.commit(records)
    }

    /// A session's transcript. On a turn's handle it is read at the first
    /// call and kept: every node the turn's frames write joins it, so the
    /// turn reads it once (theseus-qa0). Elsewhere it is read now.
    pub fn transcript(&self, session_id: &str) -> Result<Transcript> {
        let read = || -> Result<Transcript> {
            Ok(self
                .session_nodes(session_id)?
                .into_iter()
                .map(|(p, n)| (p, Arc::new(n)))
                .collect())
        };
        let Some(turn) = &self.turn else {
            return read();
        };
        let mut t = turn.transcript.lock().unwrap();
        let kept = t
            .as_ref()
            .filter(|(s, _)| s == session_id)
            .map(|(_, nodes)| nodes.clone());
        let nodes = match kept {
            Some(nodes) => nodes,
            None => {
                let nodes = read()?;
                *t = Some((session_id.to_string(), nodes.clone()));
                nodes
            }
        };
        drop(t);
        // Tests hold the kept transcript to the store's: a node the turn
        // wrote past its handle would be missing here, and the model would
        // never read it.
        #[cfg(debug_assertions)]
        {
            let stored = self.scan_nodes(session_id)?;
            let kept: Vec<(u64, &str)> = nodes.iter().map(|(p, n)| (*p, n.id.as_str())).collect();
            let stored: Vec<(u64, &str)> =
                stored.iter().map(|(p, n)| (*p, n.id.as_str())).collect();
            assert_eq!(
                kept, stored,
                "the turn's transcript differs from the store's"
            );
        }
        Ok(nodes)
    }

    /// Every node of a session with its WAL position, in order (§4.1: order is positional).
    pub fn session_nodes(&self, session_id: &str) -> Result<Vec<(u64, crate::node::Node)>> {
        #[cfg(test)]
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.scan_nodes(session_id)
    }

    /// How many times the transcript was read in full (tests).
    #[cfg(test)]
    pub fn transcript_reads(&self) -> u64 {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn scan_nodes(&self, session_id: &str) -> Result<Vec<(u64, crate::node::Node)>> {
        let mut out = Vec::new();
        for r in self.inner.scan_scope(session_id, 0, usize::MAX)? {
            if r.kind == kinds::NODE {
                out.push((r.position, r.decode()?));
            }
        }
        Ok(out)
    }

    /// A session's first node, with its WAL position: a task's brief
    /// (theseus-jpff). Its records are read from the start a few at a time,
    /// so a long session is not read whole.
    pub fn first_node(&self, session_id: &str) -> Result<Option<(u64, crate::node::Node)>> {
        let mut after = 0;
        loop {
            let records = self.inner.scan_scope(session_id, after, 8)?;
            let Some(last) = records.last() else {
                return Ok(None);
            };
            after = last.position;
            if let Some(r) = records.iter().find(|r| r.kind == kinds::NODE) {
                return Ok(Some((r.position, r.decode()?)));
            }
        }
    }

    /// Newest `n` nodes across every session, oldest first.
    pub fn recent_nodes(&self, n: usize) -> Result<Vec<(u64, crate::node::Node)>> {
        self.inner
            .tail_of_kind(kinds::NODE, n)?
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect()
    }

    pub fn node_count(&self) -> Result<u64> {
        self.inner.count_of_kind(kinds::NODE)
    }

    /// A node by its id, with its WAL position.
    pub fn get_node(&self, id: &str) -> Result<Option<(u64, crate::node::Node)>> {
        match self.inner.latest_by_key(kinds::NODE, id)? {
            Some(r) => Ok(Some((r.position, r.decode()?))),
            None => Ok(None),
        }
    }

    /// Every record of a scope after the position `after`, oldest first: a
    /// session's, or the edges into a node (`in:<node>`, 12a).
    pub fn scope_after(&self, scope: &str, after: u64) -> Result<Vec<Record>> {
        self.inner.scan_scope(scope, after, usize::MAX)
    }

    pub fn get_compilation(&self, id: &str) -> Result<Option<crate::compiler::Compilation>> {
        match self.inner.latest_by_key(kinds::COMPILATION, id)? {
            Some(r) => Ok(Some(r.decode()?)),
            None => Ok(None),
        }
    }

    /// A session's compilations, oldest first.
    pub fn session_compilations(
        &self,
        session_id: &str,
    ) -> Result<Vec<crate::compiler::Compilation>> {
        let mut out = Vec::new();
        for r in self.inner.scan_scope(session_id, 0, usize::MAX)? {
            if r.kind == kinds::COMPILATION {
                out.push(r.decode()?);
            }
        }
        Ok(out)
    }

    /// Newest `n` compilations across every session, oldest first.
    pub fn recent_compilations(&self, n: usize) -> Result<Vec<crate::compiler::Compilation>> {
        self.inner
            .tail_of_kind(kinds::COMPILATION, n)?
            .iter()
            .map(|r| r.decode())
            .collect()
    }

    /// The same store as the kernel's `Store` trait object (one WAL, one index).
    pub fn inner(&self) -> &Arc<WalStore> {
        &self.inner
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ledger::LedgerRow;
    use std::collections::BTreeMap;

    fn row(kind: &str) -> NewRecord {
        let r = LedgerRow::named(kind, Some("ses_t"), Some("turn_t"), serde_json::Value::Null);
        NewRecord::json(kinds::LEDGER, None, &r).unwrap()
    }

    fn labels(s: &Store, after: u64) -> Vec<String> {
        s.inner
            .scan(after + 1, None, 100)
            .unwrap()
            .iter()
            .map(|r| match r.kind {
                kinds::LEDGER => r.decode::<LedgerRow>().unwrap().kind,
                k => kinds::name(k).to_string(),
            })
            .collect()
    }

    /// A turn's rows wait for its next frame, whoever commits it: the turn's
    /// own write or the kernel through `shared` (theseus-qa0). They go in
    /// front, in the order written, and the caller gets its own positions. A
    /// crash loses only the rows still waiting.
    #[test]
    fn a_turns_rows_ride_in_its_next_frame_and_a_crash_loses_only_those() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        let frames = |s: &Store| s.stats().unwrap().frames_appended;
        let t = store.for_turn();
        let (f0, p0) = (frames(&store), store.last_position());
        t.defer(row("turn.started")).unwrap();
        t.defer(row("context.compiled")).unwrap();
        assert_eq!(frames(&store), f0, "a deferred row writes nothing");
        t.put_session("ses_t", &serde_json::json!({"n": 1}))
            .unwrap();
        assert_eq!(frames(&store), f0 + 1);
        assert_eq!(
            labels(&store, p0),
            ["turn.started", "context.compiled", "session"]
        );
        let p1 = store.last_position();
        t.defer(row("loop.started")).unwrap();
        let at = t
            .shared()
            .append(&[NewRecord::json(kinds::META, Some("m"), &1).unwrap()])
            .unwrap();
        assert_eq!(
            at,
            vec![p1 + 2],
            "the kernel gets its own record's position"
        );
        assert_eq!(labels(&store, p1), ["loop.started", "meta"]);
        // Another handle's writes do not carry the turn's rows.
        t.defer(row("loop.ended")).unwrap();
        store.put_meta("other", &2).unwrap();
        assert_eq!(labels(&store, p1 + 2), ["meta"]);
        // The process dies with a row still waiting.
        drop((t, store));
        let reopened = Store::open(d.path()).unwrap();
        assert_eq!(reopened.last_position(), p1 + 3);
        assert!(!labels(&reopened, p0).contains(&"loop.ended".to_string()));
    }

    /// A turn's transcript is read at its first reader and kept (theseus-qa0):
    /// the nodes its frames write join it with their positions, the kernel's
    /// frames included, and no later reader reads it again. Another session's
    /// node does not join, and a node written past the handle is not in it
    /// (in a test build, `transcript` would then panic naming the gap).
    #[test]
    fn a_turns_transcript_is_read_once_and_keeps_what_the_turn_writes() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        let node = |sid: &str, text: &str| Node::user(sid, None, "op", text);
        let ids = |v: &Transcript| -> Vec<(u64, String)> {
            v.iter().map(|(p, n)| (*p, n.id.clone())).collect()
        };
        let n1 = node("ses_t", "one");
        let p1 = store.append(&[n1.record().unwrap()]).unwrap()[0];
        let t = store.for_turn();
        let r0 = store.transcript_reads();
        assert_eq!(ids(&t.transcript("ses_t").unwrap()), [(p1, n1.id.clone())]);
        let (n2, n3, other) = (
            node("ses_t", "two"),
            node("ses_t", "three"),
            node("ses_o", "x"),
        );
        t.defer(row("loop.started")).unwrap();
        let p2 = t.append(&[n2.record().unwrap()]).unwrap()[0];
        let p3 = t.shared().append(&[n3.record().unwrap()]).unwrap()[0];
        t.append(&[other.record().unwrap()]).unwrap();
        let kept = t.transcript("ses_t").unwrap();
        assert_eq!(ids(&kept), [(p1, n1.id), (p2, n2.id), (p3, n3.id.clone())]);
        assert_eq!(kept[2].1.body, n3.body, "decoded from the bytes written");
        assert_eq!(store.transcript_reads() - r0, 1, "read once");
        // Another turn reads its own.
        assert_eq!(store.for_turn().transcript("ses_t").unwrap().len(), 3);
        assert_eq!(store.transcript_reads() - r0, 2);
        // A node written through another handle does not join the kept one.
        store
            .append(&[node("ses_t", "four").record().unwrap()])
            .unwrap();
        let held = t.turn.as_ref().unwrap().transcript.lock().unwrap();
        assert_eq!(held.as_ref().unwrap().1.len(), 3);
    }

    /// Two frames committed at once can return in the other order than their
    /// positions (theseus-a60). Their nodes still join the kept transcript at
    /// their positions, as a fresh read has them (in a test build,
    /// `transcript` compares the two).
    #[test]
    fn nodes_join_the_kept_transcript_at_their_positions() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        let t = store.for_turn();
        let node = |text: &str| Node::user("ses_t", None, "op", text);
        let first = node("first");
        let p0 = t.append(&[first.record().unwrap()]).unwrap()[0];
        assert_eq!(t.transcript("ses_t").unwrap().len(), 1);
        let (a, b) = (node("a"), node("b"));
        let (ra, rb) = (vec![a.record().unwrap()], vec![b.record().unwrap()]);
        // Both frames are written; the later one's commit returns first.
        let pa = store.inner.append(&ra).unwrap();
        let pb = store.inner.append(&rb).unwrap();
        let turn = t.turn.as_ref().unwrap();
        turn.wrote(&rb, &pb);
        turn.wrote(&ra, &pa);
        let kept: Vec<(u64, String)> = t
            .transcript("ses_t")
            .unwrap()
            .iter()
            .map(|(p, n)| (*p, n.id.clone()))
            .collect();
        assert_eq!(kept, [(p0, first.id), (pa[0], a.id), (pb[0], b.id)]);
    }

    /// `flush` writes what still waits in one frame, and nothing when
    /// nothing waits; a frame that fails leaves its rows waiting.
    #[test]
    fn flush_writes_what_waits_and_a_failed_frame_keeps_its_rows() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        let t = store.for_turn();
        let f0 = store.stats().unwrap().frames_appended;
        t.flush().unwrap();
        assert_eq!(store.stats().unwrap().frames_appended, f0, "nothing waits");
        t.defer(row("turn.ended")).unwrap();
        t.defer(row("turn.trace")).unwrap();
        let p0 = store.last_position();
        t.flush().unwrap();
        assert_eq!(store.stats().unwrap().frames_appended, f0 + 1);
        assert_eq!(labels(&store, p0), ["turn.ended", "turn.trace"]);

        // A WAL with room for one small frame only: the second append fails.
        let full = tempfile::tempdir().unwrap();
        let wal = WalStore::open(
            full.path(),
            WalConfig {
                max_total_bytes: Some(600),
                ..WalConfig::default()
            },
        )
        .unwrap();
        let small = Store {
            inner: Arc::new(wal),
            dir: full.path().to_path_buf(),
            blobs: Arc::new(crate::blobs::Blobs::new(full.path())),
            turn: None,
            sessions: Arc::default(),
            reads: Default::default(),
            faults: Default::default(),
        }
        .for_turn();
        small.put_meta("a", &1).unwrap();
        small.defer(row("loop.ended")).unwrap();
        let big = "x".repeat(1000);
        assert!(small.put_meta("b", &big).is_err(), "the WAL is full");
        let waiting = small.turn.as_ref().unwrap().waiting.lock().unwrap().len();
        assert_eq!(waiting, 1, "the row waits for the next frame");
    }

    /// A copy of the store an older binary wrote (460a35b, before F2; see the
    /// fixture's README), in a temporary directory.
    pub(crate) fn older_store() -> tempfile::TempDir {
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/store-460a35b");
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("wal")).unwrap();
        for f in ["MANIFEST.json", "index.redb", "wal/000000001.seg"] {
            std::fs::copy(from.join(f), d.path().join(f)).unwrap();
        }
        d
    }

    /// The versioned-reader rule (P5b, theseus-qa0 F4a): today's binary
    /// opens a store an older one wrote, as it is, and reads every record in
    /// it into today's types: sessions, executions, actions, completions,
    /// nodes, the compilation, and the ledger. Reading marks nothing.
    #[test]
    fn a_store_an_older_binary_wrote_reads_every_record() {
        use theseus_kernel::{Action, Completion, Execution};
        let d = older_store();
        let store = Store::open(d.path()).unwrap();
        let st = store.stats().unwrap();
        assert_eq!(st.last_position, 121);
        assert!(
            store.inner.recovery().checked_from.is_some() && st.history_bytes > 0,
            "the old index's checkpoint spares the open its history: {st:?}"
        );
        let all = store.inner.scan(1, None, usize::MAX).unwrap();
        assert_eq!(all.len(), 121, "every position reads");
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for r in &all {
            assert_eq!(r.schema, 1, "{} at {}", kinds::name(r.kind), r.position);
            let name = kinds::name(r.kind);
            *by_kind.entry(name).or_default() += 1;
            let read = match r.kind {
                kinds::SESSION => r.decode::<crate::session::SessionRecord>().map(|_| ()),
                kinds::LEDGER => r.decode::<LedgerRow>().map(|_| ()),
                kinds::EXECUTION => Execution::from_stored(&r.payload, 100_000_000).map(|_| ()),
                kinds::ACTION | kinds::OUTBOX => r.decode::<Action>().map(|_| ()),
                kinds::COMPLETION => r.decode::<Completion>().map(|_| ()),
                kinds::NODE => r.decode::<crate::node::Node>().map(|_| ()),
                kinds::COMPILATION => r.decode::<crate::compiler::Compilation>().map(|_| ()),
                _ => r.decode::<serde_json::Value>().map(|_| ()),
            };
            read.unwrap_or_else(|e| panic!("{name} at {}: {e:#}", r.position));
        }
        let want: BTreeMap<&str, usize> = [
            ("session", 6),
            ("ledger", 76),
            ("execution", 17),
            ("action", 14),
            ("completion", 2),
            ("node", 5),
            ("compilation", 1),
        ]
        .into_iter()
        .collect();
        assert_eq!(by_kind, want);

        // Through the product's own reads: every session, and the turn's
        // transcript.
        let sessions: Vec<crate::session::SessionRecord> = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 4);
        let nodes: usize = sessions
            .iter()
            .map(|s| store.session_nodes(&s.session_id).unwrap().len())
            .sum();
        assert_eq!(nodes, 5, "every node belongs to a session");
        assert!(store.recent_compilations(5).unwrap().len() == 1);
        let kinds_seen: Vec<String> = store
            .ledger_tail::<LedgerRow>(200)
            .unwrap()
            .into_iter()
            .map(|(_, r)| r.kind)
            .collect();
        assert!(kinds_seen.iter().any(|k| k == "tool.job_started"));
        let m: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.path().join("MANIFEST.json")).unwrap())
                .unwrap();
        assert_eq!(m["format"], 2, "reading marks nothing: {m}");
    }

    /// The first edge (12a, theseus-n4m). EDGE is at schema 1, which every
    /// manifest marks already (a new store's lists every kind this build
    /// writes, and a format-2 store's records are all schema 1), so its
    /// first write rewrites no manifest. An older binary's store keeps
    /// format 2, opens again, and reads the edge from the scope into its
    /// target.
    #[test]
    fn an_edge_marks_no_manifest_and_an_older_store_still_opens() {
        use crate::graph::{Edge, EdgeKind, VIA_REPORT};
        let edge = Edge::new(EdgeKind::DerivedFrom, "msg_copy", "msg_first", VIA_REPORT);
        let fresh = tempfile::tempdir().unwrap();
        let store = Store::open(fresh.path()).unwrap();
        let before = std::fs::read(fresh.path().join("MANIFEST.json")).unwrap();
        store.append(&[edge.record().unwrap()]).unwrap();
        assert_eq!(
            std::fs::read(fresh.path().join("MANIFEST.json")).unwrap(),
            before,
            "the first edge marks nothing"
        );
        drop(store);

        let d = older_store();
        let store = Store::open(d.path()).unwrap();
        store.append(&[edge.record().unwrap()]).unwrap();
        drop(store);
        let m: serde_json::Value =
            serde_json::from_slice(&std::fs::read(d.path().join("MANIFEST.json")).unwrap())
                .unwrap();
        assert_eq!(
            m["format"], 2,
            "an edge keeps an older store at format 2: {m}"
        );
        let store = Store::open(d.path()).unwrap();
        let into = store.scope_after("in:msg_first", 0).unwrap();
        assert_eq!(into.len(), 1);
        assert_eq!(into[0].decode::<Edge>().unwrap(), edge);
        assert_eq!(store.stats().unwrap().last_position, 122);
    }

    /// A tool-call node as a build before theseus-ppsd wrote it (NODE schema
    /// 2): its gate record's plan has no class and no AWS call. Literal bytes,
    /// never re-serialized (theseus-djfj). Its source: the build that wrote
    /// schema 2 is before this repository's first commit (e85efb0), so the
    /// literal is the layout by hand; e85efb0's own build, which adds only
    /// fields that serialize when set, writes it back byte for byte.
    const NODE_SCHEMA_2: &str = r#"{"id":"tcl_00000000000000000000000000000021","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t2","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000021,"body":{"kind":"tool_call","tool_use_id":"tu_21","tool":"fs.read","wire_name":"fs_read","input":{"path":"/w/log/tides.md"},"assistant_node":"asm_00000000000000000000000000000021","correlation_id":"act_t21","gate":{"decision":{"posture":"open","reason":"fs.read — open (policy.tools)"},"plan":{"resources":[{"access":"read","path":"/w/log/tides.md"}],"summary":"read /w/log/tides.md"},"proposal":{"args":{"path":"/w/log/tides.md"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/log/tides.md","tool":"fs.read"},"result":{"gate":"allow"},"validated":true}}}"#;

    /// NODE schema 3 (theseus-ppsd): this build reads a schema-2 tool-call
    /// node through the store with neither `plan.class` nor `plan.aws`, and
    /// its bytes encode again unchanged; an AWS call's node, which carries
    /// both, is written at this build's schema and reads back whole.
    #[test]
    fn a_tool_call_node_written_before_its_class_and_aws_reads() {
        use crate::node::Body;
        use theseus_protocol::{AwsPlan, ToolClass};
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        assert_eq!(
            store.inner.schema_marks()[&kinds::NODE],
            kinds::schema(kinds::NODE)
        );
        let old = NewRecord {
            schema: 2,
            ..NewRecord::bytes(
                kinds::NODE,
                Some("tcl_00000000000000000000000000000021"),
                NODE_SCHEMA_2.as_bytes().to_vec(),
            )
        }
        .scoped("ses_lighthouse");
        store.append(&[old]).unwrap();

        let stored = store.scope_after("ses_lighthouse", 0).unwrap();
        assert_eq!(stored[0].schema, 2, "it keeps the schema it was written at");
        let nodes = store.session_nodes("ses_lighthouse").unwrap();
        let read = nodes[0].1.clone();
        let Body::ToolCall { gate: Some(g), .. } = &read.body else {
            panic!("a tool call with its gate: {read:?}");
        };
        let plan = g.plan.as_ref().unwrap();
        assert_eq!((plan.class, plan.aws.as_ref()), (None, None));
        assert_eq!(plan.summary, "read /w/log/tides.md");
        assert_eq!(
            serde_json::to_string(&read).unwrap(),
            NODE_SCHEMA_2,
            "a node with neither keeps its bytes"
        );

        let mut aws = read.clone();
        aws.id = "tcl_00000000000000000000000000000022".into();
        let Body::ToolCall { tool, gate, .. } = &mut aws.body else {
            unreachable!()
        };
        *tool = "aws.call".into();
        let plan = gate.as_mut().unwrap().plan.as_mut().unwrap();
        plan.class = Some(ToolClass::Read);
        plan.aws = Some(AwsPlan {
            account: "111122223333".into(),
            region: "us-west-2".into(),
            service: "cloudformation".into(),
            operation: "DescribeStacks".into(),
            ..Default::default()
        });
        let r = aws.record().unwrap();
        assert_eq!(r.schema, kinds::schema(kinds::NODE));
        let text = String::from_utf8_lossy(&r.payload).into_owned();
        assert!(
            text.contains(r#""class":"read""#)
                && text.contains(r#""aws":{"account":"111122223333""#),
            "{text}"
        );
        store.append(&[r]).unwrap();
        let nodes: Vec<Node> = store
            .session_nodes("ses_lighthouse")
            .unwrap()
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert_eq!(nodes, vec![read, aws]);
    }

    /// A tool call as theseus-ppsd's build wrote it (NODE schema 3): its plan
    /// names its class, and its decision has none. Literal bytes, never
    /// re-serialized, so a change to how a node encodes fails the test below
    /// (theseus-djfj). Its source: the build before 4fc7ddc (b503b2c) reads it
    /// and writes it back byte for byte.
    const NODE_SCHEMA_3: &str = r#"{"id":"tcl_00000000000000000000000000000031","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t2","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000021,"body":{"kind":"tool_call","tool_use_id":"tu_21","tool":"proc.run","wire_name":"fs_read","input":{"path":"/w/log/tides.md"},"assistant_node":"asm_00000000000000000000000000000021","correlation_id":"act_t21","gate":{"decision":{"posture":"open","reason":"fs.read — open (policy.tools)"},"plan":{"class":"run","resources":[{"access":"read","path":"/w/log/tides.md"}],"summary":"read /w/log/tides.md"},"proposal":{"args":{"path":"/w/log/tides.md"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/log/tides.md","tool":"fs.read"},"result":{"gate":"allow"},"validated":true}}}"#;

    /// NODE schema 4 (theseus-7ve.1): an L1 call's gate decision names its
    /// class. A schema-3 node (a proc.run call whose plan names its class, as
    /// theseus-ppsd writes it), taken as its literal bytes, reads with no class
    /// in its decision, and its bytes encode again unchanged; an L1 call's node
    /// is written at schema 4 and reads back whole.
    #[test]
    fn a_tool_call_node_written_before_its_l1_class_reads() {
        use crate::node::Body;
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        assert!(kinds::schema(kinds::NODE) >= 4);
        let schema_3 = NODE_SCHEMA_3;
        let old = NewRecord {
            schema: 3,
            ..NewRecord::bytes(
                kinds::NODE,
                Some("tcl_00000000000000000000000000000031"),
                schema_3.as_bytes().to_vec(),
            )
        }
        .scoped("ses_lighthouse");
        store.append(&[old]).unwrap();

        let stored = store.scope_after("ses_lighthouse", 0).unwrap();
        assert_eq!(stored[0].schema, 3, "it keeps the schema it was written at");
        let read = store.session_nodes("ses_lighthouse").unwrap()[0].1.clone();
        let Body::ToolCall { gate: Some(g), .. } = &read.body else {
            panic!("a tool call with its gate: {read:?}");
        };
        assert_eq!(g.decision.as_ref().unwrap().class, None);
        assert_eq!(
            serde_json::to_string(&read).unwrap(),
            schema_3,
            "a node with no class keeps its bytes"
        );

        let mut l1 = read.clone();
        l1.id = "tcl_00000000000000000000000000000032".into();
        let Body::ToolCall { gate, .. } = &mut l1.body else {
            unreachable!()
        };
        gate.as_mut().unwrap().decision.as_mut().unwrap().class = Some("l1".into());
        let r = l1.record().unwrap();
        assert_eq!(r.schema, kinds::schema(kinds::NODE));
        let text = String::from_utf8_lossy(&r.payload).into_owned();
        assert!(text.contains(r#""class":"l1""#), "{text}");
        store.append(&[r]).unwrap();
        let nodes: Vec<Node> = store
            .session_nodes("ses_lighthouse")
            .unwrap()
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert_eq!(nodes, vec![read, l1]);
    }

    /// An action as schema 2 wrote it: a job a cancel verified gone, before
    /// 18a's verdict. Literal bytes (theseus-djfj). Its source: the build
    /// before b77ffe9 (08b595d) reads it and writes it back byte for byte.
    const ACTION_SCHEMA_2: &str = r#"{"correlation_id":"act_00000000000000000000000000000041","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"proc.run","args_digest":"7ad721861f8d37f2a13e31ce6588122b125276a670f610806e09c6516d851cd6","retry_class":{"class":"non_repeatable"},"state":"cancelled","deadline_at_ms":1790000060041,"planned_at_ms":1790000000041,"authorized_at_ms":1790000000041,"dispatched_at_ms":1790000000041,"settled_at_ms":1790000001041,"cancel":"termination_verified","reserved_micros":0,"completions_seen":0}"#;

    /// A post as outbox schema 1 wrote it: a notice its channel took. Literal
    /// bytes (theseus-djfj). Its source: the build before b77ffe9 (08b595d)
    /// reads it and writes it back byte for byte.
    const OUTBOX_SCHEMA_1: &str = r#"{"correlation_id":"out_00000000000000000000000000000044","schema":2,"execution_id":"","session_id":"ses_lighthouse","tool":"outbox","args_digest":"9f2c7a1e4b8d3f6a0c5e9b2d7f1a4c8e3b6d9f0a2c5e8b1d4f7a0c3e6b9d2f5a","proposal":{"tool":"outbox","args":{"kind":"notice","text":"the harbour opens at six"},"resource":"discord:dm:42","policy_context":null},"resource":"discord:dm:42","retry_class":{"class":"idempotent_with_key","key":"discord.nonce"},"state":"succeeded","deadline_at_ms":0,"planned_at_ms":1790000000044,"authorized_at_ms":1790000000044,"dispatched_at_ms":1790000000045,"settled_at_ms":1790000000144,"reserved_micros":0,"completions_seen":1,"detail":{"messages":["m_44"]}}"#;

    /// ACTION schema 3 (M4 18a): a cancel's verdict on the action. A
    /// schema-2 action reads through the store with no verdict, and its bytes
    /// encode again unchanged; one a cancel verified by its pid namespace is
    /// written at schema 3 and reads back whole. A post is an action of its
    /// own kind, so OUTBOX moves with it, 1 to 2: a schema-1 post reads the
    /// same way.
    #[test]
    fn an_action_written_before_its_cancels_verdict_reads() {
        use theseus_kernel::{Action, Verdict, VerifiedBy};
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        // 3 and 2 since 18a; 4 and 3 since 18d (the test below).
        assert!(kinds::schema(kinds::ACTION) >= 3);
        assert!(kinds::schema(kinds::OUTBOX) >= 2);
        let post_id = "out_00000000000000000000000000000044";
        let post = NewRecord {
            schema: 1,
            ..NewRecord::bytes(
                kinds::OUTBOX,
                Some(post_id),
                OUTBOX_SCHEMA_1.as_bytes().to_vec(),
            )
        }
        .scoped("ses_lighthouse");
        store.append(&[post]).unwrap();
        let rec = store
            .inner
            .latest_by_key(kinds::OUTBOX, post_id)
            .unwrap()
            .unwrap();
        assert_eq!(rec.schema, 1);
        let post: Action = rec.decode().unwrap();
        assert_eq!((post.tool.as_str(), &post.verdict), ("outbox", &None));
        assert_eq!(serde_json::to_string(&post).unwrap(), OUTBOX_SCHEMA_1);
        let id = "act_00000000000000000000000000000041";
        let old = NewRecord {
            schema: 2,
            ..NewRecord::bytes(kinds::ACTION, Some(id), ACTION_SCHEMA_2.as_bytes().to_vec())
        }
        .scoped("ses_lighthouse");
        store.append(&[old]).unwrap();
        let rec = store
            .inner
            .latest_by_key(kinds::ACTION, id)
            .unwrap()
            .unwrap();
        assert_eq!(rec.schema, 2, "it keeps the schema it was written at");
        let read: Action = rec.decode().unwrap();
        assert_eq!(read.verdict, None);
        assert_eq!(
            serde_json::to_string(&read).unwrap(),
            ACTION_SCHEMA_2,
            "an action with no verdict keeps its bytes"
        );

        let mut judged = read;
        judged.correlation_id = "act_00000000000000000000000000000042".into();
        judged.verdict = Some(Verdict {
            verified_by: VerifiedBy::Pidns,
            killed: Some(4),
            survivors: Some(0),
            scope: Some("namespace".into()),
            ms: 120,
            why: None,
        });
        let r = NewRecord::json(kinds::ACTION, Some(&judged.correlation_id), &judged)
            .unwrap()
            .scoped("ses_lighthouse");
        assert_eq!(r.schema, kinds::schema(kinds::ACTION));
        let text = String::from_utf8_lossy(&r.payload).into_owned();
        assert!(text.contains(r#""verified_by":"pidns""#), "{text}");
        store.append(&[r]).unwrap();
        let back: Action = store
            .inner
            .latest_by_key(kinds::ACTION, &judged.correlation_id)
            .unwrap()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(back, judged);
    }

    /// An action as schema 3 wrote it (M4 18a): a job a cancel's tree walk
    /// verified gone, its verdict on it, and no parent. Literal bytes, never
    /// re-serialized (theseus-djfj): the kernel frames golden's at 18a.
    const ACTION_SCHEMA_3: &str = r#"{"correlation_id":"act_00000000000000000000000000000043","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"proc.run","args_digest":"7ad721861f8d37f2a13e31ce6588122b125276a670f610806e09c6516d851cd6","retry_class":{"class":"non_repeatable"},"state":"cancelled","deadline_at_ms":1790000060043,"planned_at_ms":1790000000043,"authorized_at_ms":1790000000043,"dispatched_at_ms":1790000000043,"settled_at_ms":1790000001043,"cancel":"termination_verified","verdict":{"verified_by":"tree","killed":2,"survivors":0,"ms":0},"reserved_micros":0,"resolution":"stopped by the operator","completions_seen":0}"#;

    /// A post as outbox schema 2 wrote it: a card its channel took.
    const OUTBOX_SCHEMA_2: &str = r#"{"correlation_id":"out_00000000000000000000000000000045","schema":2,"execution_id":"exe_lighthouse","session_id":"ses_lighthouse","tool":"outbox","args_digest":"4b1e9c7a2d5f8b0e3a6c9d2f5b8e1a4c7d0f3b6e9a2c5d8f1b4e7a0c3d6f9b2e","proposal":{"tool":"outbox","args":{"kind":"card","node":"tcl_00000000000000000000000000000045","question":"act_00000000000000000000000000000045"},"resource":"discord:dm:42","policy_context":null},"resource":"discord:dm:42","retry_class":{"class":"idempotent_with_key","key":"discord.nonce"},"state":"succeeded","deadline_at_ms":0,"planned_at_ms":1790000000045,"authorized_at_ms":1790000000045,"dispatched_at_ms":1790000000046,"settled_at_ms":1790000000146,"reserved_micros":0,"completions_seen":1,"detail":{"messages":["m_45"]}}"#;

    /// ACTION schema 4 (M4 18d): an action names its parent, a credential
    /// request its job. A schema-3 action, taken as its literal bytes, reads
    /// with no parent, and its bytes encode again unchanged; a request with
    /// its parent is written at schema 4 and reads back whole. A post is an
    /// action of its own kind, so OUTBOX moves with it, 2 to 3: a schema-2
    /// post reads the same way. Nothing makes a request since theseus-w5op,
    /// and one a store holds (`cred.request`) still reads whole.
    #[test]
    fn an_action_written_before_its_parent_reads() {
        use theseus_kernel::Action;
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        assert_eq!(kinds::schema(kinds::ACTION), 4);
        assert_eq!(kinds::schema(kinds::OUTBOX), 3);
        let job = "act_00000000000000000000000000000043";
        for (kind, id, schema, bytes) in [
            (kinds::ACTION, job, 3, ACTION_SCHEMA_3),
            (
                kinds::OUTBOX,
                "out_00000000000000000000000000000045",
                2,
                OUTBOX_SCHEMA_2,
            ),
        ] {
            let old = NewRecord {
                schema,
                ..NewRecord::bytes(kind, Some(id), bytes.as_bytes().to_vec())
            }
            .scoped("ses_lighthouse");
            store.append(&[old]).unwrap();
            let rec = store.inner.latest_by_key(kind, id).unwrap().unwrap();
            assert_eq!(rec.schema, schema, "it keeps the schema it was written at");
            let read: Action = rec.decode().unwrap();
            assert_eq!(read.parent, None);
            assert_eq!(
                serde_json::to_string(&read).unwrap(),
                bytes,
                "an action with no parent keeps its bytes"
            );
        }

        let mut request: Action = serde_json::from_str(ACTION_SCHEMA_3).unwrap();
        request.correlation_id = "act_00000000000000000000000000000046".into();
        request.tool = "cred.request".into();
        request.parent = Some(job.into());
        let r = NewRecord::json(kinds::ACTION, Some(&request.correlation_id), &request)
            .unwrap()
            .scoped("ses_lighthouse");
        assert_eq!(r.schema, 4);
        let text = String::from_utf8_lossy(&r.payload).into_owned();
        assert!(text.contains(&format!(r#""parent":"{job}""#)), "{text}");
        store.append(&[r]).unwrap();
        let back: Action = store
            .inner
            .latest_by_key(kinds::ACTION, &request.correlation_id)
            .unwrap()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(back, request);
    }

    /// 18d's two ledger kinds as its build stored them (literal bytes): a
    /// request granted at notify, and one declined.
    const SECRET_REQUESTED_18D: &str = r#"{"at_unix_ms":1790000000047,"kind":"secret.requested","session_id":"ses_lighthouse","data":{"command":"sh","correlation_id":"act_00000000000000000000000000000047","job":"act_00000000000000000000000000000043","kind":"secret","outcome":"granted","posture":"notify","secret":"github_token","setting":"proc.run ran at notify","why":null}}"#;
    const SECRET_DECLINED_18D: &str = r#"{"at_unix_ms":1790000000048,"kind":"secret.declined","session_id":"ses_lighthouse","data":{"by":"the CLI","correlation_id":"act_00000000000000000000000000000048","job":"act_00000000000000000000000000000043","secret":"github_token","why":"not today"}}"#;

    /// A ledger row of a kind no build writes now still reads (theseus-w5op):
    /// 18d's `secret.requested` and `secret.declined`, as stored. The row
    /// keeps its kind's name and its data, byte for byte, and the registry
    /// names neither kind (an unknown one parses to none).
    #[test]
    fn a_row_of_a_kind_no_longer_written_still_reads() {
        use theseus_protocol::LedgerKind;
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        for bytes in [SECRET_REQUESTED_18D, SECRET_DECLINED_18D] {
            let rec = NewRecord::bytes(kinds::LEDGER, None, bytes.as_bytes().to_vec());
            store.append(&[rec.scoped("ses_lighthouse")]).unwrap();
        }
        let rows = store.ledger_tail::<LedgerRow>(10).unwrap();
        let kinds_read: Vec<&str> = rows.iter().map(|(_, r)| r.kind.as_str()).collect();
        assert_eq!(kinds_read, ["secret.requested", "secret.declined"]);
        for ((_, row), bytes) in rows.iter().zip([SECRET_REQUESTED_18D, SECRET_DECLINED_18D]) {
            assert_eq!(
                LedgerKind::parse(&row.kind),
                None,
                "{} is written",
                row.kind
            );
            assert_eq!(row.data["secret"], "github_token");
            assert_eq!(
                serde_json::to_string(row).unwrap(),
                bytes,
                "it keeps its bytes"
            );
        }
    }

    /// The labels' five ledger kinds as 19a's and 19c's builds stored them
    /// (literal bytes, in their rows' shapes): a place's viewers read, a
    /// compile that withheld, a graduation, a held post, and its answer.
    const LABEL_ROWS: [&str; 5] = [
        r#"{"at_unix_ms":1790000000071,"kind":"label.audience","data":{"name":"lab","place":"discord:314159265358979323","viewers":2,"why":null}}"#,
        r#"{"at_unix_ms":1790000000072,"kind":"label.withheld","session_id":"ses_lighthouse","data":{"audience":{"digest":"0f1e2d3c4b5a6978","kind":"place","name":"lab","place":"discord:314159265358979323","viewers":2},"compilation_id":"cmp_00000000000000000000000000000052","reasons":{"owner-only":{"context_files":0,"nodes":1}},"withheld":1}}"#,
        r#"{"at_unix_ms":1790000000073,"kind":"label.graduated","session_id":"ses_lighthouse","data":{"node_id":"msg_00000000000000000000000000000061","readers":{"place":"discord:7"},"source":"trs_00000000000000000000000000000051","why":"the code is for the whole lab"}}"#,
        r#"{"at_unix_ms":1790000000074,"kind":"label.held_post","session_id":"ses_lighthouse","data":{"post":"out_00000000000000000000000000000074","question":"act_00000000000000000000000000000074","target":"discord:channel:314159265358979323"}}"#,
        r#"{"at_unix_ms":1790000000075,"kind":"label.held_post_answered","session_id":"ses_lighthouse","data":{"approve":false,"by":"the CLI","question":"act_00000000000000000000000000000074"}}"#,
    ];

    /// Old rows of the labels' kinds still read (the place rule,
    /// theseus-nbsh): each keeps its kind's name and its data, byte for byte,
    /// and the registry names none of the five (an unknown kind parses to
    /// none), so the ledger, the cockpit, and `theseus ledger` show them as
    /// they were.
    #[test]
    fn the_labels_rows_still_read() {
        use theseus_protocol::LedgerKind;
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        for bytes in LABEL_ROWS {
            let rec = NewRecord::bytes(kinds::LEDGER, None, bytes.as_bytes().to_vec());
            store.append(&[rec]).unwrap();
        }
        let rows = store.ledger_tail::<LedgerRow>(10).unwrap();
        assert_eq!(rows.len(), 5);
        for ((_, row), bytes) in rows.iter().zip(LABEL_ROWS) {
            assert!(row.kind.starts_with("label."), "{}", row.kind);
            assert_eq!(
                LedgerKind::parse(&row.kind),
                None,
                "{} is written",
                row.kind
            );
            assert_eq!(
                serde_json::to_string(row).unwrap(),
                bytes,
                "it keeps its bytes"
            );
        }
    }

    /// An L1 call's node as 17b's build wrote it (NODE schema 4): its gate
    /// decision names its class, and it carries no label. Literal bytes, never
    /// re-serialized (theseus-djfj). Its source: the build before 42a27af
    /// (1d33622) reads it and writes it back byte for byte.
    const NODE_SCHEMA_4: &str = r#"{"id":"tcl_00000000000000000000000000000041","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t2","loop_index":0,"origin":"harness","author":null,"created_at_ms":1790000000021,"body":{"kind":"tool_call","tool_use_id":"tu_21","tool":"proc.run","wire_name":"fs_read","input":{"path":"/w/log/tides.md"},"assistant_node":"asm_00000000000000000000000000000021","correlation_id":"act_t21","gate":{"decision":{"class":"l1","posture":"open","reason":"fs.read — open (policy.tools)"},"plan":{"class":"run","resources":[{"access":"read","path":"/w/log/tides.md"}],"summary":"read /w/log/tides.md"},"proposal":{"args":{"path":"/w/log/tides.md"},"policy_context":{"cwd":"/w","roots":["/w"]},"resource":"/w/log/tides.md","tool":"fs.read"},"result":{"gate":"allow"},"validated":true}}}"#;

    /// A labeled node as 19a's build wrote it (NODE schema 5): an owner-only
    /// file's result, with no warrant. Literal bytes (theseus-djfj). Its
    /// source: the build before 751780d (79f2d1d) reads it and writes it back
    /// byte for byte.
    const NODE_SCHEMA_5: &str = r#"{"id":"trs_00000000000000000000000000000051","schema":1,"session_id":"ses_lighthouse","turn_id":"turn_t5","loop_index":0,"origin":"tool","author":null,"created_at_ms":1790000000051,"body":{"kind":"tool_result","tool_use_id":"tu_51","tool":"fs.read","status":"ok","is_error":false,"content":"the vault code is 4417","correlation_id":"act_t51","bytes_total":22,"truncated":false,"full_ref":null,"duration_ms":3,"late":false,"meta":{}},"label":{"integrity":"trusted","readers":"owner"}}"#;

    /// A graduated node as 19c's build wrote it (NODE schema 6): its label
    /// names a place's readers and the operator's warrant. Literal bytes, in
    /// serde's order for 19c's `Label` and `Warrant`.
    const NODE_SCHEMA_6: &str = r#"{"id":"msg_00000000000000000000000000000061","schema":1,"session_id":"ses_lighthouse","turn_id":null,"loop_index":null,"origin":"operator","author":"cli","created_at_ms":1790000000061,"body":{"kind":"user_message","text":"the vault code is 4417"},"label":{"integrity":"trusted","readers":{"place":"discord:7"},"warrant":{"graduated_from":"trs_00000000000000000000000000000051","who":"cli","how":"cli","why":"the code is for the whole lab","at_ms":1790000000062}}}"#;

    /// NODE schema 7 (the place rule, theseus-nbsh): a node has no label. A
    /// schema-4 node, from before labels, reads and keeps its bytes; a
    /// schema-5 and a schema-6 node read whole, their labels left unread, and
    /// encode again without them; a node is written at schema 7 with no
    /// label.
    #[test]
    fn a_node_written_with_a_label_reads() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        assert_eq!(kinds::schema(kinds::NODE), 7);
        let old = |schema: u16, id: &str, bytes: &str| {
            NewRecord {
                schema,
                ..NewRecord::bytes(kinds::NODE, Some(id), bytes.as_bytes().to_vec())
            }
            .scoped("ses_lighthouse")
        };
        store
            .append(&[
                old(4, "tcl_00000000000000000000000000000041", NODE_SCHEMA_4),
                old(5, "trs_00000000000000000000000000000051", NODE_SCHEMA_5),
                old(6, "msg_00000000000000000000000000000061", NODE_SCHEMA_6),
            ])
            .unwrap();
        let read: Vec<Node> = store
            .session_nodes("ses_lighthouse")
            .unwrap()
            .into_iter()
            .map(|(_, n)| n)
            .collect();
        assert_eq!(read.len(), 3, "every node reads");
        let unlabeled = |bytes: &str| -> String {
            let at = bytes.find(r#","label":"#).unwrap();
            format!("{}}}", &bytes[..at])
        };
        let again: Vec<String> = read
            .iter()
            .map(|n| serde_json::to_string(n).unwrap())
            .collect();
        assert_eq!(
            again,
            [
                NODE_SCHEMA_4.to_string(),
                unlabeled(NODE_SCHEMA_5),
                unlabeled(NODE_SCHEMA_6)
            ],
            "a node from before labels keeps its bytes; a labeled one loses its label"
        );
        match &read[1].body {
            crate::node::Body::ToolResult { content, .. } => {
                assert_eq!(content, "the vault code is 4417")
            }
            other => panic!("{other:?}"),
        }
        let r = Node::user("ses_lighthouse", None, "cli", "a new node")
            .record()
            .unwrap();
        assert_eq!(r.schema, 7);
        assert!(!String::from_utf8_lossy(&r.payload).contains("label"));
    }

    /// A compilation as cache2's build wrote it (COMPILATION schema 3): its
    /// manifest has its cache layout and a context file, and no audience.
    /// Literal bytes (theseus-djfj). Its source: the build before 42a27af
    /// (1d33622) reads it and writes it back byte for byte.
    const COMPILATION_SCHEMA_3: &str = r#"{"id":"cmp_00000000000000000000000000000051","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000051,"trigger":"new_session","strategy":"transcript","as_of":17,"includes":["msg_00000000000000000000000000000051"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"context_files":[{"path":"/w/NOTES.md","digest":"a1b2c3d4e5f60718","bytes":12}],"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]}}}"#;

    /// A compilation as 19a's build wrote it (COMPILATION schema 4): its
    /// manifest records the audience, the readers, the integrity in play,
    /// and a withheld node, and its context files their readers and
    /// withholding. Literal bytes, in serde's order for 19a's types.
    const COMPILATION_SCHEMA_4: &str = r#"{"id":"cmp_00000000000000000000000000000052","schema":1,"session_id":"ses_lighthouse","created_at_ms":1790000000052,"trigger":"audience","strategy":"transcript","as_of":18,"includes":["msg_00000000000000000000000000000051"],"derived_from":null,"manifest":{"compiler_version":1,"renderer_version":2,"profile":"sonnet","provider":"anthropic","model":"claude-sonnet-5-5","system_digest":"0123456789abcdef","tools_digest":"fedcba9876543210","tools":["fs_read"],"catalog_version":"2026-10-01","context_window":1000000,"strip_thinking":false,"context_files":[{"path":"/w/NOTES.md","bytes":0,"withheld":"owner-only"},{"path":"/w/open/README.md","digest":"0f1e2d3c4b5a6978","bytes":9,"readers":"public"}],"cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]},"audience":{"kind":"place","place":"discord:314159265358979323","name":"lab","viewers":2,"digest":"0f1e2d3c4b5a6978"},"readers":{"place":"discord:314159265358979323"},"integrity":{"latched":false,"untrusted":0},"withheld":[{"node_id":"trs_00000000000000000000000000000052","reason":"owner-only"}]}}"#;

    /// COMPILATION schema 5 (the place rule, theseus-nbsh): the manifest has
    /// no audience, readers, integrity, or withheld nodes, and a context file
    /// no readers. A schema-3 compilation reads and keeps its bytes; a
    /// schema-4 one reads, its label fields left unread, and encodes again
    /// without them (a public file's mark is not read back: the next compile
    /// takes it from the config).
    #[test]
    fn a_compilation_written_with_an_audience_reads() {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path()).unwrap();
        assert_eq!(kinds::schema(kinds::COMPILATION), 5);
        let old = |schema: u16, id: &str, bytes: &str| {
            NewRecord {
                schema,
                ..NewRecord::bytes(kinds::COMPILATION, Some(id), bytes.as_bytes().to_vec())
            }
            .scoped("ses_lighthouse")
        };
        store
            .append(&[
                old(
                    3,
                    "cmp_00000000000000000000000000000051",
                    COMPILATION_SCHEMA_3,
                ),
                old(
                    4,
                    "cmp_00000000000000000000000000000052",
                    COMPILATION_SCHEMA_4,
                ),
            ])
            .unwrap();
        let both = store.session_compilations("ses_lighthouse").unwrap();
        assert_eq!(
            serde_json::to_string(&both[0]).unwrap(),
            COMPILATION_SCHEMA_3,
            "a compilation from before labels keeps its bytes"
        );
        let m = &both[1].manifest;
        assert_eq!(m.context_files[0].withheld.as_deref(), Some("owner-only"));
        assert!(!m.context_files[1].public, "the mark is not read back");
        let again = serde_json::to_string(&both[1]).unwrap();
        for field in [
            r#""audience":"#,
            r#""readers":"#,
            r#""integrity":"#,
            r#""node_id":"#,
        ] {
            assert!(!again.contains(field), "{field} is left unread: {again}");
        }
        assert!(again.ends_with(
            r#""cache":{"caches":true,"min_tokens":2048,"blocks":[{"block":"header","prefix_bytes":9000,"marked":true}]}}}"#
        ));
        let r = NewRecord::json(kinds::COMPILATION, Some(&both[1].id), &both[1]).unwrap();
        assert_eq!(r.schema, 5);
    }
}
