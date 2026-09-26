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
use theseus_store::{kinds, Engine, NewRecord, Store as _, StoreStats, WalConfig, WalStore};

#[derive(Clone)]
pub struct Store {
    inner: Arc<WalStore>,
    dir: std::path::PathBuf,
}

impl Store {
    /// Open the store directory with the configured engine (default redb; the
    /// M1 benchmark's choice). A directory created with another engine refuses.
    pub fn open(dir: &Path, engine: Engine) -> Result<Self> {
        let inner = WalStore::open(dir, engine, WalConfig::default())
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
        })
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
}
