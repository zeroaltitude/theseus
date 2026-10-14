//! The binary's growth rule and the daemon's soft memory line (theseus-2m7jg).
//!
//! §9's 60 MB for `theseusd` is a target the release build has missed since it
//! grew, and a line that is only ever missed stops anyone looking. So
//! `bench size --check` fails on **growth**: a binary more than
//! [`GROWTH_MB`] over the last recorded size of the same build profile. The 60
//! MB miss is printed with its gap and fails nothing. Memory has no hard line:
//! `bench turn --check` prints a WARN when the burst's resident memory is more
//! than [`RSS_WARN_PERCENT`] over the median of its last recorded runs, and
//! records that in the history; it never fails.

use crate::history::{History, Row};
use crate::lifecycle::Verdict;

/// A release binary may be this many MB larger than the last one recorded.
pub const GROWTH_MB: f64 = 1.0;

/// The burst's memory is warned about when it is more than this share over the
/// median of the recorded runs.
pub const RSS_WARN_PERCENT: f64 = 10.0;

/// How many recorded runs the median is taken over.
pub const RSS_WINDOW: usize = 5;

/// The history column a warning is recorded in: the percentage over the median.
pub const RSS_OVER_COLUMN: &str = "rss_burst_over";

/// A size of `column` (a `size_*` column) recorded by a run that passed. A run
/// that failed on growth is not the baseline: the next build would otherwise
/// compare itself to the growth it was refused for. `tag`, when given, must be
/// a word of the row's label (`size:release` is not `size:release-thin`), so a thin build is not judged against a fat one.
pub fn last_size(history: &History, column: &str, tag: Option<&str>) -> Option<f64> {
    history
        .rows
        .iter()
        .rev()
        .filter(|r| r.passed && tag.is_none_or(|t| r.label.split_whitespace().any(|w| w == t)))
        .find_map(|r| r.phases.iter().find(|p| p.phase == column).map(|p| p.p95))
}

/// The growth verdict for a binary of `mb` against the last recorded size
/// `last`: its limit is that size plus [`GROWTH_MB`].
pub fn growth_verdict(column: &str, mb: f64, last: f64) -> Verdict {
    let limit = last + GROWTH_MB;
    Verdict {
        phase: column.to_string(),
        p95: mb,
        budget: limit,
        margin: 0.0,
        ok: mb <= limit,
    }
}

/// Lines on the 60 MB target: a miss with its gap, a hit with its headroom.
/// Printed, never judged.
pub fn target_line(column: &str, mb: f64, target: f64) -> String {
    if mb > target {
        format!(
            "  {column}: {mb:.2} MB is {:.2} MB over §9's {target} MB target: a miss, not a failure (only growth fails)",
            mb - target
        )
    } else {
        format!(
            "  {column}: {mb:.2} MB is {:.2} MB under §9's {target} MB target",
            target - mb
        )
    }
}

/// One line on a verdict of growth.
pub fn growth_line(v: &Verdict, last: f64) -> String {
    format!(
        "  {}: {:.2} MB against the last recorded {last:.2} MB: {:+.2} MB (limit +{GROWTH_MB} MB): {}",
        v.phase,
        v.p95,
        v.p95 - last,
        if v.ok { "ok" } else { "GREW: FAILED" }
    )
}

/// The median of the last [`RSS_WINDOW`] recorded `rss_burst` readings, `None`
/// when the history has none.
pub fn rss_burst_median(history: &History) -> Option<f64> {
    let mut last: Vec<f64> = history
        .rows
        .iter()
        .rev()
        .filter_map(|r: &Row| r.phases.iter().find(|p| p.phase == "rss_burst"))
        .map(|p| p.p95)
        .take(RSS_WINDOW)
        .collect();
    if last.is_empty() {
        return None;
    }
    last.sort_by(|a, b| a.total_cmp(b));
    let n = last.len();
    Some(if n % 2 == 1 {
        last[n / 2]
    } else {
        (last[n / 2 - 1] + last[n / 2]) / 2.0
    })
}

/// A burst whose memory is over the line.
#[derive(Debug, PartialEq)]
pub struct RssWarning {
    pub burst_mb: f64,
    pub median_mb: f64,
    pub over_percent: f64,
}

impl RssWarning {
    pub fn line(&self) -> String {
        format!(
            "WARN rss_burst {:.1} MB is {:.1}% over the median {:.1} MB of its last {RSS_WINDOW} recorded runs (soft line: {RSS_WARN_PERCENT}%; never fails)",
            self.burst_mb, self.over_percent, self.median_mb
        )
    }
}

