//! `route.report` (own-benchmark): what the owner's own turns say about the
//! model they need. Per (model, effort) cell: the turns and what they cost;
//! the turns the sampler replayed one rung down and how many `compare.v1`
//! called `same` or `better` at the bar; the turns the owner checked (a
//! routing correction is a label); and the dollars moving the cell one rung
//! down would have saved, at the stated confidence:
//!
//!   movable = spend × floor × saving
//!
//! where `floor` is the one-sided Wilson lower bound of the match rate and
//! `saving` is what the replays' first requests cost less than the originals'.
//! A cell with no judged replay moves nothing. Read only: every number is a
//! row already written. The replays' cost is shown against `[route_bench]
//! replay_day_usd`.

use std::collections::{BTreeMap, BTreeSet};

use theseus_protocol::own_bench::{
    DataCell, GateCount, RouteCell, RouteReportParams, RouteReportResult,
};
use theseus_protocol::LedgerKind;

use super::sampler::Candidate;
use crate::learning::local_midnight;
use crate::rpc::Core;

/// The bar a judged replay must reach: `same` plus `better` at this.
pub use crate::judge::compare::MATCH_BAR as BAR;
/// The confidence the floor is stated at: one side, z = 1.2816.
pub const CONFIDENCE: f64 = 0.9;
const Z: f64 = 1.2816;

/// A turn, as the report counts it.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub model: String,
    pub effort: String,
    pub cost_usd: f64,
    /// The owner corrected its routing.
    pub checked: bool,
    /// What its `route.decided` rows say of the data rule (theseus-a1wvq.11).
    pub data: Data,
}

/// A turn's data rule as its route rows wrote it: the origin it ended on
/// (none: the rows predate it), Jev's data answer, and why the rule kept it
/// off a restricted-origin model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Data {
    pub origin: Option<String>,
    pub verdict: Option<String>,
    pub gate: Option<String>,
    /// Why a class that drafts did not, this turn (theseus-a1wvq.12).
    pub draft_skip: Option<String>,
}

/// The origin a turn's cell says: `unrecorded` before routing wrote one.
fn origin_of(d: &Data) -> &str {
    d.origin.as_deref().unwrap_or("unrecorded")
}

/// Per origin and data verdict, and the gate's skips per reason, sorted.
pub fn data_cells(turns: &[Turn]) -> (Vec<DataCell>, Vec<GateCount>) {
    let mut cells: BTreeMap<(&str, &str), DataCell> = BTreeMap::new();
    let mut gates: BTreeMap<&str, u64> = BTreeMap::new();
    for t in turns {
        let (origin, data) = (
            origin_of(&t.data),
            t.data.verdict.as_deref().unwrap_or("none"),
        );
        let c = cells.entry((origin, data)).or_insert_with(|| DataCell {
            origin: origin.into(),
            data: data.into(),
            ..DataCell::default()
        });
        c.turns += 1;
        c.spend_usd += t.cost_usd;
        if let Some(g) = &t.data.gate {
            *gates.entry(g).or_default() += 1;
        }
    }
    let gates = gates
        .into_iter()
        .map(|(why, turns)| GateCount {
            why: why.into(),
            turns,
        })
        .collect();
    (cells.into_values().collect(), gates)
}

/// A `route.replay.judged` row, as the report reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    pub from_model: String,
    pub from_effort: String,
    pub to_profile: String,
    pub same_or_better: Option<f64>,
    pub orig_usd: f64,
    pub replay_usd: f64,
    pub judge_usd: f64,
}

/// A whole-turn `turn.replayed` row, as the report reads it
/// (theseus-pw1q.20).
#[derive(Debug, Clone, PartialEq)]
pub struct Whole {
    pub from_model: String,
    pub from_effort: String,
    pub to_profile: String,
    pub same_or_better: Option<f64>,
    /// The last test run was the same on both sides; none when neither ran
    /// one.
    pub tests_same: Option<bool>,
}

