//! The job (theseus-slvl.6): read the archive, cluster, ask the cheap model
//! to state each fresh cluster, and write the proposals. Off every turn's
//! path; the owner's `card.propose`, and the nightly tender's.
//!
//! The order of a run: expire what nobody answered; a full queue ends a real
//! run here, as an attempt that records itself and scans nothing (a dry run
//! still counts); scan (a read of the
//! imported records on the blocking pool); cluster and rank; drop the
//! clusters whose episodes were mostly seen already; then, in batches of
//! [`ask::BATCH`], reserve the call's worst case on `[card] day_usd` and the
//! day ceiling, call, parse, drop the statements that repeat a live standing
//! record or another proposal, and write the call's proposals, the episodes
//! it was sent, the spend and the `card.proposed` row in **one frame**. The
//! run stops when `[card] max_open` are waiting, the day's line would be
//! passed, or the clusters run out. A failed call writes its row and is not
//! remembered as sent, so the next run reads those clusters again.
//!
//! Two statements of one call that the model says conflict are not both
//! offered (`settle_conflicts`, theseus-ly75u): the one with more support
//! stays, and the run's result notes the other.

use theseus_kernel::micros_to_usd;
use theseus_protocol::{
    card_options, card_question, CardProposal, CardProposeResult, CardSource, ProposalState,
    CARD_MAX_SOURCES,
};

use super::ask;
use super::cluster::{self, Cluster};
use super::evidence::{self, Episode, Scan};
use super::state::{self, State};
use crate::fact::card::CardProposed;
use crate::judge::spend;
use crate::rpc::Core;
use crate::standing;

/// The most model calls one run makes.
pub const MAX_CALLS: usize = 8;
pub(super) const DAY_MS: u64 = 86_400_000;

/// One run at a time: the owner's and the nightly tender's.
pub(super) static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What one run holds while it goes: the evidence and its clusters, the
/// model, the day's state, and the words already taken.
pub(super) struct Job<'a> {
    pub(super) run: String,
    pub(super) now: u64,
    pub(super) today: String,
    pub(super) cfg: &'a crate::config::card::CardConfig,
    pub(super) scan: &'a Scan,
    pub(super) clusters: &'a [Cluster],
    pub(super) target: crate::turn::Target,
    pub(super) provider: std::sync::Arc<dyn crate::provider::Provider>,
    pub(super) st: State,
    /// Texts a statement must not repeat: live standing records and every
    /// proposal's words, kept, open or declined.
    pub(super) texts: Vec<String>,
    /// Proposals the queue still has room for.
    pub(super) room: usize,
    /// What this run's earlier calls made: each proposal's words and its
    /// cluster, listed in the later calls so a conflict with one is named.
    pub(super) earlier: Vec<(String, usize)>,
    /// The live pass's: what each cluster asks and the observations the
    /// scan's episodes are (`live.rs`); none for the archive's run.
    pub(super) live: Option<super::live::LiveMeta>,
}

impl Job<'_> {
    fn scrubbed(
        &self,
        core: &Core,
        picked: &[usize],
        earlier: &[String],
    ) -> (Vec<ask::Shown>, crate::provider::ProviderRequest) {
        let shown = ask::shown(picked, self.clusters);
        let scrubber = core.tools.scrubber.clone();
        let how = core.runner.catalog.forcing(&self.target.model);
        let request = ask::request(&self.target.model, how, &shown, earlier, &|t| {
            scrubber.scrub(t).0
        });
        (shown, request)
    }
}