/// `Some` when `burst_mb` is more than [`RSS_WARN_PERCENT`] over the median of
/// the history's last runs; `None` when it is not, or there is no history.
pub fn rss_warning(burst_mb: f64, history: Option<&History>) -> Option<RssWarning> {
    let median = rss_burst_median(history?)?;
    let over = (burst_mb / median - 1.0) * 100.0;
    (median > 0.0 && over > RSS_WARN_PERCENT).then_some(RssWarning {
        burst_mb,
        median_mb: median,
        over_percent: over,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::PhaseRow;

    fn row(label: &str, passed: bool, cols: &[(&str, f64)]) -> Row {
        Row {
            time: "2026-10-12T10:00:00+00:00".to_string(),
            label: label.to_string(),
            load1: None,
            phases: cols
                .iter()
                .map(|(n, v)| PhaseRow {
                    phase: n.to_string(),
                    p50: *v,
                    p95: *v,
                    limit: None,
                })
                .collect(),
            passed,
            allowance: None,
        }
    }

    fn history(rows: Vec<Row>) -> History {
        History {
            rows,
            skipped: Vec::new(),
        }
    }

    #[test]
    fn growth_fails_at_one_and_a_half_megabytes_and_passes_at_half_one() {
        let h = history(vec![row(
            "main size:release",
            true,
            &[("size_theseusd", 60.0)],
        )]);
        let last = last_size(&h, "size_theseusd", Some("size:release")).unwrap();
        assert!(!growth_verdict("size_theseusd", 61.5, last).ok);
        assert!(growth_verdict("size_theseusd", 60.5, last).ok);
        assert!(
            growth_verdict("size_theseusd", 61.0, last).ok,
            "exactly 1 MB passes"
        );
        assert!(
            growth_verdict("size_theseusd", 50.0, last).ok,
            "a smaller one passes"
        );
    }

    #[test]
    fn the_baseline_is_the_last_passed_run_of_the_same_profile() {
        let h = history(vec![
            row("a size:release", true, &[("size_theseusd", 70.0)]),
            row("b size:release-thin", true, &[("size_theseusd", 50.0)]),
            row("c size:release", false, &[("size_theseusd", 75.0)]),
            row("d size:release", true, &[("turn", 1.0)]),
        ]);
        assert_eq!(
            last_size(&h, "size_theseusd", Some("size:release")),
            Some(70.0)
        );
        assert_eq!(
            last_size(&h, "size_theseusd", Some("size:release-thin")),
            Some(50.0)
        );
        assert_eq!(last_size(&h, "size_theseusd", Some("size:install")), None);
        assert_eq!(last_size(&h, "size_theseusd", None), Some(50.0));
    }

    #[test]
    fn the_sixty_megabyte_miss_prints_its_gap_and_is_no_verdict() {
        let line = target_line("size_theseusd", 77.85, 60.0);
        assert!(
            line.contains("17.85 MB over") && line.contains("not a failure"),
            "{line}"
        );
        assert!(target_line("size_theseusd", 58.0, 60.0).contains("2.00 MB under"));
    }

    fn burst_history(values: &[f64]) -> History {
        history(
            values
                .iter()
                .map(|v| row("x", true, &[("rss_burst", *v)]))
                .collect(),
        )
    }

    #[test]
    fn the_rss_warning_fires_at_eleven_percent_and_is_silent_at_nine() {
        let h = burst_history(&[100.0, 100.0, 100.0]);
        let w = rss_warning(111.0, Some(&h)).expect("+11% warns");
        assert_eq!((w.burst_mb, w.median_mb), (111.0, 100.0));
        assert!(
            w.line().contains("111.0") && w.line().contains("100.0"),
            "{}",
            w.line()
        );
        assert_eq!(rss_warning(109.0, Some(&h)), None);
        assert_eq!(rss_warning(90.0, Some(&h)), None);
        assert_eq!(rss_warning(150.0, None), None, "no history, no line");
        assert_eq!(rss_warning(150.0, Some(&history(Vec::new()))), None);
    }

    #[test]
    fn the_median_is_over_the_last_five_runs_only() {
        let h = burst_history(&[500.0, 500.0, 100.0, 100.0, 100.0, 120.0, 90.0]);
        // The last five: 100, 100, 100, 120, 90.
        assert_eq!(rss_burst_median(&h), Some(100.0));
        assert_eq!(
            rss_burst_median(&burst_history(&[100.0, 110.0])),
            Some(105.0)
        );
    }
}
