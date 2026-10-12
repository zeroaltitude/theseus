//! The writer's half of the ledger's hash chain (theseus-bmkxk): in `commit`,
//! before a frame is written, each ledger record gets its link, in WAL order,
//! and the head moves only once the batch is durable and indexed. `chain.rs`
//! says what a link is; `chain_walk.rs` is its one reader.

use super::*;
use crate::chain::{self, Head, Opened};
use crate::record::kinds;

/// Where the chain is, as the writer last committed it.
#[derive(Debug, Clone, Default)]
pub(super) struct ChainState {
    /// The head has been read from the store (or the chain started here).
    pub loaded: bool,
    /// The last link, if the chain has started.
    pub head: Option<Head>,
}

impl Inner {
    /// The head, as the last chained ledger record in the store says it:
    /// the newest few ledger records are looked at, so a row that carries no
    /// link of its own never restarts the chain.
    pub(super) fn read_chain_head(&self) -> Result<Option<Head>> {
        let mut recent = self.index.positions_of_kind_rev(kinds::LEDGER, 8)?;
        recent.sort_unstable();
        for r in self.read_many(&recent)?.into_iter().rev() {
            if let Opened::Chained { link, .. } = chain::open(&r.payload) {
                return Ok(Some(Head {
                    link,
                    position: r.position,
                }));
            }
        }
        Ok(None)
    }

    /// Chain one job's ledger records onto `st`, starting the chain first
    /// when this is the first ledger row a chain-aware build writes.
    pub(super) fn chain_job(&self, st: &mut ChainState, job: &mut Job) -> Result<()> {
        #[cfg(test)]
        if self.chain_off.load(Ordering::Relaxed) {
            return Ok(());
        }
        let first = job
            .records
            .iter()
            .position(|r| r.kind == kinds::LEDGER && chain::chainable(&r.payload));
        let Some(first) = first else { return Ok(()) };
        if !st.loaded {
            st.head = self.read_chain_head()?;
            st.loaded = true;
        }
        let mut prev = match st.head {
            Some(h) => h.link,
            None => {
                // The chain starts here: a row that says how much history came
                // before it, in this frame, ahead of the caller's first row.
                let unchained = self.index.count_of_kind(kinds::LEDGER)?;
                let through = self
                    .index
                    .positions_of_kind_rev(kinds::LEDGER, 1)?
                    .first()
                    .copied()
                    .unwrap_or(0);
                let started = chain::started_row(crate::record::now_unix_ms(), unchained, through);
                let (payload, link) = chain::seal(&started, &chain::GENESIS, 0)
                    .expect("the started row is an object");
                let rec = NewRecord::bytes(kinds::LEDGER, None, payload);
                job.projected.insert(
                    first,
                    (
                        self.projection
                            .and_then(|p| p.of(rec.kind, None, &rec.payload)),
                        self.projection
                            .and_then(|p| p.sums_of(rec.kind, None, &rec.payload)),
                    ),
                );
                job.tags.insert(first, tags_of(rec.kind, &rec.payload));
                job.records.insert(first, rec);
                job.hidden = Some(first);
                st.head = Some(Head { link, position: 0 });
                link
            }
        };
        let hidden = job.hidden;
        for (i, r) in job.records.iter_mut().enumerate() {
            if Some(i) == hidden || r.kind != kinds::LEDGER || !chain::chainable(&r.payload) {
                continue;
            }
            let seq = prev.seq + 1;
            let (payload, link) =
                chain::seal(&r.payload, &prev.digest, seq).expect("checked chainable");
            r.payload = payload;
            prev = link;
        }
        st.head = Some(Head {
            link: prev,
            position: 0,
        });
        Ok(())
    }

    /// After a job's frame is placed: the head's position is its last ledger record's.
    pub(super) fn chain_placed(st: &mut ChainState, job: &Job, placed: &[(u64, RecordLocation)]) {
        let last = job.records.iter().zip(placed).rfind(|(r, _)| {
            r.kind == kinds::LEDGER && !matches!(chain::open(&r.payload), Opened::Unchained)
        });
        if let (Some((_, (pos, _))), Some(h)) = (last, st.head.as_mut()) {
            h.position = *pos;
        }
    }
}
