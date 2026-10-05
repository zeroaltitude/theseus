//! Every queue writes its row, in its frame (theseus-2xep, theseus-6qwr):
//! a frame that moves an execution to `queued` from any other state holds
//! that execution's `execution.queued` row, with its why. A start's requeue
//! of an interrupted turn is the one other word for it: its
//! `execution.interrupted` row.
//!
//! Read from the kernel's observer, which sees each frame whole as it
//! commits, under its executions' locks: so each execution's frames arrive
//! in their order, the racing thread's included.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use anyhow::{bail, Result};
use theseus_kernel::{Committed, LedgerRow, Observer};
use theseus_store::kinds;

/// Each execution's state as its last observed frame left it, and the frames
/// that queued one without its row.
#[derive(Default)]
struct Seen {
    state: HashMap<String, String>,
    unrowed: Vec<String>,
}

/// The watch, kept across restarts: each new kernel gets an observer onto it.
#[derive(Default, Clone)]
pub(super) struct Queues(Arc<Mutex<Seen>>);

impl Queues {
    pub(super) fn observer(&self) -> Observer {
        let seen = self.0.clone();
        Arc::new(move |c: Committed<'_>| {
            let mut s = seen.lock().unwrap_or_else(PoisonError::into_inner);
            s.frame(&c);
        })
    }

    /// Fails on the first frame that queued an execution with no row.
    pub(super) fn check(&self, at: &str) -> Result<()> {
        let s = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(first) = s.unrowed.first() {
            bail!(
                "{at}: a frame queued an execution without its execution.queued row: {first} \
                 ({} such frames)",
                s.unrowed.len()
            );
        }
        Ok(())
    }
}

impl Seen {
    fn frame(&mut self, c: &Committed<'_>) {
        // Each execution's last record in the frame is where the frame left it.
        let mut last: HashMap<String, String> = HashMap::new();
        let mut rows: Vec<(String, String)> = Vec::new();
        for r in c.records {
            if r.kind == kinds::EXECUTION {
                let Ok(v) = serde_json::from_slice::<serde_json::Value>(&r.payload) else {
                    continue;
                };
                if let (Some(id), Some(state)) = (v["id"].as_str(), v["state"].as_str()) {
                    last.insert(id.to_string(), state.to_string());
                }
            } else if r.kind == kinds::LEDGER {
                let Ok(row) = serde_json::from_slice::<LedgerRow>(&r.payload) else {
                    continue;
                };
                if let Some(id) = row.data["execution_id"].as_str() {
                    rows.push((id.to_string(), row.kind));
                }
            }
        }
        let at = c.positions.first().copied().unwrap_or_default();
        for (id, state) in last {
            let before = self.state.insert(id.clone(), state.clone());
            // An execution first seen here (one stored before the run) has no
            // known state to move from.
            let Some(before) = before else { continue };
            if state != "queued" || before == "queued" {
                continue;
            }
            let rowed = rows.iter().any(|(e, k)| {
                e == &id && (k == "execution.queued" || k == "execution.interrupted")
            });
            if !rowed {
                let kinds: Vec<&str> = rows
                    .iter()
                    .filter(|(e, _)| e == &id)
                    .map(|(_, k)| k.as_str())
                    .collect();
                self.unrowed.push(format!(
                    "{id} {before} -> queued at {at}, its rows {kinds:?}"
                ));
            }
        }
    }
}
