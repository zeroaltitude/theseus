//! Pinning the history from before the chain (theseus-bmkxk). A store an older
//! build wrote holds ledger rows with no link. The first write of a chain-aware
//! build starts the chain with a `ledger.chain_started` row that counts them
//! (so the start path reads nothing that grows with history); this pass, after
//! serving, reads them once and writes `ledger.history_pinned`, whose digest
//! covers every one, so an edit of the old history is found too
//! (`chain_walk::Walk` checks the pin). It paces itself like the store's other
//! background reads, writes its row and a META marker in one frame, and does
//! nothing once the marker is there, or when no rows came before the chain.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::chain::{self, Opened};
use theseus_store::chain_walk::read_before;
use theseus_store::Store as _;
use theseus_store::{kinds, NewRecord};

use crate::ledger::LedgerRow;
use crate::rpc::Core;
use crate::store::Store;

/// How long after serving the pass begins.
pub const START_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

/// The META key that says the history was pinned.
pub const PINNED_KEY: &str = "ledger.history_pinned";

/// What a pass did.
#[derive(Debug, PartialEq, Eq)]
pub enum Pinned {
    /// No chain yet, or none of the rows came before it.
    NotNeeded,
    /// Pinned by an earlier pass.
    Already,
    /// The rows from before the chain are not the ones the chain's first row
    /// counted: nothing is pinned, and the chain's verify says so.
    Mismatch { claimed: u64, found: u64 },
    /// The pin's row is written, over this many rows.
    Written { rows: u64, position: u64 },
}

/// One pass: `between` runs after each page read.
pub fn pin(store: &Store, between: impl FnMut()) -> Result<Pinned> {
    pin_with(|| Some(store.clone()), between)
}

/// The pass, taking the store afresh each time it needs it (`None`: the
/// daemon is stopping, and the pass ends without writing).
pub fn pin_with(get: impl Fn() -> Option<Store>, between: impl FnMut()) -> Result<Pinned> {
    let stopped = || anyhow::anyhow!("the daemon is stopping");
    let store = get().ok_or_else(stopped)?;
    if store.inner().ledger_head()?.is_none() {
        return Ok(Pinned::NotNeeded);
    }
    if store.get_meta::<Value>(PINNED_KEY)?.is_some() {
        return Ok(Pinned::Already);
    }
    drop(store);
    let before = read_before(
        |after| {
            let s = get().ok_or_else(stopped)?;
            s.inner()
                .of_kind_after(kinds::LEDGER, after, theseus_store::chain_walk::PAGE)
        },
        between,
    )?;
    let store = get().ok_or_else(stopped)?;
    let Some(start) = before.start else {
        return Ok(Pinned::NotNeeded);
    };
    let Opened::Chained { canonical, .. } = chain::open(&start.payload) else {
        return Ok(Pinned::NotNeeded);
    };
    let data: Value = serde_json::from_slice::<Value>(&canonical)
        .map(|v| v["data"].clone())
        .context("the chain's first row does not read")?;
    let claimed = data["unchained_rows"].as_u64().unwrap_or(0);
    if claimed == 0 && before.history.rows == 0 {
        return Ok(Pinned::NotNeeded);
    }
    if claimed != before.history.rows {
        return Ok(Pinned::Mismatch {
            claimed,
            found: before.history.rows,
        });
    }
    let (rows, through) = (before.history.rows, before.history.through);
    let digest = chain::hex(&before.history.digest());
    let body = json!({"rows": rows, "through_position": through, "digest": digest});
    let row = LedgerRow::new(LedgerKind::LedgerHistoryPinned, None, None, body);
    let at = theseus_protocol::now_unix_ms();
    let positions = store.append(&[
        NewRecord::json(kinds::META, Some(PINNED_KEY), &json!({"pinned_at_unix_ms": at, "rows": rows, "through_position": through, "digest": digest}))?,
        NewRecord::json(kinds::LEDGER, None, &row)?,
    ])?;
    Ok(Pinned::Written {
        rows,
        position: positions[1],
    })
}

