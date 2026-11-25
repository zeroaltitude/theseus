
impl Inner {

    /// Write, index, answer, and sync one batch (theseus-7yhp: the sync
    /// comes after the answers, so a turn's model request leaves while the
    /// disk works).
    pub(super) fn commit(&self, mut batch: Vec<Job>) {
        let began = std::time::Instant::now();
        if let Some(why) = self.answered_lost.lock().unwrap().clone() {
            // The index holds frames a failed sync cut, so no position of
            // theirs may be written again (`say_lost`).
            for job in batch {
                self.queued.fetch_sub(1, Ordering::SeqCst);
                let _ = job.answer.send(Err(anyhow::anyhow!("{why}")));
            }
            return;
        }
        if let Err(e) = self.upgrade_manifest() {
            // Nothing is written under a manifest an older build opens.
            for job in batch {
                self.queued.fetch_sub(1, Ordering::SeqCst);
                let _ = job.answer.send(Err(anyhow::anyhow!("{e:#}")));
            }
            return;
        }
        let (written, chained) = self.write_batch(&mut batch);
        let wrote = written
            .iter()
            .any(|w| w.as_ref().is_ok_and(|(p, _)| !p.is_empty()));
        // The index, in one transaction, before the sync: a reader may see a
        // frame the disk has yet to take (theseus-7yhp; the owner, 2026-11-12
        // 19:24, answering exactly this: "YES, OPTION (A) -- the index may
        // show a provider call's plan frame before that frame's sync has
        // landed"). The answer still comes after the index, so a caller's
        // lock spans its read to its frame indexed (K1).
        let (entries, records) = Self::entries_of(&batch, &written);
        let indexed = if entries.is_empty() {
            Ok(())
        } else {
            self.index_batch(&entries)
        };
        self.after_index(&indexed, records);
        if indexed.is_err() {
            // A batch the index refused is answered after its sync, exactly
            // as every batch was before theseus-7yhp: nothing is waiting on
            // a failure, and its gap must be open, and its chain head
            // settled, before its caller is told.
            let synced = self.sync_batch(wrote);
            self.after_sync(&synced, &indexed, records, chained);
            if synced.is_err()
                && written.iter().any(|w| {
                    w.as_ref().is_ok_and(|(p, _)| {
                        p.last()
                            .is_some_and(|(at, _)| *at <= self.wal.last_position())
                    })
                })
            {
                // A roll synced some frames, but the final sync cut others.
                // Reopen must rebuild the chain head from the surviving WAL.
                let why = "store: an unindexed batch only partly survived a failed sync; no further frame and no checkpoint until reopen";
                tracing::error!("{why}");
                *self.answered_lost.lock().unwrap() = Some(why.into());
            }
            self.answer_all(batch, written, &indexed, began);
            return;
        }
        // The chain head follows the log, which has taken these frames (one
        // it refused gave its links back above). The batch is answered
        // before its sync, so a caller holding its answer reads the head its
        // own rows left; a sync that fails here stops the store, so no later
        // row chains on what it lost.
        *self.chain.lock().unwrap() = chained.clone();
        self.answer_all(batch, written, &indexed, began);
        let synced = self.sync_batch(wrote);
        self.after_sync(&synced, &indexed, records, chained);
    }
}