impl Job<'_> {
    /// The words of the proposals this run's earlier calls made.
    fn earlier_texts(&self) -> Vec<String> {
        self.earlier.iter().map(|(t, _)| t.clone()).collect()
    }

    /// The proposals `parsed`'s statements make: one each, until the queue
    /// is full, and none that repeats a live record or any proposal's words
    /// (`dropped` counts those left out).
    fn statements(
        &mut self,
        parsed: &ask::Parsed,
        dropped: &mut u64,
        conflicts: &mut Vec<String>,
    ) -> Vec<CardProposal> {
        // One that conflicts with an entry of an earlier call is not offered:
        // that entry is already written.
        let mut fresh = Vec::new();
        for (i, s) in parsed.statements.iter().enumerate() {
            match parsed.earlier_clash.iter().find(|(k, _)| *k == i) {
                Some(&(_, p)) => conflicts.push(format!(
                    "kept '{}' over '{}': they conflict",
                    self.earlier[p].0, s.text
                )),
                None => fresh.push(s.clone()),
            }
        }
        let (kept, notes) = settle_conflicts(&fresh, self.clusters);
        conflicts.extend(notes);
        let mut made = Vec::new();
        for s in &kept {
            if made.len() >= self.room || self.texts.iter().any(|t| cluster::similar(t, &s.text)) {
                *dropped += 1;
                continue;
            }
            self.texts.push(s.text.clone());
            self.earlier.push((s.text.clone(), s.cluster));
            let mut p = proposal(
                &self.run,
                s,
                &self.clusters[s.cluster],
                self.scan,
                self.now,
                self.cfg.ask_days,
            );
            if let Some(live) = &self.live {
                live.shape(&mut p, s.cluster, &self.clusters[s.cluster], self.scan);
            }
            made.push(p);
        }
        self.room -= made.len();
        made
    }
}

