//! The miner's entries on the core: the backlog night's (`mine_night`, which
//! reads, writes one frame, and hands the backlog its pattern causes) and the
//! owner's read-only run (`self_patterns`, which writes nothing, not even the
//! cursor). Neither calls a model.
//!
//! The night asks `[self.mint] mine` and [`crate::rsi::gate`] first, and reads
//! nothing with either off. It runs on the backlog's blocking-pool thread,
//! after serving, and waits for a quiet machine between pages
//! (`theseus_store::pressure`); on the start path there is nothing.

use anyhow::Result;
use theseus_protocol::patterns::{SelfPatternRow, SelfPatternsParams, SelfPatternsResult};
use theseus_protocol::rsi::SelfMode;

use super::read::{self, Env, SessionInfo, MAX_CALLS, SETTLE_MS};
use super::{bench, mine, Mined, Motif, Run, Vocab};
use crate::approval::Answerer;
use crate::fact::rsi_mine::{MineRan, Top};
use crate::rpc::Core;
use crate::rsi::{gate, Gate};

const DAY_MS: u64 = 86_400_000;
/// Motifs the ledger row names.
const TOP: usize = 3;
/// A page wait for a busy machine, at most.
const QUIET: std::time::Duration = std::time::Duration::from_secs(2);

/// What a night did.
pub struct Night {
    pub mined: Mined,
}

impl Core {
    /// The place class of a session, as a word.
    fn mine_place(&self, session: &crate::session::SessionRecord) -> String {
        if session.task.is_some() {
            return "task".into();
        }
        match self.outbox.try_target(&session.session_id) {
            Ok(Some(t)) if t.contains("dm") => "dm".into(),
            Ok(Some(_)) => "channel".into(),
            _ => "cli".into(),
        }
    }

    /// What the miner reads of a session: none for one it leaves alone (an
    /// imported one, one that speaks in a shared place, a self step's).
    fn mine_session(&self, id: &str) -> Option<SessionInfo> {
        if crate::import::is_imported(id) || !self.runner.speaks_privately(id) {
            return None;
        }
        let rec = self
            .store
            .get_session::<crate::session::SessionRecord>(id)
            .ok()??;
        if let Some(e) = rec
            .execution_id
            .as_deref()
            .and_then(|e| self.kernel.execution(e).ok()?)
        {
            if e.authority.principal == crate::rsi::SELF_PRINCIPAL {
                return None;
            }
        }
        Some(SessionInfo {
            dir: rec.dir.clone(),
            place: self.mine_place(&rec),
        })
    }

    fn mine_bench(&self, since_ms: u64, env: &Env<'_>, vocab: &mut Vocab) -> Vec<Run> {
        self.cfg
            .self_improve
            .mint
            .bench_dirs
            .iter()
            .flat_map(|d| bench::read_dir(&crate::config::expand(d), since_ms, env, vocab))
            .collect()
    }

    /// Fold what the cursor has not seen into the window, mine it, and, when
    /// `write`, keep it: one frame with the cursor, the days that changed, the
    /// vocabulary and the night's row.
    pub(crate) fn mine_run(&self, now_ms: u64, write: bool) -> Result<Night> {
        let p = self.cfg.self_improve.mint.params();
        let mut w = read::load(&self.store, now_ms, p.window_days)?;
        let fresh_key = w.cursor.key.is_empty();
        if fresh_key {
            w.cursor.key = hex::encode(crate::new_id("mk").as_bytes());
        }
        let clean = |v: &str| self.tools.scrubber.scrub(v).1 == 0;
        let session = |id: &str| self.mine_session(id);
        let env = Env {
            key: w.cursor.key.as_bytes().to_vec(),
            clean: &clean,
            session: &session,
        };
        let since = now_ms.saturating_sub(u64::from(p.window_days) * DAY_MS);
        // Without a cursor the read starts at the window's first day.
        let from = w.cursor.pos;
        let folded = if from == 0 {
            read::fold(
                &self.store,
                &env,
                0,
                Some(since),
                now_ms.saturating_sub(SETTLE_MS),
                MAX_CALLS,
            )?
        } else {
            if write {
                theseus_store::pressure::quiet_blocking(QUIET);
            }
            read::fold(
                &self.store,
                &env,
                from,
                None,
                now_ms.saturating_sub(SETTLE_MS),
                MAX_CALLS,
            )?
        };
        let calls_read = folded.calls;
        w.cursor.pos = folded.cursor.max(from);
        w.cursor.ran_ms = now_ms;
        let changed = w.add(folded);
        let mut vocab = w.vocab.clone();
        let mut runs = w.runs();
        runs.extend(self.mine_bench(since, &env, &mut vocab));
        let mined = mine(&runs, &vocab, &p, now_ms);
        if write {
            let expired = read::date_of(now_ms.saturating_sub(u64::from(p.window_days) * DAY_MS));
            let had = self
                .store
                .get_meta::<read::Day>(&read::day_key(&expired))?
                .is_some_and(|d| !d.runs.is_empty());
            let mut frame = w.records(&changed, had.then_some(expired))?;
            let fact = MineRan {
                window_days: p.window_days,
                runs: mined.runs as u32,
                calls_read: calls_read as u32,
                motifs: mined.motifs.len() as u32,
                top: mined
                    .motifs
                    .iter()
                    .take(TOP)
                    .map(|m| Top {
                        digest: m.cause_key(),
                        support: m.support.occurrences,
                        saving_usd_per_day: m.saving.usd_per_day,
                    })
                    .collect(),
            };
            frame.push(crate::fact::row(&fact, None, None)?);
            self.store.append(&frame)?;
            self.rec(None).announce(&fact);
        }
        Ok(Night { mined })
    }

