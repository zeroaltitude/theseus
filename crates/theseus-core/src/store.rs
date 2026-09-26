//! A first `Store`: session records and ledger rows in an embedded store
//! (redb). M0 persists what it did; WAL discipline, checkpoints, and the
//! benchmark against fjall arrive with M1 Keel.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{de::DeserializeOwned, Serialize};

const SESSIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("sessions");
const LEDGER: TableDefinition<u64, &[u8]> = TableDefinition::new("ledger");
/// Small runtime state that must survive restarts (e.g. the live profile).
const META: TableDefinition<&str, &[u8]> = TableDefinition::new("meta");

#[derive(Clone)]
pub struct Store {
    db: Arc<Database>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let db =
            Database::create(path).with_context(|| format!("opening store {}", path.display()))?;
        // Make sure both tables exist so reads never fail on a fresh store.
        let txn = db.begin_write()?;
        {
            txn.open_table(SESSIONS)?;
            txn.open_table(LEDGER)?;
            txn.open_table(META)?;
        }
        txn.commit()?;
        Ok(Self { db: Arc::new(db) })
    }

    pub fn put_meta<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(META)?;
            t.insert(key, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_meta<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(META)?;
        match t.get(key)? {
            Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
            None => Ok(None),
        }
    }

    pub fn put_session<T: Serialize>(&self, id: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        let txn = self.db.begin_write()?;
        {
            let mut t = txn.open_table(SESSIONS)?;
            t.insert(id, bytes.as_slice())?;
        }
        txn.commit()?;
        Ok(())
    }

    pub fn get_session<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(SESSIONS)?;
        match t.get(id)? {
            Some(v) => Ok(Some(serde_json::from_slice(v.value())?)),
            None => Ok(None),
        }
    }

    pub fn list_sessions<T: DeserializeOwned>(&self) -> Result<Vec<T>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(SESSIONS)?;
        let mut out = Vec::new();
        for row in t.iter()? {
            let (_, v) = row?;
            out.push(serde_json::from_slice(v.value())?);
        }
        Ok(out)
    }

    pub fn session_count(&self) -> Result<u64> {
        let txn = self.db.begin_read()?;
        Ok(txn.open_table(SESSIONS)?.len()?)
    }

    /// Append a ledger row; returns its position.
    pub fn append_ledger<T: Serialize>(&self, row: &T) -> Result<u64> {
        let bytes = serde_json::to_vec(row)?;
        let txn = self.db.begin_write()?;
        let pos = {
            let mut t = txn.open_table(LEDGER)?;
            let next = t.last()?.map(|(k, _)| k.value() + 1).unwrap_or(1);
            t.insert(next, bytes.as_slice())?;
            next
        };
        txn.commit()?;
        Ok(pos)
    }

    pub fn ledger_len(&self) -> Result<u64> {
        let txn = self.db.begin_read()?;
        Ok(txn.open_table(LEDGER)?.len()?)
    }

    pub fn ledger_tail<T: DeserializeOwned>(&self, n: usize) -> Result<Vec<(u64, T)>> {
        let txn = self.db.begin_read()?;
        let t = txn.open_table(LEDGER)?;
        let mut out = Vec::new();
        for row in t.iter()?.rev().take(n) {
            let (k, v) = row?;
            out.push((k.value(), serde_json::from_slice(v.value())?));
        }
        out.reverse();
        Ok(out)
    }
}
