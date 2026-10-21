//! The one progress line (theseus-ypnxt.2, contract C1): what every surface
//! says of a running task, in the same words. `paused`, `step 3 of 5`,
//! `$0.40 of $2`, `~6 min left`, `on sonnet` (a task a checkpoint moved off its base), joined by ` · `, each part left
//! out when the view has no data for it. Where the forecast has tasks like it, the dollars
//! read against what they usually cost, `$0.40 of ~$0.60 (limit $2)`, beside
//! the time left (the owner, 2026-10-18: keep `[$x of ~$p50]`). A pure function of the view and the clock the
//! caller passes. The cockpit writes the same words in TypeScript from
//! `golden/progress_line.json`.

use crate::forecast::WorkForecast;
use crate::usd;
use crate::work::{Cost, WorkView};

const MIN_MS: f64 = 60_000.0;

/// `step N of M` for a plan with steps: the one being worked on, counted from
/// 1 and never past the last.
pub fn step_words(steps_done: u32, steps_total: u32) -> Option<String> {
    (steps_total > 0).then(|| {
        format!(
            "step {} of {steps_total}",
            (steps_done + 1).min(steps_total)
        )
    })
}

/// Minutes left as people say them: `<1 min`, `6 min`, `2 h`.
fn left(minutes: f64) -> String {
    let m = minutes.round() as u64;
    match m {
        0 => "<1 min".to_string(),
        1..=59 => format!("{m} min"),
        _ => format!("{} h", (minutes / 60.0).round() as u64),
    }
}

/// The time part: `~6 min left` counting down to the median; `past its usual
/// time` once the task has run past the median, its spend has, or the forecast
/// says so. Nothing for an ended task, one with no known start, or a forecast
/// made from no finished task.
fn time_words(v: &WorkView, now_ms: u64) -> Option<String> {
    let f = v.forecast.as_ref()?;
    if v.state.ended() {
        return None;
    }
    time_left(
        f,
        v.started_at_ms,
        v.cost.as_ref().map_or(0.0, |c| c.spent_usd),
        now_ms,
    )
}

/// The same from the forecast and the start alone, for a row that keeps no
/// whole view.
pub fn time_left(
    f: &WorkForecast,
    started_at_ms: u64,
    spent_usd: f64,
    now_ms: u64,
) -> Option<String> {
    if f.n == 0 || started_at_ms == 0 {
        return None;
    }
    let elapsed = now_ms.saturating_sub(started_at_ms) as f64 / MIN_MS;
    let to_p50 = f.min_p50 - elapsed;
    if f.past || to_p50 <= 0.0 || f.spend_past(spent_usd) {
        return Some("past its usual time".to_string());
    }
    Some(format!("~{} left", left(to_p50)))
}

/// The model's last words as a surface shows them: bare while they are as new as what the task does
/// (`doing_since_ms`, none when it does nothing) or under a minute old, else with their age,
/// `Round 1 done. Continuing. (2 min ago)`.
pub fn aged_said(said: &str, at_ms: u64, doing_since_ms: Option<u64>, now_ms: u64) -> String {
    let age = now_ms.saturating_sub(at_ms);
    if age < 60_000 || doing_since_ms.is_none_or(|d| at_ms >= d) {
        return said.to_string();
    }
    match age / 60_000 {
        m @ 1..=59 => format!("{said} ({m} min ago)"),
        m => format!("{said} ({} h ago)", m / 60),
    }
}

