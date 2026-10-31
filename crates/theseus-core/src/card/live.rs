//! The live pass (theseus-6f8t0): the card proposes from what the owner said
//! in live conversations, kept as `card.obs.<id>` by the exchange-end
//! observer. Nightly at `[judge] learning_hour` (`tender.rs`), and after the
//! archive's run in `theseus card propose`; it runs for as long as `[card]
//! observe` is on, whether or not the card is done (done stops the archive
//! pass alone).
//!
//! The order of a pass: sweep the observer's unread sessions
//! ([`Core::sweep_observations`]); expire what nobody answered; read the open
//! observations; then propose, until `[card] live_per_night` live questions
//! have been made since this night's learning hour or `max_open` wait:
//!
//! 1. **Stale**, at most `stale_per_night` a night: an observation that
//!    contradicts a record still live, which has no open Stale question,
//!    asks "still true?" of that record (keep, edit, or end it). An
//!    observation that contradicts a live record is a Stale question's alone:
//!    it is never an Add or a Pattern.
//! 2. **Add**: one fact or correction (a correction counts at support 1), as
//!    its own words with no call; or two or more preferences and decisions
//!    that cluster, stated by the cheap model as the archive's clusters are.
//! 3. **Pattern**: preferences and decisions of at least `pattern_sessions`
//!    distinct conversations, in one cluster, stated by the same call. Never a
//!    behaviour: only what the owner said or chose.
//!
//! An observation a question rests on is `used` in the question's frame; one
//! whose words repeat a live record or any proposal's is set aside
//! (`dropped`), as the archive's statements are dropped; the rest stay open.
//! Nothing here keeps, ends or edits a record: only the owner's answer does.

use std::collections::HashSet;

use anyhow::Result;
use theseus_protocol::{
    card_options, card_question, CardAsk, CardObservation, CardProposal, CardProposeResult,
    CardSource, ObservationKind, ObservationState, ProposalState, StandingKind, StandingRecord,
    StandingScope, CARD_MAX_SOURCES, CARD_OPTIONS, CARD_STALE_OPTIONS,
};
use theseus_store::NewRecord;

use super::cluster::{self, Cluster};
use super::evidence::{self, Episode, LiveScan, Scan};
use super::run::{Job, DAY_MS, MAX_CALLS, RUNNING};
use super::state::{self, State};
use crate::fact::card::CardProposed;
use crate::judge::spend;
use crate::rpc::Core;
use crate::standing;

/// What a live job knows beyond the archive's: the observation behind each
/// episode of its scan (by index) and what each cluster asks.
pub(super) struct LiveMeta {
    pub obs: Vec<CardObservation>,
    pub asks: Vec<CardAsk>,
}

/// The month and day of `ms`, local, as `MM-DD`.
fn md(ms: u64) -> String {
    spend::local_day(ms)
        .get(5..)
        .unwrap_or_default()
        .to_string()
}