/// The whole-turn columns of `cells`: a cell the sampler never saw is made.
pub fn add_wholes(cells: &mut Vec<RouteCell>, wholes: &[Whole]) {
    for w in wholes {
        let at = cells
            .iter()
            .position(|c| c.model == w.from_model && c.effort == w.from_effort);
        let c = match at {
            Some(i) => &mut cells[i],
            None => {
                cells.push(RouteCell {
                    model: w.from_model.clone(),
                    effort: w.from_effort.clone(),
                    ..RouteCell::default()
                });
                cells.last_mut().expect("just pushed")
            }
        };
        if c.rung_down.is_none() {
            c.rung_down = Some(w.to_profile.clone());
        }
        if let Some(p) = w.same_or_better {
            c.whole_replayed += 1;
            c.whole_matched += u64::from(p >= BAR);
            c.tests_same += u64::from(w.tests_same == Some(true));
        }
    }
}

/// The lower bound of a rate of `k` in `n`, one-sided at z = 1.2816.
pub fn wilson_floor(k: u64, n: u64) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let (k, n) = (k as f64, n as f64);
    let p = k / n;
    let z2 = Z * Z;
    let centre = p + z2 / (2.0 * n);
    let spread = Z * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt();
    ((centre - spread) / (1.0 + z2 / n)).max(0.0)
}

/// The cells: pure over the turns and the judged rows.
pub fn cells(turns: &[Turn], judged: &[Judged]) -> Vec<RouteCell> {
    let mut by: BTreeMap<(String, String), RouteCell> = BTreeMap::new();
    for t in turns {
        let c = by.entry((t.model.clone(), t.effort.clone())).or_default();
        c.model.clone_from(&t.model);
        c.effort.clone_from(&t.effort);
        match c.origin.as_str() {
            "" => c.origin = origin_of(&t.data).into(),
            o if o != origin_of(&t.data) => c.origin = "mixed".into(),
            _ => {}
        }
        c.turns += 1;
        c.spend_usd += t.cost_usd;
        c.checked += u64::from(t.checked);
    }
    // Per cell: the judged replays' first-request cost, original and replay.
    let mut cost: BTreeMap<(String, String), (f64, f64)> = BTreeMap::new();
    for j in judged {
        let key = (j.from_model.clone(), j.from_effort.clone());
        let c = by.entry(key.clone()).or_default();
        c.model.clone_from(&j.from_model);
        c.effort.clone_from(&j.from_effort);
        c.replay_usd += j.replay_usd + j.judge_usd;
        c.rung_down = Some(j.to_profile.clone());
        if let Some(p) = j.same_or_better {
            c.replayed += 1;
            c.matched += u64::from(p >= BAR);
            let e = cost.entry(key).or_default();
            e.0 += j.orig_usd;
            e.1 += j.replay_usd;
        }
    }
    let mut out: Vec<RouteCell> = by.into_values().collect();
    for c in &mut out {
        if c.replayed == 0 {
            continue;
        }
        let floor = wilson_floor(c.matched, c.replayed);
        c.match_floor = Some(floor);
        let (orig, replay) = cost
            .get(&(c.model.clone(), c.effort.clone()))
            .copied()
            .unwrap_or_default();
        if orig > 0.0 {
            let ratio = replay / orig;
            c.cost_ratio = Some(ratio);
            c.movable_usd = c.spend_usd * floor * (1.0 - ratio).max(0.0);
        }
    }
    out.sort_by(|a, b| {
        b.spend_usd
            .partial_cmp(&a.spend_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| (&a.model, &a.effort).cmp(&(&b.model, &b.effort)))
    });
    out
}

