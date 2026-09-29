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

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use theseus_store::{kinds, NewRecord, Record, Store as _, StoreStats, WalConfig, WalStore};

use crate::node::Node;

#[derive(Clone)]
pub struct Store {
    inner: Arc<WalStore>,
    dir: std::path::PathBuf,
    /// Image bytes beside the WAL, by digest (theseus-9g2).
    blobs: Arc<crate::blobs::Blobs>,
    /// A turn's handle: its waiting rows and its transcript.
    turn: Option<Arc<TurnState>>,
    /// Full transcript reads, by this store and its turn handles.
    #[cfg(test)]
    reads: Arc<std::sync::atomic::AtomicU64>,
}

/// A session's nodes with their WAL positions, in order (§4.1).
pub type Transcript = Vec<(u64, Arc<Node>)>;

/// What a turn's handle keeps (theseus-qa0).
#[derive(Default)]
struct TurnState {
    /// Rows that are no state transition, waiting for the turn's next frame.
    waiting: Mutex<Vec<NewRecord>>,
    /// The session's transcript as the turn knows it, once a reader asked.
    transcript: Mutex<Option<(String, Transcript)>>,
}

impl TurnState {
    /// Commit `records` with the waiting rows in front, as one frame, and
    /// return the positions of `records`. A frame that fails is not written,
    /// so its rows wait again for the next.
    fn commit(&self, inner: &WalStore, records: &[NewRecord]) -> Result<Vec<u64>> {
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
    /// the bytes just written. A node that does not decode drops the
    /// transcript, so the next reader reads it again from the store.
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
            (Ok(new), Some((_, nodes))) => nodes.extend(new),
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
}

impl Store {
    /// Open the store directory. A store whose manifest names another format
    /// or engine is refused.
    pub fn open(dir: &Path) -> Result<Self> {
        let inner = WalStore::open(dir, WalConfig::default())
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
            #[cfg(test)]
            reads: Default::default(),
        })
    }

    /// A handle for one turn: the same store, with the turn's own waiting
    /// rows and transcript (theseus-qa0).
    pub fn for_turn(&self) -> Store {
        Store {
            turn: Some(Arc::default()),
            ..self.clone()
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

    pub fn list_sessions<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        self.inner
            .latest_of_kind(kinds::SESSION)?
            .iter()
            .map(|r| r.decode())
            .collect()
    }

    pub fn session_count(&self) -> Result<u64> {
        Ok(self.inner.latest_of_kind(kinds::SESSION)?.len() as u64)
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
mod tests {
    use super::*;
    use crate::ledger::LedgerRow;

    fn row(kind: &str) -> NewRecord {
        let r = LedgerRow::new(kind, Some("ses_t"), Some("turn_t"), serde_json::Value::Null);
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
            reads: Default::default(),
        }
        .for_turn();
        small.put_meta("a", &1).unwrap();
        small.defer(row("loop.ended")).unwrap();
        let big = "x".repeat(1000);
        assert!(small.put_meta("b", &big).is_err(), "the WAL is full");
        let waiting = small.turn.as_ref().unwrap().waiting.lock().unwrap().len();
        assert_eq!(waiting, 1, "the row waits for the next frame");
    }
}