/// `[Keep] [Edit] [Not this]`: the options as the question ends with them.
fn bracketed(options: &[&str]) -> String {
    options
        .iter()
        .map(|o| format!("[{o}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A Pattern's question.
pub fn pattern_question(text: &str, conversations: u32, since_ms: u64) -> String {
    format!(
        "You've said or chosen this in {conversations} conversations since {}: '{text}'. Add it to your card? {}",
        md(since_ms),
        bracketed(&CARD_OPTIONS)
    )
}

/// A Stale question: what the card says, and what the owner said since.
pub fn stale_question(record: &StandingRecord, observed: &CardObservation) -> String {
    format!(
        "Your card says '{}' (kept {}). On {} you said '{}'. Still true? {}",
        record.text,
        md(record.since_ms),
        md(observed.at_ms),
        observed.text,
        bracketed(&CARD_STALE_OPTIONS)
    )
}

/// The sources of `obs`: each cited message node of each, newest first.
fn sources_of(obs: &[&CardObservation]) -> Vec<CardSource> {
    let mut v: Vec<&CardObservation> = obs.to_vec();
    v.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(a.id.cmp(&b.id)));
    v.iter()
        .flat_map(|o| {
            o.nodes.iter().map(|n| CardSource {
                episode: o.id.clone(),
                node: n.clone(),
                kind: "live".into(),
                at_ms: o.at_ms,
            })
        })
        .take(CARD_MAX_SOURCES)
        .collect()
}

/// A proposal of the live pass with what is the same for all three asks.
fn base(
    run: &str,
    now: u64,
    ask_days: u32,
    ask: CardAsk,
    obs: &[&CardObservation],
) -> CardProposal {
    CardProposal {
        id: crate::new_id("cdp"),
        kind: StandingKind::Preference,
        scope: StandingScope::Everywhere,
        text: String::new(),
        question: String::new(),
        options: card_options(),
        sources: sources_of(obs),
        support: obs.len() as u32,
        created_ms: now,
        expires_ms: now + u64::from(ask_days) * DAY_MS,
        state: ProposalState::Open,
        run: run.to_string(),
        answered_ms: None,
        answered_by: None,
        kept_text: None,
        standing_id: None,
        ask,
        record: None,
        observations: obs.iter().map(|o| o.id.clone()).collect(),
        posted_ms: None,
    }
}

/// The record `ask` makes of a lone fact or correction: its words as they
/// are, one source.
fn add_of(run: &str, now: u64, ask_days: u32, o: &CardObservation) -> CardProposal {
    let mut p = base(run, now, ask_days, CardAsk::Add, &[o]);
    p.kind = match o.kind {
        ObservationKind::Fact => StandingKind::Fact,
        _ => StandingKind::Preference,
    };
    p.text = o.text.clone();
    p.question = card_question(&p.text, &p.scope, 1);
    p
}

/// A Stale question about `record`, from `o`.
fn stale_of(
    run: &str,
    now: u64,
    ask_days: u32,
    record: &StandingRecord,
    o: &CardObservation,
) -> CardProposal {
    let mut p = base(run, now, ask_days, CardAsk::Stale, &[o]);
    p.kind = record.kind;
    p.scope = record.scope.clone();
    p.text = record.text.clone();
    p.options = CARD_STALE_OPTIONS.iter().map(|s| s.to_string()).collect();
    p.question = stale_question(record, o);
    p.record = Some(record.id.clone());
    p
}

impl LiveMeta {
    /// Make a statement's proposal (`run::proposal`'s) a live one: its ask,
    /// the observations of its cluster, their messages as its sources, and
    /// its words.
    pub(super) fn shape(&self, p: &mut CardProposal, cluster: usize, c: &Cluster, scan: &Scan) {
        let obs: Vec<&CardObservation> = c.episodes.iter().map(|&e| &self.obs[e]).collect();
        let ask = self.asks[cluster];
        p.ask = ask;
        p.observations = obs.iter().map(|o| o.id.clone()).collect();
        p.sources = sources_of(&obs);
        p.support = obs.len() as u32;
        if ask == CardAsk::Pattern {
            let sessions: HashSet<&str> = c
                .episodes
                .iter()
                .map(|&e| scan.episodes[e].session_id.as_str())
                .collect();
            let since = obs.iter().map(|o| o.at_ms).min().unwrap_or(p.created_ms);
            p.support = sessions.len() as u32;
            p.question = pattern_question(&p.text, p.support, since);
        }
    }

    /// The records that move the observations of `made` to `used`, with the
    /// proposal that used them.
    pub(super) fn used(&self, made: &[CardProposal], _scan: &Scan) -> Result<Vec<NewRecord>> {
        let mut out = Vec::new();
        for p in made {
            for id in &p.observations {
                if let Some(o) = self.obs.iter().find(|o| &o.id == id) {
                    out.push(state::obs_record(&moved(
                        o,
                        ObservationState::Used,
                        Some(&p.id),
                    ))?);
                }
            }
        }
        Ok(out)
    }
}

/// `o` in `to`, with the proposal that moved it.
pub(super) fn moved(
    o: &CardObservation,
    to: ObservationState,
    by: Option<&str>,
) -> CardObservation {
    CardObservation {
        state: to,
        proposal: by.map(str::to_string),
        ..o.clone()
    }
}

/// The latest learning hour at or before `now_ms`: where this night's count
/// of live questions starts (a `card propose` by day counts in its night).
pub fn night_of(now_ms: u64, learning_hour: u8) -> u64 {
    crate::learning::tender::due(now_ms, learning_hour, Some(now_ms)).0 - DAY_MS
}

/// Whether a proposal came from the live pass.
pub fn is_live(p: &CardProposal) -> bool {
    !p.ask.is_add() || !p.observations.is_empty()
}

impl Core {
    /// Read what the observer has not yet read of the owner's recent sessions,
    /// and write its observations (`observe::sweep`). How many sessions it read.
    async fn sweep_observations(&self, now_ms: u64) -> anyhow::Result<u32> {
        crate::card::observe::sweep(self, now_ms).await
    }

    /// The live pass (see the module's words). Its new proposals are added to
    /// `out.proposed` and returned, written in one frame a call; a dry run, a
    /// card with `[card] observe` off and a day line of 0 do nothing.
    pub(crate) async fn card_live_run(
        &self,
        dry_run: bool,
        out: &mut CardProposeResult,
    ) -> anyhow::Result<Vec<CardProposal>> {
        let cfg = &self.cfg.card;
        if dry_run || !cfg.observe || cfg.day_usd <= 0.0 {
            return Ok(Vec::new());
        }
        let Ok(_one) = RUNNING.try_lock() else {
            anyhow::bail!("a card run is already going");
        };
        let now = theseus_protocol::now_unix_ms();
        self.sweep_observations(now).await?;
        out.expired += self.card_expire(now).await? as u64;
        let scan = theseus_store::blocking(|| evidence::observations(&self.store))?;
        if scan.observations.is_empty() {
            return Ok(Vec::new());
        }
        let all = theseus_store::blocking(|| state::proposals(&self.store))?;
        let open = state::in_state(&all, ProposalState::Open).count();
        // This night's questions: those made since its learning hour, so the
        // nightly pass never sees the last night's (a rolling day did, by ms).
        let night = night_of(now, self.cfg.judge.learning_hour);
        let recent: Vec<&CardProposal> = all
            .iter()
            .filter(|p| is_live(p) && p.created_ms >= night)
            .collect();
        let stale_made = recent.iter().filter(|p| p.ask == CardAsk::Stale).count();
        let mut room = (cfg.live_per_night as usize)
            .saturating_sub(recent.len())
            .min((cfg.max_open as usize).saturating_sub(open));
        if room == 0 {
            return Ok(Vec::new());
        }
        let mut stale_room = (cfg.stale_per_night as usize).saturating_sub(stale_made);
        let board = self.runner.standing.all(&self.store)?;
        let records = standing::live(&board, None, now);
        let mut texts: Vec<String> = records.iter().map(|r| r.text.clone()).collect();
        texts.extend(all.iter().flat_map(|p| {
            let mut v = vec![p.text.clone()];
            v.extend(p.kept_text.clone());
            v
        }));
        let run = crate::new_id("crd");
        out.run.clone_from(&run);
        let mut made: Vec<CardProposal> = Vec::new();
        let mut moves: Vec<CardObservation> = Vec::new();
        let mut asked_about: HashSet<String> = all
            .iter()
            .filter(|p| p.state == ProposalState::Open && p.ask == CardAsk::Stale)
            .filter_map(|p| p.record.clone())
            .collect();
        // What is left for the Add and the Pattern: no observation of a live
        // record's contradiction (that is a Stale question's, or waits).
        let mut rest: Vec<usize> = Vec::new();
        for (i, o) in scan.observations.iter().enumerate() {
            let record = o
                .contradicts
                .as_ref()
                .and_then(|id| records.iter().find(|r| &r.id == id));
            let Some(record) = record else {
                rest.push(i);
                continue;
            };
            if stale_room > 0 && room > 0 && asked_about.insert(record.id.clone()) {
                let p = stale_of(&run, now, cfg.ask_days, record, o);
                moves.push(moved(o, ObservationState::Used, Some(&p.id)));
                made.push(p);
                stale_room -= 1;
                room -= 1;
            }
        }
        // A lone fact or correction, in its own words.
        let mut clustered: Vec<usize> = Vec::new();
        for &i in &rest {
            let o = &scan.observations[i];
            if !matches!(o.kind, ObservationKind::Fact | ObservationKind::Correction) {
                clustered.push(i);
            } else if texts.iter().any(|t| cluster::similar(t, &o.text)) {
                // It repeats a live record or a proposal: set aside.
                moves.push(moved(o, ObservationState::Dropped, None));
            } else if room > 0 {
                let p = add_of(&run, now, cfg.ask_days, o);
                texts.push(p.text.clone());
                moves.push(moved(o, ObservationState::Used, Some(&p.id)));
                made.push(p);
                room -= 1;
            }
        }
        let first = out.proposed.len();
        self.live_frame(&run, &made, &moves).await?;
        out.proposed.extend(made.iter().cloned());
        // What the model states: the preferences and decisions that cluster.
        let sub = LiveScan {
            episodes: clustered
                .iter()
                .map(|&i| scan.episodes[i].clone())
                .collect(),
            observations: clustered
                .iter()
                .map(|&i| scan.observations[i].clone())
                .collect(),
        };
        self.live_calls(&run, now, room, texts, sub, out).await?;
        // p2k-dm: after the frames above, a call here posts these (at most
        // `[card] dm_per_day`) to the owner's DM; this pass posts nothing.
        Ok(out.proposed[first..].to_vec())
    }

    /// The model's part: state the clusters of `sub` (preferences and
    /// decisions) that make an Add or a Pattern, one call and one frame a batch.
    async fn live_calls(
        &self,
        run: &str,
        now: u64,
        room: usize,
        texts: Vec<String>,
        sub: LiveScan,
        out: &mut CardProposeResult,
    ) -> Result<()> {
        let cfg = &self.cfg.card;
        let (clusters, asks, picked) = plan(&sub.episodes, cfg.pattern_sessions as usize);
        if room > 0 && !picked.is_empty() {
            let (target, provider) = self
                .card_model(picked.len() as u32)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            let scan = Scan {
                episodes: sub.episodes,
                ..Scan::default()
            };
            let mut job = Job {
                run: run.to_string(),
                now,
                today: spend::local_day(now),
                cfg,
                scan: &scan,
                clusters: &clusters,
                target,
                provider,
                st: theseus_store::blocking(|| State::read(&self.store))?,
                texts,
                room,
                earlier: Vec::new(),
                live: Some(LiveMeta {
                    obs: sub.observations,
                    asks,
                }),
            };
            out.spent_today_usd = job.st.spent_on(&job.today);
            let batches: Vec<Vec<usize>> = picked
                .chunks(super::ask::BATCH)
                .take(MAX_CALLS)
                .map(<[usize]>::to_vec)
                .collect();
            for (n, b) in batches.iter().enumerate() {
                if job.room == 0 {
                    out.stopped = Some(format!("full: {} questions are waiting", cfg.max_open));
                    break;
                }
                if !self.card_call(&mut job, n, b, out).await? {
                    break;
                }
            }
        }
        Ok(())
    }

    /// The pass's questions that made no call, and the observations they
    /// moved, in one frame with their row.
    async fn live_frame(
        &self,
        run: &str,
        made: &[CardProposal],
        moves: &[CardObservation],
    ) -> Result<()> {
        if made.is_empty() && moves.is_empty() {
            return Ok(());
        }
        let ids: Vec<String> = made.iter().map(|p| p.id.clone()).collect();
        let f = CardProposed {
            run,
            profile: self.cfg.card.profile.as_deref().unwrap_or_default(),
            model: "",
            cost_usd: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            clusters: 0,
            dropped: moves
                .iter()
                .filter(|o| o.state == ObservationState::Dropped)
                .count() as u32,
            proposals: &ids,
            failed: None,
        };
        let mut frame = Vec::new();
        for p in made {
            frame.push(state::record(p)?);
        }
        for o in moves {
            frame.push(state::obs_record(o)?);
        }
        frame.push(crate::fact::row(&f, None, None)?);
        self.people_write(&frame).await?;
        self.rec(None).announce(&f);
        Ok(())
    }
}

/// The clusters of `episodes`, what each asks, and which are worth a call, in
/// the order Add then Pattern: two or more observations make an Add, and
/// `pattern_sessions` distinct conversations a Pattern; a lone one waits.
fn plan(episodes: &[Episode], pattern_sessions: usize) -> (Vec<Cluster>, Vec<CardAsk>, Vec<usize>) {
    let clusters = cluster::cluster(episodes);
    let mut asks = Vec::new();
    let (mut adds, mut patterns) = (Vec::new(), Vec::new());
    for (i, c) in clusters.iter().enumerate() {
        let sessions: HashSet<&str> = c
            .episodes
            .iter()
            .map(|&e| episodes[e].session_id.as_str())
            .collect();
        if sessions.len() >= pattern_sessions {
            asks.push(CardAsk::Pattern);
            patterns.push(i);
        } else {
            asks.push(CardAsk::Add);
            if c.episodes.len() >= 2 {
                adds.push(i);
            }
        }
    }
    adds.extend(patterns);
    (clusters, asks, adds)
}
