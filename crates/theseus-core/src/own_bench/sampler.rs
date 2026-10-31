//! The nightly sampler (own-benchmark): a few of the owner's recent turns
//! per (model, effort) cell, replayed one rung down the routing table and
//! judged by `compare.v1`, each a `route.replay.judged` row. Paced by
//! pressure (the machine quiet before each replay), and under
//! `[route_bench] replay_day_usd`: the day's spend is read back from the rows, and
//! a replay whose reservation would pass what is left is not made (the check
//! is before the call, never after). Never a shared place's turn, and never
//! one whose replay would send a sealed thread's, a veiled recall's or an
//! attachment's bytes to a provider other than the original's
//! (`Core::crossing`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use theseus_protocol::own_bench::TurnReplayParams;
use theseus_protocol::LedgerKind;
use tokio::time::Instant;

use super::rebuild::NotRebuildable;
use super::replay::{CapReached, Crossing};
use super::{rung, BY_SAMPLER};
use crate::fact::own_bench::RouteReplayJudged;
use crate::learning::tender::{due, tend};
use crate::learning::{local_midnight, rebuild::TurnEnd};
use crate::rpc::Core;

/// Turns a cell is replayed for in a night.
pub const PER_CELL: usize = 3;
/// How far back turns are sampled from.
pub const WINDOW_DAYS: u64 = 7;
/// The sampler runs this many hours after the learning run's hour.
pub const HOURS_AFTER_LEARNING: u8 = 2;
/// The META key of the last nightly run (`{"at_unix_ms"}`).
pub const LAST_RUN: &str = "route.replay.last";

const DAY_MS: u64 = 86_400_000;

/// A turn the sampler may replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub session: String,
    pub turn: String,
    pub model: String,
    /// The effort it ran at, empty: the profile's own.
    pub effort: String,
    pub at_ms: u64,
    pub cost_usd: f64,
}

/// The cell a candidate belongs to.
pub type Cell = (String, String);

/// At most `per_cell` turns a cell, newest first, leaving out any turn in
/// `done`. Pure.
pub fn pick(
    mut all: Vec<Candidate>,
    done: &HashSet<String>,
    per_cell: usize,
) -> BTreeMap<Cell, Vec<Candidate>> {
    all.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then_with(|| a.turn.cmp(&b.turn)));
    let mut out: BTreeMap<Cell, Vec<Candidate>> = BTreeMap::new();
    for c in all {
        if done.contains(&c.turn) {
            continue;
        }
        let cell = out.entry((c.model.clone(), c.effort.clone())).or_default();
        if cell.len() < per_cell {
            cell.push(c);
        }
    }
    out
}

/// What a night's pass did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sampled {
    /// Turns replayed, and judged.
    pub replayed: u64,
    pub judged: u64,
    /// Turns left out, by why.
    pub skipped: BTreeMap<String, u64>,
    pub spent_usd: f64,
    pub cap_usd: f64,
    /// Why it ended before its cells did.
    pub stopped: Option<String>,
}

impl Core {
    /// Each turn that ended since `since_ms`: its model, and the effort it ran
    /// at (the `route.decided` row's `effort_ran`, else none).
    pub(crate) fn turns_since(&self, since_ms: u64) -> anyhow::Result<Vec<Candidate>> {
        let ends = self.turn_ends(None, since_ms)?;
        let mut effort: BTreeMap<String, String> = BTreeMap::new();
        for r in self.rows_since(LedgerKind::RouteDecided, since_ms)? {
            if let (Some(t), Some(e)) = (r.turn_id.clone(), r.data["effort_ran"].as_str()) {
                effort.insert(t, e.to_string());
            }
        }
        Ok(ends
            .into_iter()
            .filter(|e: &TurnEnd| !e.model.is_empty())
            .map(|e| Candidate {
                effort: effort.get(&e.turn_id).cloned().unwrap_or_default(),
                session: e.session_id,
                turn: e.turn_id,
                model: e.model,
                at_ms: e.at_ms,
                cost_usd: e.cost_usd.unwrap_or(0.0),
            })
            .collect())
    }

