//! The operator's labels on recalled nodes (M6 step 30b; `memory.label`,
//! §2.14): each a `memory.label` row scoped `memory`. The set recall leaves
//! out (`labeled_wrong`) is kept in memory, built after serving by one scan
//! of that scope (`Core::warm_labels`), or by its first reader, and kept
//! current by each label as it is written. A node's latest label decides:
//! `wrong` or `stale` excludes it, `useful` lets it back, and `should_have`
//! changes nothing.

use std::collections::BTreeSet;
use std::sync::PoisonError;

use anyhow::Result;
use theseus_protocol::LedgerKind;
use theseus_store::kinds;

use super::Memory;
use crate::ledger::LedgerRow;
use crate::store::Store;

/// The scope every label row is kept under.
pub const SCOPE: &str = "memory";

/// Whether `label` keeps a node out of recall; `None` for one that leaves
/// it as it was.
pub fn excludes(label: &str) -> Option<bool> {
    match label {
        "wrong" | "stale" => Some(true),
        "useful" => Some(false),
        _ => None,
    }
}

impl Memory {
    /// The nodes recall leaves out, built from the label rows the first
    /// time it is asked.
    pub fn labeled(&self, store: &Store) -> Result<BTreeSet<String>> {
        if let Some(set) = &*self.labels.read().unwrap_or_else(PoisonError::into_inner) {
            return Ok(set.clone());
        }
        let mut set = BTreeSet::new();
        for r in store.scope_after(SCOPE, 0)? {
            if r.kind != kinds::LEDGER {
                continue;
            }
            let row: LedgerRow = r.decode()?;
            if row.kind != LedgerKind::MemoryLabel.as_str() {
                continue;
            }
            let (Some(node), Some(label)) =
                (row.data["node_id"].as_str(), row.data["label"].as_str())
            else {
                continue;
            };
            apply(&mut set, node, label);
        }
        let mut held = self.labels.write().unwrap_or_else(PoisonError::into_inner);
        // A label written while this scan ran is in the store, so the scan
        // saw it; one that raced the write lock is applied over this.
        Ok(held.get_or_insert(set).clone())
    }

    /// A label just written: the set follows it, when it is built.
    pub fn labeled_now(&self, node_id: &str, label: &str) {
        if let Some(set) = &mut *self.labels.write().unwrap_or_else(PoisonError::into_inner) {
            apply(set, node_id, label);
        }
    }
}

fn apply(set: &mut BTreeSet<String>, node: &str, label: &str) {
    match excludes(label) {
        Some(true) => {
            set.insert(node.to_string());
        }
        Some(false) => {
            set.remove(node);
        }
        None => {}
    }
}