impl Core {
    /// The pass, once, on a thread of its own after serving, waiting while the
    /// machine is busy. It holds the core by `Weak`: a daemon that stops
    /// meanwhile still closes its store.
    pub fn pin_ledger_history_after_serving(self: &Arc<Self>) {
        let core = Arc::downgrade(self);
        let log = self.startup_log.clone();
        let phase = log.begin("ledger.pin", true, Instant::now());
        let spawned = std::thread::Builder::new()
            .name("ledger-pin".into())
            .spawn(move || {
                use theseus_store::pressure::{idle_this_thread, quiet_blocking, BOUND};
                let _ = idle_this_thread();
                // After a start's aftermath, as the index tender starts: a stop
                // soon after serving must find no thread holding the store (it
                // would not close it, and the next start would replay).
                std::thread::sleep(START_AFTER);
                let get = || core.upgrade().map(|c| c.store.clone());
                let detail = match pin_with(get, || {
                    quiet_blocking(BOUND);
                }) {
                    Ok(Pinned::Written { rows, position }) => {
                        json!({"outcome": "pinned", "rows": rows, "position": position})
                    }
                    Ok(Pinned::Already) => json!({"outcome": "already"}),
                    Ok(Pinned::NotNeeded) => json!({"outcome": "not needed"}),
                    Ok(Pinned::Mismatch { claimed, found }) => {
                        tracing::error!(
                            claimed,
                            found,
                            "ledger: the rows from before the chain are not the ones it counted; nothing is pinned (`theseus ledger verify` says more)"
                        );
                        json!({"outcome": "mismatch", "claimed": claimed, "found": found})
                    }
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "ledger: pinning the old history failed");
                        json!({"outcome": "failed", "error": format!("{e:#}")})
                    }
                };
                log.end(phase, detail);
            });
        if let Err(e) = spawned {
            self.startup_log
                .end(phase, json!({"outcome": "not run", "error": e.to_string()}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_store::chain_walk::verify;
    use theseus_store::wal::{Wal, WalConfig};

    fn old_row(n: u64) -> Vec<u8> {
        format!("{{\"at_unix_ms\":{n},\"kind\":\"turn.started\",\"data\":{{\"n\":{n}}}}}")
            .into_bytes()
    }

    /// A store with `old` ledger rows written by a build with no chain.
    fn store_with(old: &[Vec<u8>]) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store");
        std::fs::create_dir_all(&path).unwrap();
        {
            let wal = Wal::open(&path.join("wal"), WalConfig::default()).unwrap();
            for p in old {
                wal.append(&[NewRecord::bytes(kinds::LEDGER, None, p.clone())])
                    .unwrap();
            }
        }
        let store = Store::open(&path).unwrap();
        (dir, store)
    }

    fn row() -> LedgerRow {
        LedgerRow::new(LedgerKind::TurnStarted, Some("ses_a"), None, Value::Null)
    }

    #[test]
    fn the_history_is_pinned_once_and_the_chain_checks_it() {
        let old: Vec<Vec<u8>> = (1..=4).map(old_row).collect();
        let (_d, store) = store_with(&old);
        assert_eq!(
            pin(&store, || {}).unwrap(),
            Pinned::NotNeeded,
            "no chain yet"
        );
        store.append_ledger(&row()).unwrap();
        let mut pages = 0;
        let got = pin(&store, || pages += 1).unwrap();
        assert!(matches!(got, Pinned::Written { rows: 4, .. }), "{got:?}");
        assert_eq!(pin(&store, || {}).unwrap(), Pinned::Already);
        let r = verify(store.inner().as_ref(), None, None).unwrap();
        assert!(r.broken.is_none(), "{r:?}");
        assert!(
            r.history_checked,
            "the pin was checked against the old rows"
        );
        assert_eq!((r.unchained, r.rows), (4, 3), "start, the row, the pin");
    }

    #[test]
    fn a_fresh_store_has_nothing_to_pin() {
        let (_d, store) = store_with(&[]);
        store.append_ledger(&row()).unwrap();
        assert_eq!(pin(&store, || {}).unwrap(), Pinned::NotNeeded);
    }

    #[test]
    fn an_edit_of_the_old_history_after_the_pin_is_found() {
        let old: Vec<Vec<u8>> = (1..=4).map(old_row).collect();
        let (dir, store) = store_with(&old);
        store.append_ledger(&row()).unwrap();
        pin(&store, || {}).unwrap();
        drop(store);
        // The same log, with one old row written differently: a store forged
        // around the writer, its chain rows copied as they were.
        let mut edited = old;
        edited[2] = old_row(99);
        let sealed = Store::open(&dir.path().join("store")).unwrap();
        let chain_rows: Vec<_> = sealed.inner().of_kind_after(kinds::LEDGER, 4, 10).unwrap();
        drop(sealed);
        let forged_dir = tempfile::tempdir().unwrap();
        let path = forged_dir.path().join("store");
        std::fs::create_dir_all(&path).unwrap();
        {
            let wal = Wal::open(&path.join("wal"), WalConfig::default()).unwrap();
            for p in edited
                .iter()
                .cloned()
                .chain(chain_rows.iter().map(|r| r.payload.clone()))
            {
                wal.append(&[NewRecord::bytes(kinds::LEDGER, None, p)])
                    .unwrap();
            }
        }
        let forged = Store::open(&path).unwrap();
        let r = verify(forged.inner().as_ref(), None, None).unwrap();
        let b = r.broken.expect("the old history no longer matches its pin");
        assert_eq!(b.fault.word(), "history");
        assert_eq!(b.kind.as_deref(), Some("ledger.history_pinned"));
    }
}
