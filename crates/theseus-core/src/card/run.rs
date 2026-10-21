//! The job (theseus-slvl.6): read the archive, cluster, ask the cheap model
//! to state each fresh cluster, and write the proposals. Off every turn's
//! path; the owner's `card.propose`, and the nightly tender's.
//!
//! The order of a run: expire what nobody answered; scan (a read of the
//! imported records on the blocking pool); cluster and rank; drop the
//! clusters whose episodes were mostly seen already; then, in batches of
//! [`ask::BATCH`], reserve the call's worst case on `[card] day_usd` and the
//! day ceiling, call, parse, drop the statements that repeat a live standing
//! record or another proposal, and write the call's proposals, the episodes
//! it was sent, the spend and the `card.proposed` row in **one frame**. The
//! run stops when `[card] max_open` are waiting, the day's line would be
//! passed, or the clusters run out. A failed call writes its row and is not
//! remembered as sent, so the next run reads those clusters again.

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
const DAY_MS: u64 = 86_400_000;

/// One run at a time: the owner's and the nightly tender's.
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// What one run holds while it goes: the evidence and its clusters, the
/// model, the day's state, and the words already taken.
struct Job<'a> {
    run: String,
    now: u64,
    today: String,
    cfg: &'a crate::config::card::CardConfig,
    scan: &'a Scan,
    clusters: &'a [Cluster],
    target: crate::turn::Target,
    provider: std::sync::Arc<dyn crate::provider::Provider>,
    st: State,
    /// Texts a statement must not repeat: live standing records and every
    /// proposal's words, kept, open or declined.
    texts: Vec<String>,
    /// Proposals the queue still has room for.
    room: usize,
}

impl Job<'_> {
    fn scrubbed(
        &self,
        core: &Core,
        picked: &[usize],
    ) -> (Vec<ask::Shown>, crate::provider::ProviderRequest) {
        let shown = ask::shown(picked, self.clusters);
        let scrubber = core.tools.scrubber.clone();
        let request = ask::request(&self.target.model, &shown, &|t| scrubber.scrub(t).0);
        (shown, request)
    }
}

impl Job<'_> {
    /// The proposals `parsed`'s statements make: one each, until the queue
    /// is full, and none that repeats a live record or any proposal's words
    /// (`dropped` counts those left out).
    fn statements(&mut self, parsed: &ask::Parsed, dropped: &mut u64) -> Vec<CardProposal> {
        let mut made = Vec::new();
        for s in &parsed.statements {
            if made.len() >= self.room || self.texts.iter().any(|t| cluster::similar(t, &s.text)) {
                *dropped += 1;
                continue;
            }
            self.texts.push(s.text.clone());
            made.push(proposal(
                &self.run,
                s,
                &self.clusters[s.cluster],
                self.scan,
                self.now,
                self.cfg.ask_days,
            ));
        }
        self.room -= made.len();
        made
    }
}

/// An answer read: its model, usage, statements, and why it gave none.
fn read_answer(
    answered: &anyhow::Result<crate::provider::ModelResponse>,
    shown: &[ask::Shown],
    asked: &str,
) -> (
    String,
    Option<theseus_protocol::Usage>,
    ask::Parsed,
    Option<String>,
) {
    match answered {
        Ok(r) => match ask::parse(&r.content, shown) {
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
        let st = theseus_store::blocking(|| State::read(&self.store))?;
        out.spent_today_usd = st.spent_on(&today);
        if cfg.day_usd <= 0.0 {
            out.stopped = Some("off: [card] day_usd is 0".into());
            return Ok(out);
        }
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        let open = state::in_state(&all, ProposalState::Open).count();
        let seen = theseus_store::blocking(|| state::seen(&self.store, &all))?;
        let scan = theseus_store::blocking(|| evidence::scan(&self.store))?;
        let clusters = cluster::cluster(&scan.episodes);
        let fresh: Vec<usize> = (0..clusters.len())
            .filter(|&i| cluster::fresh(&clusters[i], &scan.episodes, &seen))
            .collect();
        out.evidence = scan.episodes.len() as u64;
        out.withheld = scan.withheld;
        out.clusters = clusters.len() as u64;
        out.fresh = fresh.len() as u64;
        if open >= cfg.max_open as usize {
            out.stopped = Some(format!("full: {open} proposals are waiting"));
            return Ok(out);
        }
        let (target, provider) = self.card_model().map_err(|e| anyhow::anyhow!(e))?;
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
        };
        out.run = job.run.clone();
        let batches: Vec<Vec<usize>> = fresh
            .chunks(ask::BATCH)
            .take(MAX_CALLS)
            .map(<[usize]>::to_vec)
            .collect();
        if dry_run {
            for b in &batches {
                if let Some(m) = self.audit_reserve(&job.scrubbed(self, b).1) {
                    out.at_most_usd += micros_to_usd(m);
                }
            }
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

    /// One batch of clusters: reserve, call, parse, filter, and write the
    /// call's frame. False when the run should stop (`out.stopped` says why).
    async fn card_call(
        &self,
        job: &mut Job<'_>,
        n: usize,
        picked: &[usize],
        out: &mut CardProposeResult,
    ) -> anyhow::Result<bool> {
        let (shown, request) = job.scrubbed(self, picked);
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
        let mut quiet = |_: crate::provider::Delta<'_>| {};
        // The request holds every shown cluster's episodes: it is private
        // when any episode's session is.
        let sessions = shown.iter().flat_map(|s| {
            s.episodes
                .iter()
                .map(|(_, i)| job.scan.episodes[*i].session_id.as_str())
        });
        let provider = crate::google::private::over_sessions(
            job.provider.clone(),
            &self.store,
            &self.cfg,
            sessions,
        );
        let answered = provider.stream_message(&request, &mut quiet).await;
        let cost = answered
            .as_ref()
            .map_or(need, |r| self.runner.priced(r, &job.target.model, need));
        hold.settle(cost);
        let cost_usd = micros_to_usd(cost);
        job.st.spend(&job.today, cost_usd);
        job.st.last_run_ms = Some(job.now);
        out.calls += 1;
        out.spent_usd += cost_usd;
        out.spent_today_usd = job.st.spent_on(&job.today);
        let (model, usage, parsed, failed) = read_answer(&answered, &shown, &job.target.model);
        let mut dropped = u64::from(parsed.dropped);
        let made = job.statements(&parsed, &mut dropped);
        out.dropped += dropped;
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
        if failed.is_none() {
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

    /// `[card] profile`'s target and provider.
    fn card_model(
        &self,
    ) -> Result<
        (
            crate::turn::Target,
            std::sync::Arc<dyn crate::provider::Provider>,
        ),
        String,
    > {
        let profile = self.cfg.card.profile.clone();
        let (live, _) = self.live_profile();
        let target = self
            .runner
            .resolve_target(&live, Some(&profile), None, None)
            .map_err(|e| format!("[card] profile {profile:?} does not resolve: {e:#}"))?;
        let provider = self
            .runner
            .providers
            .get(&target.provider)
            .cloned()
            .ok_or_else(|| format!("no provider {:?} for {profile}", target.provider))?;
        Ok((target, provider))
    }
}

/// A proposal for `s`, resting on all of its cluster's episodes (the newest
/// [`CARD_MAX_SOURCES`]).
fn proposal(
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
    }
}
