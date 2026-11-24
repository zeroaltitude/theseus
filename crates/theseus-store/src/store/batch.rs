//! Writing, indexing, answering, and syncing one writer batch.

use super::*;

impl Inner {
    /// The batch's index write, tried up to `INDEX_TRIES` times: its frames are
    /// written but not necessarily synced, and `apply` over what a failed one left equals one
    /// whole `apply` (`tests/index_reapply.rs`), so a retry is safe. The tries
    /// follow each other at once: a full disk is met three times, and what
    /// they buy is transient contention on redb's write lock. A failure past
    /// the bound is the caller's, as it was before the retry.
    fn index_batch(&self, entries: &[IndexEntry]) -> Result<()> {
        let mut tries = 0;
        loop {
            tries += 1;
            let injected = self
                .index_fail
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
            let wrote = if injected {
                Err(anyhow::anyhow!("an injected index failure"))
            } else {
                let applied = self.index.apply(entries, false);
                #[cfg(test)]
                let applied = applied.and_then(|()| {
                    match self.index_fail_after.fetch_update(
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                        |n| n.checked_sub(1),
                    ) {
                        Ok(_) => Err(anyhow::anyhow!("an injected failure after the write")),
                        Err(_) => Ok(()),
                    }
                });
                applied
            };
            match wrote {
                Ok(()) => return Ok(()),
                Err(e) if tries >= INDEX_TRIES => {
                    return Err(e.context(format!("the index write failed {tries} times")))
                }
                Err(e) => tracing::warn!(tries, "the index write failed, trying again: {e:#}"),
            }
        }
    }

    /// The batch's bookkeeping once its index write is over. The ledger's head
    /// follows the log: it moves once the frames are synced, whatever the
    /// index did, since a failed index write leaves them durable and the next
    /// open indexes them, and a head left behind would chain two rows on one
    /// link. A failed sync cut its frames, so the head stays. An index write
    /// that failed past its bound opens the gap checkpoints refuse across.
    /// The bookkeeping the batch's index write decides, before its answers
    /// (theseus-7yhp): a caller holding its answer reads a store that has
    /// counted its frames.
    fn after_index(&self, indexed: &Result<()>, records: u64) {
        if indexed.is_ok() {
            self.since_checkpoint.fetch_add(records, Ordering::Relaxed);
            if records > 0 {
                self.index_gap.took();
            }
        }
    }

    /// The bookkeeping its sync decides. The chain head follows the log, so
    /// a batch the index took has moved it already (`commit`); here is where
    /// a batch the index refused moves it, and where a sync that failed
    /// after its answers stops the store.
    fn after_sync(
        &self,
        synced: &Result<()>,
        indexed: &Result<()>,
        records: u64,
        chained: chain_writer::ChainState,
    ) {
        match (synced, indexed) {
            (Ok(()), _) => *self.chain.lock().unwrap() = chained,
            // The batch was answered before this sync, so the frames the log
            // has just cut are in the index and in no caller's doubt
            // (theseus-7yhp). An index write that failed answered them
            // failed, so nothing holds them and the cut is as it was.
            (Err(why), Ok(())) if records > 0 => self.say_lost(why),
            (Err(_), _) => {}
        }
        if let Err(why) = indexed {
            if synced.is_ok() {
                self.say_gap(why);
            }
        }
    }

    /// A sync that failed after its batch was answered and indexed
    /// (theseus-7yhp): the log cut those frames and the index holds them, so
    /// their positions must never be written again. The store takes no
    /// further frame and no checkpoint until it is reopened, and every
    /// refused caller is told why. Said once, loudly.
    fn say_lost(&self, why: &anyhow::Error) {
        let mut lost = self.answered_lost.lock().unwrap();
        if lost.is_some() {
            return;
        }
        let at = self.wal.last_position();
        let said = format!(
            "store: a sync failed after its frames were answered and indexed ({why:#}); the log \
             cut them back to position {at} and the index still holds them, so this store takes \
             no further frame and no checkpoint until it is reopened, and the next open reads the \
             log, which is the truth"
        );
        tracing::error!(position = at, "{said}");
        *lost = Some(said);
    }