/// An answer read: its model, usage, statements, and why it gave none.
fn read_answer(
    answered: &anyhow::Result<crate::provider::ModelResponse>,
    shown: &[ask::Shown],
    earlier: usize,
    asked: &str,
) -> (
    String,
    Option<theseus_protocol::Usage>,
    ask::Parsed,
    Option<String>,
) {
    match answered {
        // A cut answer's tool input is empty or partial: never a whole one.
        Ok(r) if crate::provider::hit_cap(r.stop_reason.as_deref()) => (
            r.model.clone(),
            Some(r.usage.clone()),
            ask::Parsed::default(),
            Some(crate::provider::cap_phrase(ask::MAX_TOKENS)),
        ),
        Ok(r) => match ask::parse(&r.content, shown, earlier) {
            Some(p) => (r.model.clone(), Some(r.usage.clone()), p, None),
            None => (
                r.model.clone(),
                Some(r.usage.clone()),
                ask::Parsed::default(),
                Some("the answer held no state_cards call".to_string()),
            ),
        },
        Err(e) => (
            asked.to_string(),
            None,
            ask::Parsed::default(),
            Some(format!("{e:#}")),
        ),
    }
}

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
        let mut texts: Vec<String> = {
            let board = self.runner.standing.all(&self.store)?;
            standing::live(&board, None, now)
                .iter()
                .map(|r| r.text.clone())
                .collect()
        };
        texts.extend(all.iter().flat_map(|p| {
            let mut v = vec![p.text.clone()];
            v.extend(p.kept_text.clone());
            v
        }));
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
        job.st.last_run_ms = Some(now);
        self.people_write(&[job.st.record()?]).await?;
        Ok(out)
    }

    /// The most the batches could cost: each call also lists, at most, a
    /// batch of entries per earlier call, each as long as an entry can be.
    fn dry_worst_case(&self, job: &Job<'_>, batches: &[Vec<usize>]) -> f64 {
        let mut usd = 0.0;
        for (n, b) in batches.iter().enumerate() {
            let list = vec!["x".repeat(ask::TEXT_CHARS); (n * ask::BATCH).min(job.room)];
            if let Some(m) = self.audit_reserve(&job.scrubbed(self, b, &list).1) {
                usd += micros_to_usd(m);
            }
        }
        usd
    }

    /// One batch of clusters: reserve, call, parse, filter, and write the
    /// call's frame. False when the run should stop (`out.stopped` says why).
    pub(super) async fn card_call(
        &self,
        job: &mut Job<'_>,
        n: usize,
        picked: &[usize],
        out: &mut CardProposeResult,
    ) -> anyhow::Result<bool> {
        let earlier = job.earlier_texts();
        let (shown, request) = job.scrubbed(self, picked, &earlier);
        let Some(need) = self.audit_reserve(&request) else {
            out.stopped = Some(format!("no price for {}", job.target.model));
            return Ok(false);
        };
        let left = (job.cfg.day_usd - job.st.spent_on(&job.today)).max(0.0);
        if micros_to_usd(need) > left {
            out.stopped = Some(format!(
                "budget: ${:.2} of [card] day_usd ${:.2} is left, under a call's worst case (${:.2})",
                left,
                job.cfg.day_usd,
                micros_to_usd(need)
            ));
            return Ok(false);
        }
        let hold = match self.runner.day_hold(need, "card") {
            Ok(h) => h,
            Err(why) => {
                out.stopped = Some(format!("budget: {why}"));
                return Ok(false);
            }
        };
        let started_ms = theseus_protocol::now_unix_ms();
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        let answered = job.provider.stream_message(&request, &mut quiet).await;
        let cost = answered.as_ref().map_or(need, |r| {
            self.runner.priced(r, &job.target.model, need, started_ms)
        });
        hold.settle(cost);
        let cost_usd = micros_to_usd(cost);
        job.st.spend(&job.today, cost_usd);
        if job.live.is_none() {
            job.st.last_run_ms = Some(job.now);
        }
        out.calls += 1;
        out.spent_usd += cost_usd;
        out.spent_today_usd = job.st.spent_on(&job.today);
        let (model, usage, parsed, failed) =
            read_answer(&answered, &shown, earlier.len(), &job.target.model);
        let mut dropped = u64::from(parsed.dropped);
        let made = job.statements(&parsed, &mut dropped, &mut out.conflicts);
        out.dropped += dropped;
        out.one_task += u64::from(parsed.one_task);
        let ids: Vec<String> = made.iter().map(|p| p.id.clone()).collect();
        let f = CardProposed {
            run: &job.run,
            profile: &job.target.profile,
            model: &model,
            cost_usd,
            input_tokens: usage.as_ref().map_or(0, |u| u.input_tokens),
            output_tokens: usage.as_ref().map_or(0, |u| u.output_tokens),
            clusters: shown.len() as u32,
            dropped: dropped as u32,
            proposals: &ids,
            failed: failed.as_deref(),
        };
        let mut frame = Vec::new();
        for p in &made {
            frame.push(state::record(p)?);
        }
        if let Some(live) = &job.live {
            // The observations a question rests on are used, in its frame.
            frame.extend(live.used(&made, job.scan)?);
        } else if failed.is_none() {
            // Every episode of every cluster sent is seen, shown or not.
            let mut sent: Vec<String> = picked
                .iter()
                .flat_map(|&c| {
                    job.clusters[c]
                        .episodes
                        .iter()
                        .map(|&e| job.scan.episodes[e].id.clone())
                })
                .collect();
            sent.sort();
            sent.dedup();
            frame.push(state::sent_record(&format!("{}_{n}", job.run), &sent)?);
        }
        frame.push(job.st.record()?);
        frame.push(crate::fact::row(&f, None, None)?);
        self.people_write(&frame).await?;
        self.rec(None).announce(&f);
        out.proposed.extend(made);
        if let Some(why) = failed {
            out.stopped = Some(format!("a call failed: {why}"));
            return Ok(false);
        }
        Ok(true)
    }

    /// The card's model and provider: `[card] profile` when it is set (a
    /// pin), else Jev's choice for a job over `clusters` clusters of the
    /// owner's activity (route.v10, theseus-ej6bj).
    pub(crate) async fn card_model(
        &self,
        clusters: u32,
    ) -> Result<
        (
            crate::turn::Target,
            std::sync::Arc<dyn crate::provider::Provider>,
        ),
        String,
    > {
        self.card_model_for(theseus_judge::builders::JobKind::Card { clusters })
            .await
    }

    /// [`Self::card_model`] for the job `job` (the observation call's is its
    /// own, over lines the owner wrote).
    pub(crate) async fn card_model_for(
        &self,
        job: theseus_judge::builders::JobKind,
    ) -> Result<
        (
            crate::turn::Target,
            std::sync::Arc<dyn crate::provider::Provider>,
        ),
        String,
    > {
        let (live, _) = self.live_profile();
        let site = crate::job_model::Site {
            name: "card",
            pin: self.cfg.card.profile.as_deref(),
            job,
            restricted: theseus_judge::builders::RestrictedModels::NotAllowed,
        };
        let (target, _) = self
            .runner
            .job_target(&live, site)
            .await
            .map_err(|e| format!("[card] profile {e}"))?;
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| format!("no provider {:?} for {}", target.provider, target.profile))?;
        Ok((target, provider))
    }
}

