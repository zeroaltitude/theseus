//! `self.pipeline` (theseus-pw1q.16): the three queues and the owner's, what
//! holds the next start and why, today's spend and rows, and with `week` the
//! rsi-plan's measures: rows a day, the accept rate, regressions at the next
//! rerun, and the cost of a row against today's. A read: nothing is written.

use anyhow::Result;
use theseus_protocol::rsi::{
    SelfPipelineDay, SelfPipelineItem, SelfPipelineParams, SelfPipelineResult, SelfPipelineRoom,
    SelfPipelineWeek,
};

use super::queues::Entry;
use super::{Builder, Local, Seams};
use crate::judge::spend::local_day;
use crate::rpc::Core;
use crate::rsi::join::rows_of;
use crate::rsi::{gate, Gate};

const DAY_MS: u64 = 86_400_000;

/// The kinds the pipeline writes.
pub(crate) const KINDS: [&str; 7] = [
    "self.pipeline.started",
    "self.pipeline.held",
    "self.pipeline.returned",
    "self.pipeline.parked",
    "self.pipeline.queued",
    "self.pipeline.cleared",
    "self.pipeline.ended",
];

fn item(e: &Entry, queue: &str) -> SelfPipelineItem {
    SelfPipelineItem {
        item: e.item.clone(),
        title: e.title.clone(),
        queue: queue.into(),
        place: e.place,
        branch: e.branch.clone(),
        session: e.session.clone(),
        since_ms: e.since_ms,
        rejections: e.rejections,
        why: e.why.clone(),
    }
}

impl Core {
    /// The answer: the module's doc.
    pub fn self_pipeline(&self, p: &SelfPipelineParams) -> Result<SelfPipelineResult> {
        let state = crate::rsi::state(&self.self_ctx());
        let mut out = SelfPipelineResult {
            builder: Local.name().into(),
            budget: self.runner.self_budget.view(),
            state,
            ..SelfPipelineResult::default()
        };
        let cfg = &self.cfg.self_improve;
        out.room.slots = cfg.pipeline_builds;
        out.room.min_free_gb = cfg.min_free_gb;
        let Some(repo) = cfg.repo.clone() else {
            out.room.why = Some("[self] repo is unset, so nothing is built".into());
            return Ok(out);
        };
        let place = self.tools.self_pr.place(&repo);
        let seams = Seams::default();
        let Some(q) = self.pipeline_queues(&place, seams.grace)? else {
            out.building_index = true;
            return Ok(out);
        };
        let (room, hold) = self.pipeline_hold(&seams, &place, q.building.len(), &q.ready);
        out.room = SelfPipelineRoom {
            slots: room.slots as u32,
            building: room.building as u32,
            trees: room.trees as u32,
            free_gb: room.free_gb,
            min_free_gb: room.min_free_gb,
            memory_psi: room.memory,
            cpu_psi: room.cpu,
            hold: hold.as_ref().map(|h| h.0.clone()),
            why: hold.map(|h| h.1),
        };
        match gate(&self.self_ctx()) {
            Gate::Allowed => {}
            Gate::Off => {
                out.room.hold = Some("off".into());
                out.room.why = Some("[self] mode is \"off\": nothing self-directed runs".into());
            }
            Gate::Halted { why, .. } => {
                out.room.hold = Some("halted".into());
                out.room.why = Some(format!("self-improvement is halted: {why}"));
            }
        }
        out.ready = q.ready.iter().map(|e| item(e, "ready")).collect();
        out.building = q.building.iter().map(|e| item(e, "building")).collect();
        out.reviewing = q.reviewing.iter().map(|e| item(e, "reviewing")).collect();
        out.joining = q.joining.iter().map(|e| item(e, "joining")).collect();
        out.held = q.held.iter().map(|e| item(e, "held")).collect();
        let now = theseus_protocol::now_unix_ms();
        let today = local_day(now);
        if let Some(rows) = rows_of(&self.store, &KINDS)? {
            out.rows_today = rows
                .iter()
                .filter(|r| local_day(r.at_unix_ms) == today)
                .count() as u32;
        }
        if p.week {
            out.week = self.pipeline_week(now)?;
        }
        Ok(out)
    }

    /// The week's measures, oldest day first; none while the index is built.
    pub(crate) fn pipeline_week(&self, now: u64) -> Result<Option<SelfPipelineWeek>> {
        let Some(rows) = rows_of(
            &self.store,
            &[
                "self.pipeline.started",
                "self.branch.built",
                "self.review.verdict",
                "self.joined",
                "self.reverted",
                "self.vetoed",
            ],
        )?
        else {
            return Ok(None);
        };
        let mut days: Vec<SelfPipelineDay> = (0..7)
            .rev()
            .map(|n| SelfPipelineDay {
                day: local_day(now.saturating_sub(n * DAY_MS)),
                ..SelfPipelineDay::default()
            })
            .collect();
        for r in rows {
            let day = local_day(r.at_unix_ms);
            let Some(d) = days.iter_mut().find(|d| d.day == day) else {
                continue;
            };
            let data = &r.data;
            match r.kind.as_str() {
                "self.pipeline.started" => d.started += 1,
                "self.branch.built" => {
                    d.built += u32::from(data["gate"] == "green");
                    d.cost_usd += data["cost_usd"].as_f64().unwrap_or(0.0);
                }
                "self.review.verdict" => {
                    match data["verdict"].as_str() {
                        Some("accept") => d.accepted += 1,
                        Some("reject") => d.rejected += 1,
                        _ => {}
                    }
                    d.cost_usd += data["numbers"]["cost_usd"].as_f64().unwrap_or(0.0);
                }
                "self.joined" => d.joined += 1,
                _ => d.reverted += 1,
            }
        }
        let sum = |f: fn(&SelfPipelineDay) -> u32| days.iter().map(f).sum::<u32>();
        let (accepted, rejected, joined) =
            (sum(|d| d.accepted), sum(|d| d.rejected), sum(|d| d.joined));
        let cost: f64 = days.iter().map(|d| d.cost_usd).sum();
        let last = days.last().cloned().unwrap_or_default();
        let a_row = |c: f64, j: u32| (j > 0).then(|| (c / f64::from(j) * 100.0).round() / 100.0);
        Ok(Some(SelfPipelineWeek {
            rows_a_day: (f64::from(joined) / 7.0 * 100.0).round() / 100.0,
            accept_rate: (accepted + rejected > 0)
                .then(|| f64::from(accepted) / f64::from(accepted + rejected)),
            regressions: sum(|d| d.reverted),
            cost_a_row_usd: a_row(cost, joined),
            today_cost_a_row_usd: a_row(last.cost_usd, last.joined),
            days,
        }))
    }
}