    /// Opens the gap, saying so once (the first failure's position stands); a
    /// later batch that fails the same way is counted and quiet. Health and
    /// the owner's notice carry the gap (`ledger_gap`).
    fn say_gap(&self, why: &anyhow::Error) {
        let at = self.wal.last_position().max(1);
        if self.index_gap.failed(at, why) {
            #[cfg(test)]
            self.warned.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                position = at,
                reason = %format!("{why:#}"),
                "store: the log holds frames the index lacks (up to this position); no checkpoint \
                 is taken until a restart, and the next open replays them"
            );
        }
    }

    /// The index entries of a batch's written frames, and how many records
    /// they hold: one transaction's worth (`index_batch`).
    fn entries_of(batch: &[Job], written: &[Result<Placed>]) -> (Vec<IndexEntry>, u64) {
        let mut records = 0u64;
        let mut entries = Vec::new();
        for (job, placed) in batch.iter().zip(written) {
            let Ok((placed, at)) = placed else { continue };
            records += placed.len() as u64;
            for ((((pos, loc), r), (terms, sums)), tags) in placed
                .iter()
                .zip(&job.records)
                .zip(&job.projected)
                .zip(&job.tags)
            {
                entries.push(IndexEntry {
                    position: *pos,
                    at_unix_ms: *at,
                    kind: r.kind,
                    key: r.key.clone(),
                    scope: r.scope.clone(),
                    loc: *loc,
                    terms: terms.clone(),
                    sums: *sums,
                    tags: tags.clone(),
                });
            }
        }
        (entries, records)
    }

    /// Answer every job of a batch, each with its frame's positions or the
    /// reason it has none, and note the time the writer took over each frame
    /// it answers Ok (theseus-w7dk). `began` is when the writer took the
    /// batch; the sync runs after this (theseus-7yhp), so the time is what
    /// the caller waited.
    pub(super) fn answer_all(
        &self,
        batch: Vec<Job>,
        written: Vec<Result<Placed>>,
        indexed: &Result<()>,
        began: std::time::Instant,
    ) {
        let answered = std::time::Instant::now();
        let us = answered.duration_since(began).as_micros() as u64;
        #[cfg(test)]
        let us = us + 1000 * self.commit_extra_ms.swap(0, Ordering::Relaxed);
        for (job, placed) in batch.into_iter().zip(written) {
            if let (Ok((p, _)), Ok(())) = (&placed, indexed) {
                if let Some((first, _)) = p.first() {
                    self.frame_times
                        .note(answered, FrameTime { first: *first, us });
                }
            }
            let answer = match (placed, indexed) {
                (Err(e), _) => Err(e),
                (Ok((placed, _)), Ok(())) => Ok(placed
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| Some(*i) != job.hidden)
                    .map(|(_, (p, _))| p)
                    .collect()),
                // An index write that failed fails every frame it covered, as
                // a failed group sync failed each one it led.
                (Ok((placed, _)), Err(e)) => {
                    if placed
                        .last()
                        .is_some_and(|(p, _)| *p <= self.wal.last_position())
                    {
                        Err(RetainedFrame(format!("{e:#}")).into())
                    } else {
                        Err(anyhow::anyhow!("{e:#}"))
                    }
                }
            };
            self.queued.fetch_sub(1, Ordering::SeqCst);
            let _ = job.answer.send(answer);
        }
    }

    /// Place a group without retaining any placement a failed roll cut.
    pub(super) fn write_batch(
        &self,
        batch: &mut [Job],
    ) -> (Vec<Result<Placed>>, chain_writer::ChainState) {
        // Each frame, unsynced: a frame the log refuses fails alone. A frame's
        // ledger rows are chained first, on the head this batch's earlier
        // frames left; a frame that fails gives its links back, and the head
        // follows accepted placements; a later sync failure stops the store.
        let mut chained = self.chain.lock().unwrap().clone();
        let mut written: Vec<Result<Placed>> = Vec::with_capacity(batch.len());
        let mut heads = Vec::with_capacity(batch.len());
        let mut cut = false;
        for j in batch {
            if cut {
                written.push(Err(anyhow::anyhow!(
                    "batch stopped after a failed roll sync"
                )));
                continue;
            }
            let saved = chained.clone();
            heads.push(saved.clone());
            if let Err(e) = self.chain_job(&mut chained, j) {
                chained = saved;
                written.push(Err(e));
                continue;
            }
            let before = self.wal.last_position();
            let w = self
                .wal
                .write_timed(&j.records)
                .map_err(anyhow::Error::from);
            match &w {
                Ok((placed, _)) => Inner::chain_placed(&mut chained, j, placed),
                Err(_) => {
                    chained = saved;
                    let last = self.wal.last_position();
                    if last < before {
                        // A roll cut earlier unsynced jobs too. No position
                        // is reused in this group, and durable jobs stay put.
                        cut = true;
                        let mut restored = false;
                        for (i, earlier) in written.iter_mut().enumerate() {
                            if earlier
                                .as_ref()
                                .is_ok_and(|(p, _)| p.last().is_some_and(|(pos, _)| *pos > last))
                            {
                                if !restored {
                                    chained = heads[i].clone();
                                    restored = true;
                                }
                                *earlier = Err(anyhow::anyhow!("frame cut by a failed roll sync"));
                            }
                        }
                    }
                }
            }
            written.push(w);
        }
        (written, chained)
    }

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

    /// One sync for every frame of the batch, after its answers: the turn
    /// that queued a frame goes on -- its model request leaves -- while the
    /// disk works, and the frame is durable before the writer takes the next
    /// batch, so every later frame's sync covers this one too. What a crash
    /// in that window loses is lost, by the owner's call (theseus-7yhp).
    pub(super) fn sync_batch(&self, wrote: bool) -> Result<()> {
        #[cfg(test)]
        {
            let hold = self.sync_hold.lock().unwrap().take();
            if let Some(hold) = hold {
                let _ = hold.recv();
            }
        }
        if wrote && self.fsync {
            self.wal.sync().map_err(anyhow::Error::from)
        } else {
            Ok(())
        }
    }
}