    /// The backlog night's pattern causes: nothing unless `[self.mint] mine`
    /// is on and the gate allows. A failed read is logged and gives none: it
    /// never fails the fixes' night.
    pub fn mine_obs(&self, now_ms: u64, date: &str) -> Vec<crate::rsi::backlog::Obs> {
        if !self.cfg.self_improve.mint.mine
            || self.cfg.self_improve.mode != SelfMode::Act
            || gate(&self.self_ctx()) != Gate::Allowed
        {
            return vec![];
        }
        match self.mine_run(now_ms, true) {
            Ok(n) => n
                .mined
                .motifs
                .iter()
                .map(|m| crate::rsi::backlog::of_motif(m, now_ms, date))
                .collect(),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "self mine: the night did not run");
                vec![]
            }
        }
    }

    /// `self.patterns`: the owner's read. It works with the mode off and
    /// writes nothing.
    pub fn self_patterns(
        &self,
        p: &SelfPatternsParams,
        who: &Answerer,
    ) -> Result<SelfPatternsResult> {
        self.self_patterns_at(p, who, theseus_protocol::now_unix_ms())
    }

    /// `self_patterns` at a clock a test sets.
    pub fn self_patterns_at(
        &self,
        p: &SelfPatternsParams,
        who: &Answerer,
        now: u64,
    ) -> Result<SelfPatternsResult> {
        let from_job = p
            .from_job
            .as_deref()
            .map(str::trim)
            .filter(|j| !j.is_empty());
        self.judge_act(
            who,
            crate::rpc::Act::SelfBacklog {
                method: theseus_protocol::method::SELF_PATTERNS,
                what: "",
                from_job,
            },
        )?;
        let n = self.mine_run(now, false)?;
        let top = p.top.unwrap_or(10) as usize;
        let window = self.cfg.self_improve.mint.window_days;
        Ok(SelfPatternsResult {
            window_days: window,
            runs: n.mined.runs as u32,
            calls: n.mined.calls as u32,
            found: n.mined.motifs.len() as u32,
            rows: n
                .mined
                .motifs
                .iter()
                .take(top)
                .map(|m| row_of(m, now, window))
                .collect(),
        })
    }
}

/// A motif as the wire's row: shapes and numbers.
pub fn row_of(m: &Motif, now_ms: u64, window_days: u32) -> SelfPatternRow {
    let mut arms = Vec::new();
    for b in &m.branches {
        for a in &b.arms {
            let mut line = format!("{} → {}", b.at, a.on);
            for t in &a.then {
                line.push_str(" → ");
                line.push_str(t);
            }
            if !matches!(a.exit, super::Exit::Ok) {
                line.push_str(&format!(" → {}", a.exit.word()));
            }
            arms.push(format!("{line} ({}×)", a.count));
        }
    }
    SelfPatternRow {
        digest: m.digest.clone(),
        scope: m.scope.clone(),
        bench_born: m.bench_born,
        places: m.places.clone(),
        occurrences: m.support.occurrences,
        sessions: m.support.sessions,
        days: m.support.days,
        steps: m
            .steps
            .iter()
            .map(|s| format!("{}{}", if s.core { "" } else { "? " }, s.shape.text))
            .collect(),
        arms,
        inputs: m
            .inputs
            .iter()
            .map(|i| format!("{}: {:?}", i.name, i.ty))
            .collect(),
        tokens_per_occ: m.cost.tokens.total(),
        usd_per_occ: m.cost.usd,
        secs_per_occ: m.cost.model_secs + m.cost.tool_secs,
        decisions_per_occ: m.cost.decisions,
        saving_usd_per_day: m.saving.usd_per_day,
        saving_secs_per_day: m.saving.secs_per_day,
        score: m.score(now_ms, window_days),
    }
}
