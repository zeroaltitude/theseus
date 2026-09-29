//! The kernel's view of storage: sessions, ledger rows, and small runtime
//! state, written through `theseus_store::WalStore` (spec §6, M1 Keel).
//!
//! Every write is a WAL frame, durable when the call returns. Sessions and
//! meta are "latest by key"; the ledger is an append-only kind. The index is
//! rebuilt from the WAL on open if it lost anything, so this module never
//! has to think about recovery.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use theseus_store::{kinds, NewRecord, Store as _, StoreStats, WalConfig, WalStore};

#[derive(Clone)]
pub struct Store {
    inner: Arc<WalStore>,
    dir: std::path::PathBuf,
    /// Image bytes beside the WAL, by digest (theseus-9g2).
    blobs: Arc<crate::blobs::Blobs>,
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
        })
    }

    /// The store's image blobs (theseus-9g2).
    pub fn blobs(&self) -> &crate::blobs::Blobs {
        &self.blobs
    }

    /// The same store as the kernel's `Store` trait object (one WAL, one index).
    pub fn shared(&self) -> Arc<dyn theseus_store::Store> {
        self.inner.clone()
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
        self.inner
            .append(&[NewRecord::json(kinds::META, Some(key), value)?])?;
        Ok(())
    }

    pub fn get_meta<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.inner.latest_by_key(kinds::META, key)? {
            Some(r) => Ok(Some(r.decode()?)),
            None => Ok(None),
        }
    }

    pub fn put_session<T: Serialize>(&self, id: &str, value: &T) -> Result<()> {
        self.inner
            .append(&[NewRecord::json(kinds::SESSION, Some(id), value)?])?;
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
        let p = self
            .inner
            .append(&[NewRecord::json(kinds::LEDGER, None, row)?])?;
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
        self.inner.append(records)
    }

    /// Every node of a session with its WAL position, in order (§4.1: order is positional).
    pub fn session_nodes(&self, session_id: &str) -> Result<Vec<(u64, crate::node::Node)>> {
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
