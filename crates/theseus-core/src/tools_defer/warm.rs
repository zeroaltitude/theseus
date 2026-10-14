//! The counts, read from the ledger (theseus-7m7w.2): once after serving, on
//! the blocking pool, never on the start path. The rows of the selector's
//! window (`tools.used`, `tools.searched`, `tools.missed`) are paged by kind
//! from the newest back; until the index's shape is built the read waits on
//! tokio's timer and asks again. A turn that ends first counts live, and the
//! read skips the rows from that time on (`Board`'s `live_since`).

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::LedgerKind;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use super::board::Board;
use super::select::DAY_MS;
use crate::ledger::LedgerRow;
use crate::store::Store;

/// How long to wait before asking again while the index is built.
const INDEX_WAIT: Duration = Duration::from_secs(2);

/// Read the rows of the last `days` days into `board`. `Ok(false)`: the
/// index is not ready, ask again.
pub fn read(store: &Store, board: &Board, now_ms: u64, days: u64) -> anyhow::Result<bool> {
    let since_ms = now_ms.saturating_sub(days * DAY_MS);
    let tags: Vec<String> = [
        LedgerKind::ToolsUsed,
        LedgerKind::ToolsSearched,
        LedgerKind::ToolsMissed,
    ]
    .iter()
    .map(|k| ledger_kind(k.as_str()))
    .collect();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: None,
            limit: 512,
        };
        let Some(got) = store.ledger_page(&page)? else {
            return Ok(false);
        };
        for r in &got.records {
            let Ok(row) = r.decode::<LedgerRow>() else {
                continue;
            };
            if row.at_unix_ms < since_ms {
                continue;
            }
            match row.kind.as_str() {
                k if k == LedgerKind::ToolsUsed.as_str() => {
                    board.apply_used(row.at_unix_ms, &row.data)
                }
                k if k == LedgerKind::ToolsSearched.as_str() => {
                    board.apply_searched(row.at_unix_ms, &row.data)
                }
                k if k == LedgerKind::ToolsMissed.as_str() => {
                    board.apply_missed(row.at_unix_ms, &row.data)
                }
                _ => {}
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => break,
        }
    }
    board.set_warmed();
    Ok(true)
}

impl crate::rpc::Core {
    /// Read the tool use the ledger holds into the board, after serving.
    pub fn warm_tools_use(self: &Arc<Self>) {
        let cfg = &self.runner.cfg.tools.defer;
        if !cfg.enabled {
            return;
        }
        let days = u64::from(cfg.window_days.max(cfg.hide_after_days)) + 1;
        let (core, store) = (Arc::downgrade(self), self.store.clone());
        tokio::spawn(async move {
            loop {
                let c = core.clone();
                let s = store.clone();
                let done = tokio::task::spawn_blocking(move || {
                    let core = c.upgrade()?;
                    Some(read(
                        &s,
                        &core.runner.tools.defer,
                        theseus_protocol::now_unix_ms(),
                        days,
                    ))
                })
                .await;
                match done {
                    Ok(Some(Ok(true))) | Ok(None) => return,
                    Ok(Some(Ok(false))) => tokio::time::sleep(INDEX_WAIT).await,
                    Ok(Some(Err(e))) => {
                        tracing::warn!(error = %format!("{e:#}"), "tools: the ledger's tool use cannot be read; every place loads its defaults");
                        return;
                    }
                    Err(_) => return,
                }
            }
        });
    }
}
