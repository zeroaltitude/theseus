//! The ledger rows the deploy, retire and notice steps read: a kind's rows
//! in a window, through the index's pages by kind, never a scan. Counted
//! from rows that exist (no per-call write is added anywhere): a window's
//! worth of `self.mint.deployed` is a few rows a night.

use anyhow::{bail, Result};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::ledger::LedgerRow;

/// The most rows one read decodes; past it the newest are kept.
const MAX_ROWS: usize = 5_000;

/// The rows of `names` written in `[since_ms, until_ms]`, oldest first.
pub fn between(
    store: &crate::store::Store,
    names: &[&str],
    since_ms: u64,
    until_ms: u64,
) -> Result<Vec<LedgerRow>> {
    let tags: Vec<String> = names.iter().map(|n| ledger_kind(n)).collect();
    let mut out = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: Some(since_ms),
            until_ms: Some(until_ms),
            limit: 256,
        };
        let Some(got) = store.ledger_page(&page)? else {
            bail!("the ledger's index is still being built");
        };
        for r in got.records.iter().rev() {
            if let Ok(row) = r.decode::<LedgerRow>() {
                if (since_ms..=until_ms).contains(&row.at_unix_ms) {
                    out.push(row);
                }
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) if out.len() < MAX_ROWS => before = Some(first),
            _ => break,
        }
    }
    out.reverse();
    Ok(out)
}