    /// The rung table of this daemon's config and catalog.
    pub(crate) fn rungs(&self) -> Vec<rung::Rung> {
        let names = rung::profiles_of(self.cfg.all_profiles());
        rung::ladder(&names, |p| {
            let prof = self.cfg.profile(p).ok()?;
            self.runner
                .resolve_target(&self.live_profile().0, Some(p), None, None)
                .ok()?;
            let e = self.catalog.get(&prof.model)?;
            Some((prof.model.clone(), (e.input_per_mtok, e.output_per_mtok)))
        })
    }

    /// What the sampler has spent today, from its rows: replays and
    /// judgments, failed calls included.
    pub(crate) fn replay_spent_today(&self, now_ms: u64) -> anyhow::Result<f64> {
        Ok(self
            .rows_since(LedgerKind::RouteReplayJudged, local_midnight(now_ms))?
            .iter()
            .map(|r| {
                r.data["replay_usd"].as_f64().unwrap_or(0.0)
                    + r.data["judge_usd"].as_f64().unwrap_or(0.0)
            })
            .sum::<f64>()
            + 0.0)
    }

    /// One pass of the sampler (the nightly run's, and a test's).
    pub async fn replay_sample(self: &Arc<Self>, now_ms: u64) -> anyhow::Result<Sampled> {
        let cap = theseus_kernel::usd_to_micros(self.cfg.route_bench.replay_day_usd);
        let mut out = Sampled {
            cap_usd: self.cfg.route_bench.replay_day_usd,
            ..Sampled::default()
        };
        if cap == 0 {
            out.stopped = Some("[route_bench] replay_day_usd = 0: the sampler is off".into());
            return Ok(out);
        }
        let table = self.rungs();
        if table.len() < 2 {
            out.stopped = Some("the routing table holds no second model to replay down to".into());
            return Ok(out);
        }
        let since = now_ms.saturating_sub(WINDOW_DAYS * DAY_MS);
        let judged: HashSet<String> = self
            .rows_since(LedgerKind::RouteReplayJudged, since)?
            .into_iter()
            .filter_map(|r| r.data["turn"].as_str().map(str::to_string))
            .collect();
        let plan = pick(self.turns_since(since)?, &judged, PER_CELL);
        let mut spent = theseus_kernel::usd_to_micros(self.replay_spent_today(now_ms)?);
        let skip =
            |out: &mut Sampled, why: &str| *out.skipped.entry(why.to_string()).or_default() += 1;
        'cells: for ((model, effort), turns) in plan {
            let Some(price) = self
                .catalog
                .get(&model)
                .map(|e| (e.input_per_mtok, e.output_per_mtok))
            else {
                skip(&mut out, "no_price");
                continue;
            };
            let Some(to) = rung::down(&table, price).cloned() else {
                skip(&mut out, "no_rung_down");
                continue;
            };
            for c in turns {
                if self.runner.class_of(&c.session) == crate::places::PlaceClass::Shared {
                    skip(&mut out, "shared_place");
                    continue;
                }
                if spent >= cap {
                    out.stopped = Some(format!(
                        "[route_bench] replay_day_usd (${:.2}) is spent for the day",
                        out.cap_usd
                    ));
                    break 'cells;
                }
                self.quiet_before_replay().await;
                let cell = (model.as_str(), effort.as_str());
                if self
                    .sample_one(&c, cell, &to, cap - spent, (&mut spent, &mut out))
                    .await
                {
                    break 'cells;
                }
            }
        }
        out.spent_usd = theseus_kernel::micros_to_usd(spent);
        Ok(out)
    }

    /// One turn replayed and recorded. Returns whether the pass stops (the
    /// cap would not pay for the call).
    async fn sample_one(
        self: &Arc<Self>,
        c: &Candidate,
        (model, effort): (&str, &str),
        to: &rung::Rung,
        left: u64,
        (spent, out): (&mut u64, &mut Sampled),
    ) -> bool {
        let p = TurnReplayParams {
            session: c.session.clone(),
            turn: c.turn.clone(),
            profile: to.profile.clone(),
            effort: (!effort.is_empty()).then(|| effort.to_string()),
            scope: None,
            compare: Some(true),
        };
        let mut failed_spend = 0;
        let row = |judged: Option<&theseus_protocol::own_bench::TurnReplayResult>,
                   why: Option<&str>,
                   failed: u64| {
            let v = judged.and_then(|r| r.verdict.as_ref());
            self.rec(Some(&c.session))
                .in_turn(Some(&c.turn))
                .record(&RouteReplayJudged {
                    session: &c.session,
                    turn: &c.turn,
                    from_model: model,
                    from_effort: effort,
                    to_profile: &to.profile,
                    to_model: judged.map_or(to.model.as_str(), |r| r.replay.model.as_str()),
                    to_effort: judged.and_then(|r| r.effort.as_deref()).unwrap_or(effort),
                    verdict: v.map(|v| v.verdict.as_str()),
                    p: v.map(|v| v.p),
                    same_or_better: v.map(|v| v.same_or_better),
                    judgment: v.map(|v| v.judgment.as_str()),
                    unjudged: judged.and_then(|r| r.unjudged.as_deref()).or(why),
                    orig_usd: judged.map_or(0.0, |r| r.original.cost_usd),
                    replay_usd: judged.map_or_else(
                        || theseus_kernel::micros_to_usd(failed),
                        |r| r.replay.cost_usd,
                    ),
                    judge_usd: judged.map_or(0.0, |r| (r.cost_usd - r.replay.cost_usd).max(0.0)),
                });
        };
        match self
            .replay_turn(&p, BY_SAMPLER, Some(left), &mut failed_spend)
            .await
        {
            Ok(done) => {
                let r = &done.result;
                *spent += theseus_kernel::usd_to_micros(r.cost_usd);
                out.replayed += 1;
                out.judged += u64::from(r.verdict.is_some());
                row(Some(r), None, 0);
                false
            }
            Err(e) => {
                if let Some(cap) = e.downcast_ref::<CapReached>() {
                    out.stopped = Some(cap.to_string());
                    return true;
                }
                let why = if let Some(n) = e.downcast_ref::<NotRebuildable>() {
                    format!("not_rebuildable:{}", n.part)
                } else if e.downcast_ref::<Crossing>().is_some() {
                    "crossing".to_string()
                } else {
                    tracing::warn!(turn = %c.turn, error = %format!("{e:#}"), "a sampled replay failed");
                    "failed".to_string()
                };
                // A call that was made and failed is spent.
                if failed_spend > 0 {
                    *spent += failed_spend;
                    row(None, Some(&why), failed_spend);
                }
                *out.skipped.entry(why).or_default() += 1;
                false
            }
        }
    }

    /// Wait until the machine is quiet (the pressure bound), never past a stop.
    async fn quiet_before_replay(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        let _ = tokio::task::spawn_blocking(move || {
            theseus_store::pressure::quiet_blocking_unless(theseus_store::pressure::BOUND, || {
                me.upgrade().is_none_or(|c| c.outbox.stopping())
            })
        })
        .await;
    }

    /// The nightly tender, after serving: `HOURS_AFTER_LEARNING` after the
    /// learning hour, a missed night once, never within 10 minutes of a
    /// start. Nothing with the cap at 0.
    pub fn route_replay_after_serving(self: &Arc<Self>) {
        if self.cfg.route_bench.replay_day_usd <= 0.0 {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            let ran = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let (read, ran_at) = (core.clone(), ran.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let stored = theseus_store::blocking(|| c.store.get_meta::<Value>(LAST_RUN))
                    .ok()
                    .flatten()
                    .and_then(|v| v["at_unix_ms"].as_u64());
                let here = ran_at.load(std::sync::atomic::Ordering::Relaxed);
                let last = stored.max((here > 0).then_some(here));
                let now = theseus_protocol::now_unix_ms();
                let hour = (c.cfg.judge.learning_hour + HOURS_AFTER_LEARNING) % 24;
                let (at, trigger) = due(now, hour, last);
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |_trigger: &'static str| {
                let core = core.clone();
                ran.store(
                    theseus_protocol::now_unix_ms(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                async move {
                    let Some(c) = core.upgrade() else { return };
                    let now = theseus_protocol::now_unix_ms();
                    match c.replay_sample(now).await {
                        Ok(r) => {
                            tracing::info!(
                                replayed = r.replayed,
                                judged = r.judged,
                                spent_usd = r.spent_usd,
                                stopped = ?r.stopped,
                                "route replay: the nightly sample ran"
                            );
                            let mark =
                                serde_json::json!({"at_unix_ms": now, "replayed": r.replayed});
                            let _ = theseus_store::blocking(|| c.store.put_meta(LAST_RUN, &mark));
                        }
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "route replay: the nightly sample did not run")
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }
}
