//! The kernel transaction (theseus-0owd; Review 2's C6): several transitions
//! in one frame.
//!
//! Frames used to merge one combined transition at a time (`admit_input`,
//! `plan_and_dispatch`, the `_with` family), and the kernel's API grew with
//! each. A transaction composes the ordinary transitions instead:
//!
//! ```text
//! kernel.frame(&[execution ids], |k| { k.bind_confirm(..)?; k.wake(..)?; Ok(()) })
//! ```
//!
//! - **The locks first.** `frame` locks every execution named, and each one's
//!   parent, in id order (K1: `lock_family`'s rule, for any number), before
//!   anything is read.
//! - **Staged, then committed.** The closure gets a view of the kernel whose
//!   transitions stage their records in memory instead of appending them, in
//!   order, each reading what the earlier ones staged (`Staged`). When it
//!   returns `Ok`, everything staged is committed as one frame, indexed, and
//!   handed to the observer once; only then are the locks released. When it
//!   fails, nothing is written.
//! - **No lock inside.** A transition on the view takes no lock of its own:
//!   it checks that the transaction holds what it touches, and a transition
//!   called on the kernel outside the view, which does lock, still panics
//!   ("locked twice on one thread"). That is the guard.
//! - **A turn ends after its frame.** A guard a transition releases inside
//!   (`end_turn`) is dropped once the frame is committed.
//! - **Nested, it joins.** `frame` on a view runs the closure in the outer
//!   transaction, and a failure takes back only what it staged.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::Result;
use theseus_store::{NewRecord, Record, RecordKind, Store, StoreStats};

use crate::kernel::{Kernel, TurnGuard};
use crate::terms;

/// A transaction in progress: what it locked, its staged frame, and the
/// turns its transitions ended.
pub(crate) struct Tx {
    /// The executions it locked, their parents included: a transition inside
    /// may touch only these.
    held: Vec<String>,
    pub(crate) staged: Arc<Staged>,
    /// Freed once the frame is committed (or the transaction fails).
    ended: Mutex<Vec<TurnGuard>>,
}

impl Tx {
    /// A transition inside the transaction would lock `ids`: each must be one
    /// the transaction holds, or K1 would not cover it.
    pub(crate) fn require(&self, ids: &[&str]) {
        for id in ids {
            if !self.held.iter().any(|h| h == id) {
                panic!(
                    "kernel: execution {id} is not locked by this transaction (name it in \
                     Kernel::frame)"
                );
            }
        }
    }

    /// A turn ended inside: its guard waits for the frame.
    pub(crate) fn release(&self, guard: TurnGuard) {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(guard);
    }

    fn ended_len(&self) -> usize {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Drop the guards released since `mark`: their turns are free.
    fn free_since(&self, mark: usize) {
        let freed: Vec<TurnGuard> = {
            let mut ended = self.ended.lock().unwrap_or_else(PoisonError::into_inner);
            let at = mark.min(ended.len());
            ended.split_off(at)
        };
        drop(freed);
    }
}

/// A transaction's store: the records staged so far, in order, read before
/// the store under them, so a transition sees what an earlier one in the same
/// transaction wrote. Nothing reaches the WAL until the transaction commits.
/// Kernel transitions read by key and by kind; every other read goes to the
/// store under it.
pub(crate) struct Staged {
    under: Arc<dyn Store>,
    frame: Mutex<Vec<NewRecord>>,
}

impl Staged {
    fn new(under: Arc<dyn Store>) -> Self {
        Self {
            under,
            frame: Mutex::default(),
        }
    }

    fn frame(&self) -> std::sync::MutexGuard<'_, Vec<NewRecord>> {
        self.frame.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// How many records are staged.
    pub(crate) fn len(&self) -> usize {
        self.frame().len()
    }

    /// Take back what was staged after the first `len` records.
    fn rewind(&self, len: usize) {
        self.frame().truncate(len);
    }

    fn take(&self) -> Vec<NewRecord> {
        std::mem::take(&mut *self.frame())
    }

    /// Each staged key of `kind` with its newest staged record, in key order.
    fn newest_staged(&self, kind: RecordKind) -> BTreeMap<String, Record> {
        let frame = self.frame();
        let mut out = BTreeMap::new();
        for (i, r) in frame.iter().enumerate() {
            if let (true, Some(k)) = (r.kind == kind, r.key.as_ref()) {
                out.insert(k.clone(), self.record(i, r));
            }
        }
        out
    }