impl Core {
    /// Each turn's data rule from its `route.decided` rows since `since_ms`:
    /// the origin of the last row that names one (the model the turn ended
    /// on), the first data answer (a row before content flags) or its `cn`
    /// word, the first gate word. A row from before
    /// the keys has none of them.
    fn route_data_since(&self, since_ms: u64) -> anyhow::Result<BTreeMap<String, Data>> {
        let mut out: BTreeMap<String, Data> = BTreeMap::new();
        for r in self.rows_since(LedgerKind::RouteDecided, since_ms)? {
            let Some(turn) = r.turn_id else { continue };
            let d = out.entry(turn).or_default();
            let word = |k: &str| r.data[k].as_str().map(str::to_string);
            d.origin = word("origin").or(d.origin.take());
            d.verdict = d
                .verdict
                .take()
                .or_else(|| word("data"))
                .or_else(|| word("cn"));
            d.gate = d.gate.take().or_else(|| word("data_gate"));
            d.draft_skip = d.draft_skip.take().or_else(|| word("draft_skip"));
        }
        Ok(out)
    }

    /// `route.report`: a day, or a week with `week`.
    pub async fn route_report(
        self: &std::sync::Arc<Self>,
        p: RouteReportParams,
    ) -> anyhow::Result<RouteReportResult> {
        let me = self.clone();
        let week = p.week.unwrap_or(false);
        tokio::task::spawn_blocking(move || {
            me.route_report_now(week, theseus_protocol::now_unix_ms())
        })
        .await?
    }