/// The line without `paused`: step, dollars and time left. A row that puts
/// `paused` ahead of what the task is doing says this after it.
pub fn progress_rest(v: &WorkView, now_ms: u64) -> Option<String> {
    let parts: Vec<String> = [
        v.progress
            .as_ref()
            .and_then(|p| step_words(p.steps_done, p.steps_total)),
        cost_part(v),
        time_words(v, now_ms),
        v.moved_to.as_ref().map(|p| format!("on {p}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// `$0.40 of $2`.
pub fn cost_words(c: &Cost) -> String {
    format!("{} of {}", usd(c.spent_usd), usd(c.limit_usd))
}

/// `$0.40 of ~$0.60`: the spend against the median of the tasks like it
/// (theseus-zz752's forecast). Nothing for a forecast made from none.
pub fn forecast_cost(f: &WorkForecast, spent_usd: f64) -> Option<String> {
    (f.n > 0).then(|| f.spent_of(spent_usd))
}

/// The dollars part: `$0.40 of ~$0.60 (limit $2)` where there is a forecast,
/// else `$0.40 of $2`; nothing without a cost.
pub fn cost_part(v: &WorkView) -> Option<String> {
    let c = v.cost.as_ref()?;
    match v
        .forecast
        .as_ref()
        .and_then(|f| forecast_cost(f, c.spent_usd))
    {
        Some(f) => Some(format!("{f} (limit {})", usd(c.limit_usd))),
        None => Some(cost_words(c)),
    }
}
/// The line, or `None` when no part has data.
pub fn progress_line(v: &WorkView, now_ms: u64) -> Option<String> {
    match (v.paused, progress_rest(v, now_ms)) {
        (true, Some(r)) => Some(format!("paused · {r}")),
        (true, None) => Some("paused".to_string()),
        (false, r) => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::tests::view;
    use crate::work::{Progress, WorkKind, WorkState};
    use serde_json::Value;

    fn num(v: &Value, k: &str) -> f64 {
        v[k].as_f64().expect(k)
    }

    fn forecast(n: u32, past: bool) -> WorkForecast {
        WorkForecast {
            usd_p50: 0.0,
            usd_p90: 0.0,
            min_p50: 10.0,
            min_p90: 20.0,
            n,
            past,
            read: None,
            read_p: None,
        }
    }

    #[test]
    fn the_golden_cases_hold() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("golden/progress_line.json")).unwrap();
        assert_eq!(cases.len(), 12);
        for c in cases {
            let mut v = view(WorkKind::Task, WorkState::Running, None);
            v.cost = c.get("cost").map(|k| Cost {
                spent_usd: num(k, "spent"),
                limit_usd: num(k, "limit"),
                turn_usd: None,
            });
            v.progress = c.get("progress").map(|p| Progress {
                steps_done: num(p, "done") as u32,
                steps_total: num(p, "total") as u32,
                current: None,
                said: None,
                at_ms: 0,
            });
            v.forecast = c.get("forecast").map(|f| WorkForecast {
                usd_p50: f.get("usd_p50").map_or(0.0, |x| x.as_f64().unwrap()),
                min_p50: num(f, "min_p50"),
                min_p90: num(f, "min_p90"),
                ..forecast(num(f, "n") as u32, false)
            });
            v.paused = c.get("paused").and_then(Value::as_bool).unwrap_or(false);
            let now = v.started_at_ms + (num(&c, "elapsed_min") * MIN_MS) as u64;
            let want = c["expect"].as_str().map(str::to_string);
            assert_eq!(progress_line(&v, now), want, "{}", c["case"]);
        }
    }

    /// A task a checkpoint moved off its base says where it runs, last; a task on its base
    /// reads as the golden cases do (theseus-ypnxt.9).
    #[test]
    fn a_moved_tasks_line_says_where_it_runs() {
        let cases: Vec<Value> =
            serde_json::from_str(include_str!("golden/progress_line_moved.json")).unwrap();
        assert_eq!(cases.len(), 3);
        for c in cases {
            let mut v = view(WorkKind::Task, WorkState::Running, None);
            v.cost = c.get("cost").map(|k| Cost {
                spent_usd: num(k, "spent"),
                limit_usd: num(k, "limit"),
                turn_usd: None,
            });
            v.progress = c.get("progress").map(|p| Progress {
                steps_done: num(p, "done") as u32,
                steps_total: num(p, "total") as u32,
                current: None,
                said: None,
                at_ms: 0,
            });
            v.forecast = c.get("forecast").map(|f| WorkForecast {
                usd_p50: f.get("usd_p50").map_or(0.0, |x| x.as_f64().unwrap()),
                min_p50: num(f, "min_p50"),
                min_p90: num(f, "min_p90"),
                ..forecast(num(f, "n") as u32, false)
            });
            v.paused = c.get("paused").and_then(Value::as_bool).unwrap_or(false);
            v.moved_to = c
                .get("moved_to")
                .and_then(Value::as_str)
                .map(str::to_string);
            let now = v.started_at_ms + (num(&c, "elapsed_min") * MIN_MS) as u64;
            let want = c["expect"].as_str().map(str::to_string);
            assert_eq!(progress_line(&v, now), want, "{}", c["case"]);
        }
    }

    /// The key test: a spend above the median never reads as `of ~` that median, and its time part
    /// says the usual is past (theseus-ztkhg).
    #[test]
    fn a_spend_past_its_usual_never_reads_as_under_it() {
        let mut v = view(WorkKind::Task, WorkState::Running, None);
        v.forecast = Some(WorkForecast {
            usd_p50: 0.02,
            ..forecast(4, false)
        });
        for (spent, want) in [
            (
                0.04,
                "$0.04, past its usual ~$0.02 (limit $1) · past its usual time",
            ),
            (0.01, "$0.01 of ~$0.02 (limit $1) · ~6 min left"),
        ] {
            v.cost = Some(Cost {
                spent_usd: spent,
                limit_usd: 1.0,
                turn_usd: None,
            });
            let now = v.started_at_ms + 4 * MIN_MS as u64;
            assert_eq!(progress_line(&v, now).as_deref(), Some(want));
        }
    }

    #[test]
    fn a_forecast_that_says_past_is_past_whatever_the_clock() {
        let mut v = view(WorkKind::Task, WorkState::Running, None);
        v.cost = None;
        v.forecast = Some(forecast(3, true));
        assert_eq!(
            progress_line(&v, v.started_at_ms + 60_000).as_deref(),
            Some("past its usual time")
        );
    }

    #[test]
    fn an_ended_task_has_no_time_left() {
        let mut v = view(WorkKind::Task, WorkState::Done, None);
        v.cost = None;
        v.forecast = Some(forecast(3, false));
        assert_eq!(progress_line(&v, v.started_at_ms + 60_000), None);
    }

    #[test]
    fn old_words_carry_their_age() {
        let (said, ten, now) = ("Round 1 done. Continuing.", 36_000_000, 36_120_000);
        assert_eq!(
            aged_said(said, ten, Some(ten + 60_000), now),
            "Round 1 done. Continuing. (2 min ago)"
        );
        assert_eq!(aged_said(said, ten + 90_000, Some(ten + 60_000), now), said);
        assert_eq!(aged_said(said, ten, Some(ten + 60_000), ten + 30_000), said);
        assert_eq!(aged_said(said, ten, None, now), said);
        assert!(aged_said(said, ten, Some(ten + 1), ten + 7_300_000).ends_with("(2 h ago)"));
    }

    #[test]
    fn hours_and_the_last_minute_are_said_coarsely() {
        assert_eq!(left(0.2), "<1 min");
        assert_eq!(left(59.4), "59 min");
        assert_eq!(left(125.0), "2 h");
    }
}
