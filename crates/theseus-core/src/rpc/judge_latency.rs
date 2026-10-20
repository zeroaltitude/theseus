//! `judge.latency` (theseus-xzxqv): how long a point's requests took, by
//! local day, from the `judge.call` rows' own times, so the week before and
//! the week after `onto.v1` joined the inbound batch can be set side by
//! side (`theseus judge latency --since <date>`). If the batch's p95 grows
//! by more than 50 ms, the owner sets `[ontology] request = "own"`.
//!
//! Each request counts once, whatever packs rode it: the judgments of one
//! call share its id and its time. The batch is the request that carries
//! `classify`, which `route.v6`'s own request never does.

use std::collections::BTreeMap;

use theseus_protocol::judge::{JudgeLatencyDay, JudgeLatencyParams, JudgeLatencyResult};
use theseus_store::pages::ledger_kind;

use super::server::RpcFailure;
use super::Core;
use crate::judge::spend::local_day;
use crate::ledger::LedgerRow;

/// The packs of a point, by the scope of their rows.
fn scopes(point: &str) -> Vec<String> {
    theseus_judge::pack::embedded()
        .as_ref()
        .map(|packs| {
            let mut s: Vec<String> = packs
                .iter()
                .filter(|p| format!("{:?}", p.point).eq_ignore_ascii_case(&point.replace('_', "")))
                .map(|p| format!("judge:{}", p.id))
                .collect();
            s.sort();
            s.dedup();
            s
        })
        .unwrap_or_default()
}

/// One request: its day, its packs and its total time.
struct Request {
    day: String,
    classify: bool,
    onto: bool,
    ms: u64,
}

/// The nearest-rank percentile of sorted `v`.
fn percentile(v: &[u64], p: u64) -> u64 {
    match v.len() {
        0 => 0,
        n => v[((n as u64 * p).div_ceil(100).max(1) - 1) as usize],
    }
}

/// The days' figures, from the requests, in day order.
fn days(requests: &[Request], since: &str) -> Vec<JudgeLatencyDay> {
    let mut by: BTreeMap<&str, (Vec<u64>, Vec<u64>)> = BTreeMap::new();
    for r in requests
        .iter()
        .filter(|r| r.classify && r.day.as_str() >= since)
    {
        let d = by.entry(&r.day).or_default();
        d.0.push(r.ms);
        if r.onto {
            d.1.push(r.ms);
        }
    }
    by.into_iter()
        .map(|(day, (mut all, mut onto))| {
            all.sort_unstable();
            onto.sort_unstable();
            JudgeLatencyDay {
                day: day.to_string(),
                requests: all.len() as u64,
                p50_ms: percentile(&all, 50),
                p95_ms: percentile(&all, 95),
                with_onto: onto.len() as u64,
                onto_p95_ms: percentile(&onto, 95),
            }
        })
        .collect()
}

impl Core {
    /// `judge.latency`: the point's batch requests by day.
    pub fn judge_latency(&self, p: JudgeLatencyParams) -> Result<JudgeLatencyResult, RpcFailure> {
        let point = p.point.unwrap_or_else(|| "inbound".into());
        let since = p.since.unwrap_or_else(|| {
            local_day(theseus_protocol::now_unix_ms().saturating_sub(7 * 86_400_000))
        });
        let scopes = scopes(&point);
        let mut calls: BTreeMap<String, Request> = BTreeMap::new();
        let mut note = |r: &theseus_store::Record| {
            let Ok(row) = r.decode::<LedgerRow>() else {
                return;
            };
            if row.kind != theseus_protocol::LedgerKind::JudgeCall.as_str() {
                return;
            }
            let d = &row.data;
            let (Some(id), Some(pack), Some(ms)) = (
                d["call"]["id"].as_str(),
                d["pack"].as_str(),
                d["timing"]["total_ms"].as_u64(),
            ) else {
                return;
            };
            let c = calls.entry(id.to_string()).or_insert_with(|| Request {
                day: local_day(row.at_unix_ms),
                classify: false,
                onto: false,
                ms,
            });
            c.classify |= pack.starts_with("classify.");
            c.onto |= pack.starts_with("onto.");
        };
        let since_ms = day_start_ms(&since);
        let tag = ledger_kind(theseus_protocol::LedgerKind::JudgeCall.as_str());
        let mut before = None;
        let mut paged = true;
        loop {
            let page = theseus_store::Page {
                kind: theseus_store::kinds::LEDGER,
                tags: vec![tag.clone()],
                after: None,
                before,
                since_ms,
                until_ms: None,
                limit: 500,
            };
            let Some(out) = self.store.ledger_page(&page).map_err(RpcFailure::invalid)? else {
                paged = false;
                break;
            };
            out.records
                .iter()
                .filter(|r| r.scope.as_ref().is_some_and(|s| scopes.contains(s)))
                .for_each(&mut note);
            match (out.more, out.first) {
                (true, Some(first)) => before = Some(first),
                _ => break,
            }
        }
        if !paged {
            for scope in &scopes {
                for r in self
                    .store
                    .scope_after(scope, 0)
                    .map_err(RpcFailure::invalid)?
                {
                    note(&r);
                }
            }
        }
        let requests: Vec<Request> = calls.into_values().collect();
        Ok(JudgeLatencyResult {
            point,
            days: days(&requests, &since),
        })
    }
}

/// The start of the day before the local day `day` (`2026-10-19`), in ms
/// since the epoch, as a bound to read from (the zone's offset never cuts
/// the day); `None` for a date that does not read.
fn day_start_ms(day: &str) -> Option<u64> {
    let mut parts = day.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
    // Days from 1970-01-01 to the date (Howard Hinnant's civil-from-days).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from((days - 1) * 86_400_000).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(day: &str, classify: bool, onto: bool, ms: u64) -> Request {
        Request {
            day: day.into(),
            classify,
            onto,
            ms,
        }
    }

    #[test]
    fn a_day_is_its_batch_requests_percentiles_and_route_is_left_out() {
        let mut v: Vec<Request> = (1..=20)
            .map(|n| r("2026-10-19", true, false, n * 10))
            .collect();
        v.push(r("2026-10-19", false, false, 9_999));
        v.extend((1..=4).map(|n| r("2026-10-20", true, true, 100 * n)));
        let d = days(&v, "2026-10-19");
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].requests, d[0].p50_ms, d[0].p95_ms), (20, 100, 190));
        assert_eq!((d[0].with_onto, d[0].onto_p95_ms), (0, 0));
        assert_eq!(
            (d[1].requests, d[1].with_onto, d[1].onto_p95_ms),
            (4, 4, 400)
        );
        assert_eq!(
            days(&v, "2026-10-20").len(),
            1,
            "since drops the earlier day"
        );
    }

    #[test]
    fn a_date_reads_to_the_start_of_the_day_before() {
        // 2026-10-19T00:00Z is 1,792,368,000 s after the epoch.
        assert_eq!(
            day_start_ms("2026-10-19"),
            Some(1_792_368_000_000 - 86_400_000)
        );
        assert_eq!(day_start_ms("1970-01-02"), Some(0));
        assert_eq!(day_start_ms("soon"), None);
    }
}
