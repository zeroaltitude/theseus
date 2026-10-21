//! How much of Jev's ontology filing the owner took back (theseus-nngor): per
//! kind, the writes Jev made in a window and the share an `ontology.undo`
//! names. Pure over ledger rows, so the learning report's line can call it.
//!
//! A write is Jev's when its records' rows carry `undo.write` naming a
//! judgment (`jdg_…`); the sweep's and the dream's writes carry theirs under
//! other ids and are not counted. A write's kind is its records' kind, so a
//! judgment that filed a topic and a resource is one write of each.

use std::collections::{BTreeMap, BTreeSet};

use theseus_protocol::judge_runs::UndoRate;
use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::ledger::LedgerRow;
use crate::store::Store;

/// The default window: the last 7 days.
pub(crate) const WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// The ontology rows that say a write was made or undone.
const ROWS: [LedgerKind; 5] = [
    LedgerKind::OntologyCategory,
    LedgerKind::OntologyMembership,
    LedgerKind::OntologyKind,
    LedgerKind::OntologyGuidance,
    LedgerKind::OntologyUndo,
];

/// Per kind, Jev's writes at or after `since_ms` and those the owner undid,
/// kinds in name order.
pub(crate) fn undo_rates(rows: &[LedgerRow], since_ms: u64) -> Vec<UndoRate> {
    let mut kinds_of: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut undone: BTreeSet<&str> = BTreeSet::new();
    for row in rows {
        if row.kind == LedgerKind::OntologyUndo.as_str() {
            if let Some(w) = row.data["write"].as_str() {
                undone.insert(w);
            }
            continue;
        }
        let jev = row.data["undo"]["write"]
            .as_str()
            .filter(|w| w.starts_with("jdg_"));
        if let (Some(w), Some(k), true) =
            (jev, row.data["kind"].as_str(), row.at_unix_ms >= since_ms)
        {
            kinds_of.entry(w).or_default().insert(k);
        }
    }
    let mut by_kind: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for (write, ks) in &kinds_of {
        for k in ks {
            let e = by_kind.entry(k).or_default();
            e.0 += 1;
            e.1 += u32::from(undone.contains(write));
        }
    }
    by_kind
        .into_iter()
        .map(|(kind, (writes, undone))| UndoRate {
            kind: kind.into(),
            writes,
            undone,
            rate: (writes > 0).then(|| f64::from(undone) / f64::from(writes)),
        })
        .collect()
}

/// The ontology's write and undo rows since `since_ms`, in no set order, read by
/// kind through the ledger's pages. `None` while the index's shape is built.
pub(crate) fn rows_since(store: &Store, since_ms: u64) -> anyhow::Result<Option<Vec<LedgerRow>>> {
    let mut rows = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: ROWS.iter().map(|k| ledger_kind(k.as_str())).collect(),
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: None,
            limit: 256,
        };
        let Some(out) = store.ledger_page(&page)? else {
            return Ok(None);
        };
        rows.extend(out.records.iter().rev().filter_map(|r| r.decode().ok()));
        match (out.more, out.first) {
            (true, Some(first)) => before = Some(first),
            _ => return Ok(Some(rows)),
        }
    }
}
