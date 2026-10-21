//! A task's forecast (theseus-zz752): what finished tasks like it cost, how
//! long they took, and whether this one has passed the 90th percentile of
//! either. Numbers only: no brief, no title of the tasks it is made from, so
//! a view in a shared place carries it unscrubbed.
//!
//! `WorkForecast` rides a task's `WorkView` (additive). `words()` is the one
//! wording every surface shows. `forecast.calibration` is the owner's read of
//! how often the forecast held: of the tasks that finished in the last 7 and
//! 30 days, how many came in under their p90, by class (the forecast each
//! would have had when it opened, from the tasks that had finished before).

use serde::{Deserialize, Serialize};

use crate::usd;

/// What a task is expected to cost and take, from the finished tasks nearest
/// to it. Minutes and dollars, each at the median and the 90th percentile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WorkForecast {
    pub usd_p50: f64,
    pub usd_p90: f64,
    pub min_p50: f64,
    pub min_p90: f64,
    /// How many finished tasks it is made from.
    pub n: u32,
    /// The task has passed its p90, of dollars or of minutes, whichever came
    /// first; sticky once true. The notice fires on the change to true.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub past: bool,
    /// Jev's latest read of the task (`loop.v1`'s `work_state`), when the
    /// judge ran on it: `progressing`, `thrashing`, and the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub read: Option<String>,
    /// The read's confidence, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub read_p: Option<f32>,
}

/// Minutes as people say them in a forecast: whole, but `0.5` under a
/// minute stays a fraction, never `0 min`.
fn min(m: f64) -> String {
    if m < 1.0 {
        format!("{:.1}", (m * 10.0).round() / 10.0)
    } else {
        format!("{}", m.round() as u64)
    }
}

impl WorkForecast {
    /// `forecast 8 min (p90 14) · $0.60 (p90 $1.50) · from 23 like it`.
    pub fn words(&self) -> String {
        format!(
            "forecast {} min (p90 {}) · {} (p90 {}) · from {} like it",
            min(self.min_p50),
            min(self.min_p90),
            usd(self.usd_p50),
            usd(self.usd_p90),
            self.n
        )
    }

    /// The spend is past the median of the tasks like it.
    pub fn spend_past(&self, spent_usd: f64) -> bool {
        spent_usd > self.usd_p50
    }

    /// `$0.42 of ~$0.60`: what a running task has spent against its median;
    /// past it, `$0.72, past its usual ~$0.60`, so an `of ~` figure is never
    /// below the spend it follows.
    pub fn spent_of(&self, spent_usd: f64) -> String {
        let (spent, p50) = (usd(spent_usd), usd(self.usd_p50));
        match self.spend_past(spent_usd) {
            true => format!("{spent}, past its usual ~{p50}"),
            false => format!("{spent} of ~{p50}"),
        }
    }

    /// What the judge last read, as the notice says it: `still progressing
    /// (loop.v1 0.91)`. None when the judge has not run on the task.
    pub fn read_words(&self) -> Option<String> {
        let (read, p) = (self.read.as_deref()?, self.read_p?);
        let says = match read {
            "progressing" => "still progressing",
            "thrashing" => "looks like it is thrashing",
            "blocked_needs_human" => "looks blocked on a person",
            "off_task" => "looks off task",
            "complete" => "looks complete",
            _ => "unclear",
        };
        Some(format!("{says} (loop.v1 {p:.2})"))
    }
}

/// The line for a task that has no forecast: fewer than `MIN_LIKE` finished
/// tasks are like it.
pub fn none_words(like: u32) -> String {
    format!("no forecast yet: {like} like it")
}

/// Fewer finished tasks than this are no forecast.
pub const MIN_LIKE: u32 = 5;

/// `forecast.calibration`'s params: none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ForecastCalibrationParams {}

/// One class of task over one window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalibrationClass {
    pub class: String,
    /// Tasks of the class that finished in the window.
    pub finished: u32,
    /// Of them, those that would have had a forecast when they opened.
    pub forecast: u32,
    /// Of those, the tasks that finished inside their p90 of dollars and of
    /// minutes.
    pub inside: u32,
}