/// What a scan read and left out, on the run's result.
fn counted(out: &mut CardProposeResult, scan: &Scan) {
    out.evidence = scan.episodes.len() as u64;
    out.withheld = scan.withheld;
    out.setup = scan.setup;
    out.one_task = scan.one_task;
}

/// The statements of one call with no conflicting pair: of two that conflict
/// (either names the other's cluster), the one whose cluster has more
/// support stays, a tie to the better ranked; the notes say what was kept
/// over what. Pure.
pub fn settle_conflicts(
    statements: &[ask::Statement],
    clusters: &[Cluster],
) -> (Vec<ask::Statement>, Vec<String>) {
    let mut order: Vec<usize> = (0..statements.len()).collect();
    order.sort_by_key(|&i| {
        let c = statements[i].cluster;
        (std::cmp::Reverse(clusters[c].episodes.len()), c)
    });
    let clash = |a: &ask::Statement, b: &ask::Statement| {
        a.conflicts.contains(&b.cluster) || b.conflicts.contains(&a.cluster)
    };
    let mut keep = vec![false; statements.len()];
    let mut notes = Vec::new();
    for i in order {
        let s = &statements[i];
        match (0..statements.len()).find(|&k| keep[k] && clash(&statements[k], s)) {
            Some(k) => notes.push(format!(
                "kept '{}' over '{}': they conflict",
                statements[k].text, s.text
            )),
            None => keep[i] = true,
        }
    }
    let kept = statements
        .iter()
        .zip(keep)
        .filter(|&(_, k)| k)
        .map(|(s, _)| s.clone())
        .collect();
    (kept, notes)
}

/// A proposal for `s`, resting on all of its cluster's episodes (the newest
/// [`CARD_MAX_SOURCES`]).
pub(super) fn proposal(
    run: &str,
    s: &ask::Statement,
    c: &Cluster,
    scan: &Scan,
    now: u64,
    ask_days: u32,
) -> CardProposal {
    let mut eps: Vec<&Episode> = ask::cluster_episodes(c, &scan.episodes);
    eps.sort_by(|a, b| b.end_ms.cmp(&a.end_ms).then(a.id.cmp(&b.id)));
    let support = eps.len() as u32;
    let sources = eps
        .iter()
        .take(CARD_MAX_SOURCES)
        .map(|e| CardSource {
            episode: e.id.clone(),
            node: e.anchor.clone(),
            kind: e.kind.name().to_string(),
            at_ms: e.end_ms,
        })
        .collect();
    CardProposal {
        id: crate::new_id("cdp"),
        kind: s.kind,
        scope: s.scope.clone(),
        text: s.text.clone(),
        question: card_question(&s.text, &s.scope, support),
        options: card_options(),
        sources,
        support,
        created_ms: now,
        expires_ms: now + u64::from(ask_days) * DAY_MS,
        state: ProposalState::Open,
        run: run.to_string(),
        answered_ms: None,
        answered_by: None,
        kept_text: None,
        standing_id: None,
        ask: Default::default(),
        record: None,
        observations: Vec::new(),
        posted_ms: None,
    }
}