    /// The `i`th staged record, read as the store would return it, at the
    /// position it will take if the frame commits now.
    fn record(&self, i: usize, r: &NewRecord) -> Record {
        Record {
            position: self.under.last_position() + 1 + i as u64,
            kind: r.kind,
            schema: r.schema,
            key: r.key.clone(),
            scope: r.scope.clone(),
            at_unix_ms: 0,
            payload: r.payload.clone(),
        }
    }
}

impl Store for Staged {
    fn append(&self, batch: &[NewRecord]) -> Result<Vec<u64>> {
        let mut frame = self.frame();
        let first = self.under.last_position() + 1 + frame.len() as u64;
        frame.extend_from_slice(batch);
        Ok((first..first + batch.len() as u64).collect())
    }
    fn get(&self, position: u64) -> Result<Option<Record>> {
        self.under.get(position)
    }
    fn scan(&self, from: u64, to: Option<u64>, limit: usize) -> Result<Vec<Record>> {
        self.under.scan(from, to, limit)
    }
    fn latest_by_key(&self, kind: RecordKind, key: &str) -> Result<Option<Record>> {
        let staged = {
            let frame = self.frame();
            frame
                .iter()
                .enumerate()
                .rev()
                .find(|(_, r)| r.kind == kind && r.key.as_deref() == Some(key))
                .map(|(i, r)| self.record(i, r))
        };
        match staged {
            Some(r) => Ok(Some(r)),
            None => self.under.latest_by_key(kind, key),
        }
    }
    /// The store's, in its order (by key), with each key's newest staged
    /// record in its place, or in key order when the store has none.
    fn latest_of_kind(&self, kind: RecordKind) -> Result<Vec<Record>> {
        let mut all = self.under.latest_of_kind(kind)?;
        debug_assert!(
            all.windows(2).all(|w| w[0].key < w[1].key),
            "the store's latest_of_kind is in key order (the Store contract)"
        );
        let staged: Vec<Record> = {
            let frame = self.frame();
            frame
                .iter()
                .enumerate()
                .filter(|(_, r)| r.kind == kind && r.key.is_some())
                .map(|(i, r)| self.record(i, r))
                .collect()
        };
        for r in staged {
            let key = r.key.clone().unwrap_or_default();
            match all.binary_search_by(|x| {
                x.key
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes()
                    .cmp(key.as_bytes())
            }) {
                Ok(i) => all[i] = r,
                Err(i) => all.insert(i, r),
            }
        }
        Ok(all)
    }
    fn tail_of_kind(&self, kind: RecordKind, n: usize) -> Result<Vec<Record>> {
        self.under.tail_of_kind(kind, n)
    }
    fn count_of_kind(&self, kind: RecordKind) -> Result<u64> {
        self.under.count_of_kind(kind)
    }
    fn scan_scope(&self, scope: &str, after: u64, limit: usize) -> Result<Vec<Record>> {
        self.under.scan_scope(scope, after, limit)
    }
    fn count_in_scope(&self, scope: &str) -> Result<u64> {
        self.under.count_in_scope(scope)
    }
    fn last_position(&self) -> u64 {
        self.under.last_position()
    }
    fn checkpoint(&self) -> Result<u64> {
        self.under.checkpoint()
    }
    fn stats(&self) -> Result<StoreStats> {
        self.under.stats()
    }
    /// The store's, with each staged key's newest record in its place when
    /// its terms are in the range, and out of it when they are not
    /// (theseus-lv2). The store's terms are the kernel's (`terms::PROJECTION`).
    fn latest_by_terms(&self, kind: RecordKind, lo: &str, hi: &str) -> Result<Option<Vec<Record>>> {
        let Some(mut found) = self.under.latest_by_terms(kind, lo, hi)? else {
            return Ok(None);
        };
        for (key, r) in self.newest_staged(kind) {
            let at = found.binary_search_by(|x| {
                x.key
                    .as_deref()
                    .unwrap_or_default()
                    .as_bytes()
                    .cmp(key.as_bytes())
            });
            let wanted = in_range(&terms::of(kind, &r.payload), lo, hi) > 0;
            match (at, wanted) {
                (Ok(i), true) => found[i] = r,
                (Ok(i), false) => {
                    found.remove(i);
                }
                (Err(i), true) => found.insert(i, r),
                (Err(_), false) => {}
            }
        }
        Ok(Some(found))
    }
    /// The store's count, with each staged key's newest record's terms in
    /// place of what the store counted for it.
    fn count_by_terms(&self, kind: RecordKind, lo: &str, hi: &str) -> Result<Option<u64>> {
        let Some(mut n) = self.under.count_by_terms(kind, lo, hi)? else {
            return Ok(None);
        };
        for (key, r) in self.newest_staged(kind) {
            let before = match self.under.latest_by_key(kind, &key)? {
                Some(old) => in_range(&terms::of(kind, &old.payload), lo, hi),
                None => 0,
            };
            n = (n + in_range(&terms::of(kind, &r.payload), lo, hi)).saturating_sub(before);
        }
        Ok(Some(n))
    }
    fn latest_with_prefix(&self, kind: RecordKind, prefix: &str) -> Result<Vec<Record>> {
        let mut found: BTreeMap<String, Record> = self
            .under
            .latest_with_prefix(kind, prefix)?
            .into_iter()
            .filter_map(|r| Some((r.key.clone()?, r)))
            .collect();
        for (key, r) in self.newest_staged(kind) {
            if key.starts_with(prefix) {
                found.insert(key, r);
            }
        }
        Ok(found.into_values().collect())
    }
    fn count_keys(&self, kind: RecordKind) -> Result<u64> {
        let mut n = self.under.count_keys(kind)?;
        for key in self.newest_staged(kind).keys() {
            if self.under.latest_by_key(kind, key)?.is_none() {
                n += 1;
            }
        }
        Ok(n)
    }
    /// The store's: nothing in a transaction adds anything up. A staged
    /// record counts once its frame commits.
    fn totals(&self, kind: RecordKind) -> Result<Option<theseus_store::Sums>> {
        self.under.totals(kind)
    }
}

/// How many of `terms` are in `lo..hi`.
fn in_range(terms: &[String], lo: &str, hi: &str) -> u64 {
    terms
        .iter()
        .filter(|t| lo <= t.as_str() && t.as_str() < hi)
        .count() as u64
}

impl Kernel {
    /// A kernel transaction (theseus-0owd; Review 2's C6): run `f` on a view
    /// of this kernel whose transitions stage their records, and commit what
    /// they staged as one frame. It locks `ids` and each one's parent first,
    /// in id order; a transition inside takes no lock of its own, and may
    /// touch only those. `Ok`: the frame is committed (if anything was
    /// staged), indexed, and observed once, and then the locks are released.
    /// `Err`: nothing is written. On a view, it joins the outer transaction.
    pub fn frame<T>(&self, ids: &[&str], f: impl FnOnce(&Kernel) -> Result<T>) -> Result<T> {
        if let Some(tx) = self.tx() {
            // Joined: the outer transaction holds the locks and commits. A
            // failure here takes back only what this part staged.
            for id in ids {
                tx.require(&[id]);
            }
            let (staged, ended) = (tx.staged.len(), tx.ended_len());
            let out = f(self);
            if out.is_err() {
                tx.staged.rewind(staged);
                tx.free_since(ended);
            }
            return out;
        }
        // The family: each execution and its parent, which never changes, so
        // reading it outside the lock is sound (`lock_family`).
        let mut held: Vec<String> = Vec::with_capacity(ids.len() * 2);
        for id in ids {
            held.push(id.to_string());
            if let Some(p) = self.execution(id)?.and_then(|e| e.parent) {
                held.push(p);
            }
        }
        held.sort_unstable();
        held.dedup();
        let refs: Vec<&str> = held.iter().map(String::as_str).collect();
        let _locked = self.locks().lock_all(&refs);
        let staged = Arc::new(Staged::new(self.store().clone()));
        let tx = Arc::new(Tx {
            held,
            staged: staged.clone(),
            ended: Mutex::default(),
        });
        let out = {
            let view = self.transaction_view(tx.clone(), staged.clone());
            f(&view)?
        };
        let frame = staged.take();
        if !frame.is_empty() {
            self.commit(&frame)?;
        }
        // The turns its transitions ended are free once their end is written,
        // and before the locks go.
        tx.free_since(0);
        Ok(out)
    }

    /// Records of the caller's, in this transaction's frame after what is
    /// staged so far: a turn's node, a task's report, an answer's row. Only
    /// inside a transaction (`frame`).
    pub fn stage(&self, records: &[NewRecord]) -> Result<()> {
        if self.tx().is_none() {
            anyhow::bail!("Kernel::stage outside a transaction (Kernel::frame)");
        }
        if !records.is_empty() {
            self.commit(records)?;
        }
        Ok(())
    }

    /// How many records this transaction has staged so far (0 outside one):
    /// a composition's way to tell whether a transition wrote.
    pub(crate) fn staged_len(&self) -> usize {
        self.tx().map_or(0, |tx| tx.staged.len())
    }
}
