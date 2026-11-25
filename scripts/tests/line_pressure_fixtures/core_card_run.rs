
impl Core {
    /// One run: also the nightly tender's, which is the daemon's own act.
    pub(crate) async fn card_run(&self, dry_run: bool) -> anyhow::Result<CardProposeResult> {
        let Ok(_one) = RUNNING.try_lock() else {
            anyhow::bail!("a card run is already going");
        };
        let cfg = &self.cfg.card;
        let now = theseus_protocol::now_unix_ms();
        let today = spend::local_day(now);
        let mut out = CardProposeResult {
            day_usd: cfg.day_usd,
            ..CardProposeResult::default()
        };
        if !dry_run {
            out.expired = self.card_expire(now).await? as u64;
        }
        let mut st = theseus_store::blocking(|| State::read(&self.store))?;
        out.spent_today_usd = st.spent_on(&today);
        if cfg.day_usd <= 0.0 {
            out.stopped = Some("off: [card] day_usd is 0".into());
            return Ok(out);
        }
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        let open = state::in_state(&all, ProposalState::Open).count();
        let full = open >= cfg.max_open as usize;
        if full && !dry_run {
            // A full run is an attempt: it records itself and scans nothing.
            out.stopped = Some(format!("full: {open} proposals are waiting"));
            st.last_run_ms = Some(now);
            self.people_write(&[st.record()?]).await?;
            return Ok(out);
        }
        let seen = theseus_store::blocking(|| state::seen(&self.store, &all))?;
        let scan = theseus_store::blocking(|| evidence::scan(&self.store))?;
        let clusters = cluster::cluster(&scan.episodes);
        let fresh: Vec<usize> = (0..clusters.len())
            .filter(|&i| cluster::fresh(&clusters[i], &scan.episodes, &seen))
            .collect();
        counted(&mut out, &scan);
        out.clusters = clusters.len() as u64;
        out.fresh = fresh.len() as u64;
        if full {
            out.stopped = Some(format!("full: {open} proposals are waiting"));
            return Ok(out);
        }
        let (target, provider) = self
            .card_model(fresh.len() as u32)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let texts = self.known_texts(&all, now)?;
        let mut job = Job {
            run: if dry_run {
                String::new()
            } else {
                crate::new_id("crd")
            },
            now,
            today,
            cfg,
            scan: &scan,
            clusters: &clusters,
            target,
            provider,
            st,
            texts,
            room: cfg.max_open as usize - open,
            earlier: Vec::new(),
            live: None,
        };
        out.run = job.run.clone();
        let batches: Vec<Vec<usize>> = fresh
            .chunks(ask::BATCH)
            .take(MAX_CALLS)
            .map(<[usize]>::to_vec)
            .collect();
        if dry_run {
            out.at_most_usd = self.dry_worst_case(&job, &batches);
            return Ok(out);
        }
        for (n, picked) in batches.iter().enumerate() {
            if job.room == 0 {
                out.stopped = Some(format!("full: {} proposals are waiting", cfg.max_open));
                break;
            }
            if !self.card_call(&mut job, n, picked, &mut out).await? {
                break;
            }
        }
        // A failed call is not a run the interval counts from, as far as the run
        // got: the mark is what the run's own calls left it (`card_call` moves it
        // on each one that succeeds). A run in which none did keeps the stored
        // mark, so its next attempt is the next night's, not the interval's; one
        // that succeeded first counts from its start and waits the interval. A
        // stop of any other kind that gets here (no price, a budget refusal, a
        // full board) is a run, and counts (theseus-kinfs; the tender's own mark
        // keeps a failed run from being due again before then).
        job.st.last_run_ms = run_mark(&out, job.st.last_run_ms, now);
        self.people_write(&[job.st.record()?]).await?;
        Ok(out)
    }
}
