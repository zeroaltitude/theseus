//! `index.rebuild` through the core (theseus-jn17m): the owner's act, forwarded
//! to the tender, with the rows that make it the daemon's own job on the work
//! board (`Rebuilding the index`). The tender used to take the request on its
//! own socket (`theseus-index rebuild`), where the core wrote nothing; here
//! the core writes `index.rebuild` rows: `requested`, then `progress` as the
//! tender's backfill passes each fifth of the WAL, then `finished` (or
//! `failed`). A rebuild asked on the tender's own socket still runs, and
//! shows nowhere: that is the one way to have no row.
//!
//! The wait is a task on tokio's timer, one status read every `POLL` while a
//! rebuild is outstanding (the tender's own status is a bounded read), holding
//! the core by `Weak`. It writes at most `1 + 100 / STEP + 1` frames.

use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::bail;
use serde_json::Value;
use theseus_protocol::index::{method as tender, IndexRebuildResult, IndexStatus};

use super::judge_runs::failure;
use super::server::{Conn, RpcFailure};
use super::Core;
use crate::fact::index::Rebuild;
use crate::tender::TenderMiss;

/// How often an outstanding rebuild's status is read.
const POLL: Duration = Duration::from_secs(3);

/// The share of the WAL (in points) between two progress rows.
const STEP: u64 = 5;

/// Status reads that may miss, one after another, before the wait gives up.
const MISSES: u32 = 20;

/// The longest the wait outlasts a rebuild: six hours.
const GIVE_UP: Duration = Duration::from_secs(6 * 3600);

impl Core {
    /// `index.rebuild`, from the dispatch.
    pub(super) async fn rpc_index_rebuild(
        self: &Arc<Self>,
        conn: Conn<'_>,
    ) -> Result<Value, RpcFailure> {
        let who = conn.answerer(None, None);
        let r = self.index_rebuild(who).await.map_err(failure)?;
        super::server::reply(r)
    }

    /// Ask the tender to drop the index and build it again, and watch it.
    pub async fn index_rebuild(
        self: &Arc<Self>,
        who: impl Into<crate::approval::Answerer>,
    ) -> anyhow::Result<IndexRebuildResult> {
        let who = who.into();
        self.judge_act(
            &who,
            crate::rpc::Act::JudgeRun {
                method: theseus_protocol::method::INDEX_REBUILD,
                what: "an index rebuild",
            },
        )?;
        let before: IndexStatus = self.index.ask(tender::STATUS, ()).await.map_err(miss)?;
        let r: IndexRebuildResult = self.index.ask(tender::REBUILD, ()).await.map_err(miss)?;
        if !r.accepted {
            bail!("the index tender did not accept the rebuild");
        }
        let run = theseus_protocol::now_unix_ms();
        self.rebuild_row(Rebuild {
            run,
            event: "requested",
            by: Some(&who.label),
            done_bytes: None,
            total_bytes: None,
            why: None,
        });
        tokio::spawn(wait(Arc::downgrade(self), run, before.rebuilds));
        Ok(r)
    }

    /// One `index.rebuild` row, written off the runtime's workers.
    fn rebuild_row(self: &Arc<Self>, f: Rebuild<'_>) {
        use crate::fact::Fact;
        let Some(kind) = Rebuild::KIND else { return };
        let row = crate::ledger::LedgerRow::new(kind, None, None, f.row());
        let core = self.clone();
        // A row is an fsync: off the runtime's workers.
        theseus_store::blocking(move || {
            if let Err(e) = core.store.append_ledger(&row) {
                tracing::warn!(error = %e, "index.rebuild row not written");
            }
        });
    }
}

fn miss(m: TenderMiss) -> anyhow::Error {
    match m {
        TenderMiss::Down(w) => anyhow::anyhow!("the index tender is not answering: {w}"),
        TenderMiss::Refused(w) => anyhow::anyhow!("the index tender refused: {w}"),
    }
}

/// Watch a rebuild to its end, writing its progress and its end.
async fn wait(core: Weak<Core>, run: u64, before: u64) {
    let (started, mut misses, mut last) = (Instant::now(), 0u32, 0u64);
    loop {
        tokio::time::sleep(POLL).await;
        let Some(c) = core.upgrade() else { return };
        // A stop ends the wait; the next start has no row to finish, and
        // the board settles a job whose rows stopped.
        if c.outbox.stopping() {
            return;
        }
        let status: Result<IndexStatus, TenderMiss> = c.index.ask(tender::STATUS, ()).await;
        let row = |event, done, total, why| Rebuild {
            run,
            event,
            by: None,
            done_bytes: done,
            total_bytes: total,
            why,
        };
        match status {
            Ok(s) if s.rebuilds > before && s.state == "ready" => {
                c.rebuild_row(row("finished", None, None, None));
                return;
            }
            Ok(s) => {
                misses = 0;
                if let Some(b) = s.backfill.filter(|_| s.rebuilds > before) {
                    let pct = (b.done_bytes * 100).checked_div(b.total_bytes).unwrap_or(0);
                    if pct >= last + STEP {
                        last = pct / STEP * STEP;
                        c.rebuild_row(row(
                            "progress",
                            Some(b.done_bytes),
                            Some(b.total_bytes),
                            None,
                        ));
                    }
                }
            }
            Err(m) => {
                misses += 1;
                if misses >= MISSES {
                    let why = format!("{}; the rebuild may still run", miss(m));
                    c.rebuild_row(row("failed", None, None, Some(&why)));
                    return;
                }
            }
        }
        if started.elapsed() > GIVE_UP {
            let why = "the rebuild had not finished after six hours";
            c.rebuild_row(row("failed", None, None, Some(why)));
            return;
        }
    }
}
