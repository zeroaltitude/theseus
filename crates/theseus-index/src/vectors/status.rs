//! The vector side's status (theseus-uazd): health's numbers, answered
//! without waiting on the embedding work. The counts that need the table
//! (chunks, vectors, pending, records, dead, a re-embed's progress) are read
//! when the table's lock is free at once (`try_read`), and kept; while it is
//! held, or a writer waits for it (a compaction rewriting a file, a batch's
//! append, a reconcile), the status answers the counts it last read, with
//! when it read them (`counted_ms`), and never queues behind the writer.

use super::*;

/// The counts the table gives, as last read.
#[derive(Debug, Clone, Default)]
pub(super) struct Counts {
    chunks: u64,
    vectors: u64,
    pending: u64,
    records: u64,
    dead: u64,
    reembed: Option<Reembed>,
    at_ms: u64,
}

impl Counts {
    fn of(t: &Table) -> Counts {
        let mut c = Counts {
            at_ms: now_ms(),
            ..Counts::default()
        };
        let mut done = 0u64;
        for r in t.alive() {
            c.chunks += 1;
            if r.vec.is_some() {
                c.vectors += 1;
            }
            if matches!(r.vec, Some((0, _))) {
                done += 1;
            }
        }
        c.pending = t.backlog.len() as u64;
        let (records, dead) = t.records();
        (c.records, c.dead) = (records as u64, dead as u64);
        if t.caches.len() > 1 || !t.others.is_empty() {
            c.reembed = Some(Reembed {
                from: t
                    .caches
                    .iter()
                    .skip(1)
                    .map(|c| c.stamp.clone())
                    .chain(t.others.iter().map(|o| o.0.clone()))
                    .collect(),
                done,
                total: c.chunks,
            });
        }
        c
    }
}

impl Vectors {
    /// The table's counts: read now if its lock is free, else the last read.
    fn counts(&self) -> Counts {
        let fresh = match self.table.try_read() {
            Ok(t) => Some(Counts::of(&t)),
            Err(_) => None,
        };
        let mut last = self.counts.lock().unwrap();
        if let Some(c) = fresh {
            *last = c;
        }
        last.clone()
    }

    pub fn status(&self) -> VectorStatus {
        let st = self.state.lock().unwrap();
        let model = st.model.name().to_string();
        let since = |i: Instant| now_ms().saturating_sub(i.elapsed().as_millis() as u64);
        let mut s = VectorStatus {
            model,
            weights_dir: self
                .cfg
                .weights_dir
                .as_ref()
                .map(|d| d.display().to_string()),
            stamp: self.enabled().then(|| self.stamp.clone()),
            loads: st.loads,
            unloads: st.unloads,
            load_ms: st.load_ms,
            loaded_at_ms: st.loaded_at_ms,
            last_used_ms: since(st.last_used),
            idle_unload_secs: self.cfg.idle_unload.as_secs(),
            threads: format!(
                "RAYON_NUM_THREADS={} CANDLE_NUM_THREADS={}",
                std::env::var("RAYON_NUM_THREADS").unwrap_or_else(|_| "unset".into()),
                std::env::var("CANDLE_NUM_THREADS").unwrap_or_else(|_| "unset".into())
            ),
            ..VectorStatus::default()
        };
        if let Model::Refused { why, .. } = &st.model {
            s.last_error = Some(why.clone());
        }
        drop(st);
        let c = self.counts();
        (s.chunks, s.vectors, s.pending) = (c.chunks, c.vectors, c.pending);
        (s.records, s.dead, s.reembed) = (c.records, c.dead, c.reembed);
        s.counted_ms = c.at_ms;
        let stats = self.stats.lock().unwrap();
        s.backfill = stats.0.clone();
        if s.last_error.is_none() {
            s.last_error = stats.1.clone();
        }
        s.last_error_ms = stats.2;
        drop(stats);
        s.compactions = self.compactions.lock().unwrap().stats.clone();
        s
    }
}

#[cfg(test)]
impl Vectors {
    /// The table's lock, held as a writer holds it (a compaction, a batch's
    /// append): the status tests' stand-in for the embedding work.
    pub(crate) fn hold_table(&self) -> std::sync::RwLockWriteGuard<'_, Table> {
        self.table.write().unwrap()
    }
}