    pub(crate) fn route_report_now(
        &self,
        week: bool,
        now_ms: u64,
    ) -> anyhow::Result<RouteReportResult> {
        let days = if week { 7 } else { 1 };
        let since = local_midnight(now_ms).saturating_sub((days - 1) * 86_400_000);
        let corrected: BTreeSet<String> = self
            .rows_since(LedgerKind::RouteCorrected, since)?
            .into_iter()
            .filter_map(|r| r.data["turn"].as_str().map(str::to_string))
            .collect();
        let turns = self.turns_since(since)?;
        let mut data = self.route_data_since(since)?;
        let lines: Vec<Turn> = turns
            .iter()
            .map(|c: &Candidate| Turn {
                model: c.model.clone(),
                effort: c.effort.clone(),
                cost_usd: c.cost_usd,
                checked: corrected.contains(&c.turn),
                data: data.remove(&c.turn).unwrap_or_default(),
            })
            .collect();
        let judged: Vec<Judged> = self
            .rows_since(LedgerKind::RouteReplayJudged, since)?
            .into_iter()
            .map(|r| {
                let d = &r.data;
                let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
                Judged {
                    from_model: s("from_model"),
                    from_effort: s("from_effort"),
                    to_profile: s("to_profile"),
                    same_or_better: d["same_or_better"].as_f64(),
                    orig_usd: d["orig_usd"].as_f64().unwrap_or(0.0),
                    replay_usd: d["replay_usd"].as_f64().unwrap_or(0.0),
                    judge_usd: d["judge_usd"].as_f64().unwrap_or(0.0),
                }
            })
            .collect();
        let mut cells = cells(&lines, &judged);
        let (data, gates) = data_cells(&lines);
        let draft_rows: Vec<_> = self
            .rows_since(LedgerKind::TaskDraft, since)?
            .iter()
            .map(|r| super::drafts::Draft::of_row(&r.data))
            .collect();
        let wholes: Vec<Whole> = self
            .rows_since(LedgerKind::TurnReplayed, since)?
            .into_iter()
            .filter(|r| r.data["scope"] == super::whole::WHOLE_TURN)
            .map(|r| {
                let d = &r.data;
                let s = |k: &str| d[k].as_str().unwrap_or_default().to_string();
                Whole {
                    from_model: s("from_model"),
                    from_effort: s("from_effort"),
                    to_profile: s("profile"),
                    same_or_better: d["same_or_better"].as_f64(),
                    tests_same: d["whole"]["same"]["tests"].as_bool(),
                }
            })
            .collect();
        add_wholes(&mut cells, &wholes);
        let labels_fed = self
            .rows_since(LedgerKind::JudgeLabel, since)?
            .iter()
            .filter(|r| r.data["source"] == crate::learning::REPLAY_SOURCE)
            .count() as u64;
        let cap = self.cfg.route_bench.replay_day_usd;
        Ok(RouteReportResult {
            window_days: days,
            bar: BAR,
            confidence: CONFIDENCE,
            total_spend_usd: cells.iter().map(|c| c.spend_usd).sum::<f64>() + 0.0,
            total_movable_usd: cells.iter().map(|c| c.movable_usd).sum::<f64>() + 0.0,
            cells,
            data,
            gates,
            drafts: super::drafts::drafts(&draft_rows),
            draft_skips: super::drafts::skips(&lines),
            labels_fed,
            replay_today_usd: self.replay_spent_today(now_ms)?,
            replay_day_usd: cap,
            sampler: (cap <= 0.0)
                .then(|| "[route_bench] replay_day_usd = 0: the sampler is off".into()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(model: &str, effort: &str, cost: f64, checked: bool) -> Turn {
        Turn {
            model: model.into(),
            effort: effort.into(),
            cost_usd: cost,
            checked,
            data: Data::default(),
        }
    }

    fn judged(p: Option<f64>, orig: f64, replay: f64) -> Judged {
        Judged {
            from_model: "sonnet".into(),
            from_effort: "high".into(),
            to_profile: "haiku".into(),
            same_or_better: p,
            orig_usd: orig,
            replay_usd: replay,
            judge_usd: 0.001,
        }
    }

    /// The numbers from planted rows: ten turns at $0.50, three replayed,
    /// two matched at the bar, the replays costing a fifth of the originals.
    #[test]
    fn the_cells_numbers_come_from_the_planted_rows() {
        let mut turns: Vec<Turn> = (0..10)
            .map(|i| turn("sonnet", "high", 0.5, i < 2))
            .collect();
        turns.push(turn("fable", "", 2.0, false));
        let rows = [
            judged(Some(0.95), 0.10, 0.02),
            judged(Some(0.85), 0.20, 0.04),
            judged(Some(0.40), 0.10, 0.02),
            judged(None, 0.0, 0.03),
        ];
        let got = cells(&turns, &rows);
        assert_eq!(got.len(), 2);
        let s = got.iter().find(|c| c.model == "sonnet").unwrap();
        assert_eq!((s.turns, s.checked, s.replayed, s.matched), (10, 2, 3, 2));
        assert!((s.spend_usd - 5.0).abs() < 1e-9);
        assert_eq!(s.rung_down.as_deref(), Some("haiku"));
        let ratio = s.cost_ratio.unwrap();
        assert!((ratio - 0.2).abs() < 1e-9, "{ratio}");
        let floor = s.match_floor.unwrap();
        assert!((floor - wilson_floor(2, 3)).abs() < 1e-12);
        assert!((s.movable_usd - 5.0 * floor * 0.8).abs() < 1e-9);
        // The unjudged replay's cost counts against the cap, not the rate.
        assert!((s.replay_usd - (0.02 + 0.04 + 0.02 + 0.03 + 4.0 * 0.001)).abs() < 1e-9);
        // A cell with no judged replay moves nothing and says no floor.
        let f = got.iter().find(|c| c.model == "fable").unwrap();
        assert_eq!((f.turns, f.replayed), (1, 0));
        assert!(f.match_floor.is_none() && f.movable_usd == 0.0);
    }

    /// The bar is inclusive, and the floor sits under the observed rate and
    /// climbs with the evidence.
    #[test]
    fn the_bar_is_inclusive_and_the_floor_is_below_the_rate() {
        let got = cells(
            &[turn("sonnet", "high", 1.0, false)],
            &[judged(Some(BAR), 0.1, 0.01)],
        );
        assert_eq!(got[0].matched, 1);
        assert!(wilson_floor(1, 1) < 1.0);
        assert!(wilson_floor(30, 30) > wilson_floor(3, 3));
        assert!(wilson_floor(9, 10) < 0.9);
        assert_eq!(wilson_floor(0, 0), 0.0);
    }

    /// The whole-turn columns count judged replays at the bar and the ones
    /// whose tests ran the same, in the cell of the model they replayed; an
    /// unjudged one counts nowhere, and a cell the sampler never saw is made.
    #[test]
    fn the_whole_turn_columns_count_judged_replays_and_their_tests() {
        let whole = |p: Option<f64>, tests: Option<bool>| Whole {
            from_model: "sonnet".into(),
            from_effort: "high".into(),
            to_profile: "haiku".into(),
            same_or_better: p,
            tests_same: tests,
        };
        let mut got = cells(&[turn("sonnet", "high", 1.0, false)], &[]);
        add_wholes(
            &mut got,
            &[
                whole(Some(0.95), Some(true)),
                whole(Some(0.5), Some(true)),
                whole(Some(0.9), None),
                whole(None, Some(true)),
                Whole {
                    from_model: "fable".into(),
                    ..whole(Some(0.99), Some(false))
                },
            ],
        );
        let s = got.iter().find(|c| c.model == "sonnet").unwrap();
        assert_eq!((s.whole_replayed, s.whole_matched, s.tests_same), (3, 2, 2));
        assert_eq!(s.rung_down.as_deref(), Some("haiku"));
        assert_eq!(s.replayed, 0, "the first-loop columns are the sampler's");
        let f = got.iter().find(|c| c.model == "fable").unwrap();
        assert_eq!(
            (f.turns, f.whole_replayed, f.whole_matched, f.tests_same),
            (0, 1, 1, 0)
        );
    }

    fn cells_of(turns: &[Turn]) -> Vec<RouteCell> {
        cells(turns, &[])
    }

    /// The data columns from planted rows (theseus-a1wvq.11): turns and spend
    /// per origin and verdict, the gate's skips per reason, a mixed cell, and
    /// an old row, which has no origin, counted as `unrecorded` (never as
    /// `unknown`, a real restricted origin).
    #[test]
    fn the_report_counts_turns_per_origin_and_data_and_gate_skips() {
        let with =
            |model: &str, cost: f64, origin: Option<&str>, v: Option<&str>, g: Option<&str>| Turn {
                data: Data {
                    origin: origin.map(Into::into),
                    verdict: v.map(Into::into),
                    gate: g.map(Into::into),
                    ..Data::default()
                },
                ..turn(model, "", cost, false)
            };
        let turns = [
            with("glm", 0.10, Some("cn"), Some("open"), None),
            with("glm", 0.20, Some("cn"), Some("open"), None),
            with(
                "sonnet",
                1.00,
                Some("us"),
                Some("private"),
                Some("verdict_private"),
            ),
            with("sonnet", 2.00, Some("us"), None, None),
            with("sonnet", 4.00, None, None, None),
            with(
                "sonnet",
                0.50,
                Some("unknown"),
                Some("unclear"),
                Some("verdict_unclear"),
            ),
            with("mixed", 1.00, Some("us"), None, Some("verdict_private")),
            with("mixed", 1.00, Some("cn"), None, None),
        ];
        let (data, gates) = data_cells(&turns);
        let got: Vec<_> = data
            .iter()
            .map(|c| (c.origin.as_str(), c.data.as_str(), c.turns, c.spend_usd))
            .collect();
        assert_eq!(
            got,
            [
                ("cn", "none", 1, 1.0),
                ("cn", "open", 2, 0.30000000000000004),
                ("unknown", "unclear", 1, 0.5),
                ("unrecorded", "none", 1, 4.0),
                ("us", "none", 2, 3.0),
                ("us", "private", 1, 1.0),
            ]
        );
        let gates: Vec<_> = gates.iter().map(|g| (g.why.as_str(), g.turns)).collect();
        assert_eq!(gates, [("verdict_private", 2), ("verdict_unclear", 1)]);
        let cells = cells(&turns, &[]);
        let origin = |m: &str| cells.iter().find(|c| c.model == m).unwrap().origin.clone();
        assert_eq!(origin("glm"), "cn");
        assert_eq!(origin("mixed"), "mixed");
        assert_eq!(origin("sonnet"), "mixed", "an old row beside recorded ones");
        let old = cells_of(&turns[4..5]);
        assert_eq!(old[0].origin, "unrecorded");
    }
}