/// One window: the last `days` days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CalibrationWindow {
    pub days: u32,
    pub classes: Vec<CalibrationClass>,
}

/// `forecast.calibration`'s answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ForecastCalibrationResult {
    pub at_ms: u64,
    /// The finished tasks the daemon holds for forecasting.
    pub table: u32,
    /// Whether the table has been read from the store yet (it is, a moment
    /// after the daemon serves).
    pub ready: bool,
    pub windows: Vec<CalibrationWindow>,
}

impl ForecastCalibrationResult {
    /// The lines `theseus tasks forecast` prints: one per class per window,
    /// the share that held, and the total.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.ready {
            out.push("the forecast table is still being read; try again in a moment".into());
            return out;
        }
        for w in &self.windows {
            out.push(format!("last {} days:", w.days));
            if w.classes.is_empty() {
                out.push("  no task finished".into());
            }
            let (mut fin, mut fc, mut ins) = (0, 0, 0);
            for c in &w.classes {
                out.push(format!(
                    "  {:<22} {} finished, {}",
                    c.class,
                    c.finished,
                    held(c.forecast, c.inside)
                ));
                fin += c.finished;
                fc += c.forecast;
                ins += c.inside;
            }
            if w.classes.len() > 1 {
                out.push(format!("  {:<22} {fin} finished, {}", "all", held(fc, ins)));
            }
        }
        out
    }
}

/// `17 of 20 inside their p90 (85%)`, or why there is no share.
fn held(forecast: u32, inside: u32) -> String {
    if forecast == 0 {
        return "none had a forecast".into();
    }
    format!(
        "{inside} of {forecast} inside their p90 ({}%)",
        (f64::from(inside) * 100.0 / f64::from(forecast)).round() as u32
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fc() -> WorkForecast {
        WorkForecast {
            usd_p50: 0.60,
            usd_p90: 1.50,
            min_p50: 8.0,
            min_p90: 14.0,
            n: 23,
            past: false,
            read: None,
            read_p: None,
        }
    }

    /// The forecast's words are the ones the owner's example gives.
    #[test]
    fn the_forecast_reads_as_the_example_does() {
        assert_eq!(
            fc().words(),
            "forecast 8 min (p90 14) · $0.60 (p90 $1.50) · from 23 like it"
        );
        assert_eq!(fc().spent_of(0.42), "$0.42 of ~$0.60");
        assert_eq!(fc().spent_of(0.72), "$0.72, past its usual ~$0.60");
        assert_eq!(none_words(2), "no forecast yet: 2 like it");
    }

    /// The judge's read is worded with its confidence, and only when it ran.
    #[test]
    fn the_judges_read_is_worded_with_its_confidence() {
        let mut f = fc();
        assert_eq!(f.read_words(), None);
        f.read = Some("progressing".into());
        f.read_p = Some(0.91);
        assert_eq!(
            f.read_words().as_deref(),
            Some("still progressing (loop.v1 0.91)")
        );
        f.read = Some("thrashing".into());
        assert!(f
            .read_words()
            .unwrap()
            .starts_with("looks like it is thrashing"));
    }

    /// Calibration prints each class, and the total across several.
    #[test]
    fn the_calibration_lines_count_what_held() {
        let r = ForecastCalibrationResult {
            at_ms: 1,
            table: 40,
            ready: true,
            windows: vec![CalibrationWindow {
                days: 7,
                classes: vec![
                    CalibrationClass {
                        class: "open".into(),
                        finished: 12,
                        forecast: 10,
                        inside: 9,
                    },
                    CalibrationClass {
                        class: "specified".into(),
                        finished: 3,
                        forecast: 0,
                        inside: 0,
                    },
                ],
            }],
        };
        let lines = r.lines();
        assert_eq!(lines[0], "last 7 days:");
        assert!(
            lines[1].contains("9 of 10 inside their p90 (90%)"),
            "{lines:?}"
        );
        assert!(lines[2].contains("none had a forecast"), "{lines:?}");
        assert!(lines[3].contains("15 finished, 9 of 10"), "{lines:?}");
    }
}
