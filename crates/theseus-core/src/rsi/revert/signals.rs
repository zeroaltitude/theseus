//! The three signals auto-revert watches after a self-join (theseus-pw1q.10):
//! the gates that ran on main, the lifecycle and turn benches against their
//! budgets, and the daemon's error rate against the hour before the join.
//!
//! [`Signals`] is the seam: [`Files`] reads the machine's own records (the
//! gate's phase-times log, the bench history, the ledger), and a test hands
//! in its own. A [`Red`] names its signal, when it was seen, and the commit
//! of main it judged when the record names one (`gate-times.csv` and
//! `bench-history.csv` label a row `<branch> <git describe>`); attribution
//! (`attribute.rs`) decides whose red it is.

use std::path::PathBuf;

use anyhow::Result;
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use crate::rpc::Core;

/// One signal gone red.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Red {
    /// `gate`, `bench` or `health`.
    pub signal: &'static str,
    /// When it was seen (unix ms).
    pub at_ms: u64,
    /// The abbreviated commit of main the record judged, when it names one.
    pub commit: Option<String>,
    /// What the signal read, for the notice.
    pub detail: String,
}

/// What the watch reads. `since_ms` is the earliest watched join's time and
/// `now_ms` the pass's clock; a signal reports a red it saw after `since_ms`.
pub trait Signals: Send + Sync {
    fn reds(&self, core: &Core, since_ms: u64, now_ms: u64) -> Result<Vec<Red>>;
}

/// The machine's own records.
pub struct Files {
    /// The gate's phase-times log (`$THESEUS_GATE_TIMES`).
    pub gate_times: PathBuf,
    /// The bench history (`$THESEUS_BENCH_HISTORY`).
    pub bench_history: PathBuf,
}

impl Default for Files {
    fn default() -> Self {
        let cache = |var: &str, name: &str| {
            std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(".cache/theseus")
                    .join(name)
            })
        };
        Self {
            gate_times: cache("THESEUS_GATE_TIMES", "gate-times.csv"),
            bench_history: cache("THESEUS_BENCH_HISTORY", "bench-history.csv"),
        }
    }
}

impl Signals for Files {
    fn reds(&self, core: &Core, since_ms: u64, now_ms: u64) -> Result<Vec<Red>> {
        let mut out = gate_reds(&read(&self.gate_times), since_ms);
        out.extend(bench_reds(&read(&self.bench_history), since_ms));
        out.extend(health_red(core, since_ms, now_ms)?);
        Ok(out)
    }
}

fn read(p: &std::path::Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// A CSV line's fields; a quoted field may hold commas, and `""` is a quote.
pub fn fields(line: &str) -> Vec<String> {
    let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => out.push(std::mem::take(&mut cur)),
            (c, _) => cur.push(c),
        }
    }
    out.push(cur);
    out
}

/// The commit a label names: `main g1a2b3c` or `main v1-4-g1a2b3c-dirty`
/// (`git describe --always --dirty`) is `1a2b3c`; a label of another branch
/// names none, and so is no signal of main's.
pub fn main_commit(label: &str) -> Option<String> {
    let rest = label.strip_prefix("main ")?.trim();
    let rest = rest.strip_suffix("-dirty").unwrap_or(rest);
    let tail = rest.rsplit('-').next().unwrap_or(rest);
    let hex = tail.strip_prefix('g').unwrap_or(tail);
    (hex.len() >= 7 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_string())
}

/// The gates of main that ended red after `since_ms`: a `total` row of the
/// phase-times log (`time,label,phase,seconds,status`) whose status is not 0.
pub fn gate_reds(csv: &str, since_ms: u64) -> Vec<Red> {
    csv.lines()
        .filter_map(|l| {
            let f = fields(l);
            let [time, label, phase, _secs, status] = f.as_slice() else {
                return None;
            };
            let at_ms = crate::wake::parse_at(time).ok()?;
            let commit = main_commit(label)?;
            (phase == "total" && status != "0" && at_ms > since_ms).then(|| Red {
                signal: "gate",
                at_ms,
                commit: Some(commit),
                detail: format!("main's gate at {label} ended with status {status}"),
            })
        })
        .collect()
}

/// The benches of main that missed their budget after `since_ms`: a history
/// row whose `passed` is false and carries no busy allowance (a miss whose
/// rerun passed has a later row for the same label that did).
pub fn bench_reds(csv: &str, since_ms: u64) -> Vec<Red> {
    let mut lines = csv.lines();
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    let head = fields(head);
    let col = |n: &str| head.iter().position(|h| h == n);
    let (Some(t), Some(l), Some(p), a) =
        (col("time"), col("label"), col("passed"), col("allowance"))
    else {
        return Vec::new();
    };
    // The last row of each label settles it.
    let mut last: Vec<(String, u64, bool)> = Vec::new();
    for line in lines {
        let f = fields(line);
        let (Some(time), Some(label), Some(passed)) = (f.get(t), f.get(l), f.get(p)) else {
            continue;
        };
        let Ok(at_ms) = crate::wake::parse_at(time) else {
            continue;
        };
        let allowed = a.and_then(|a| f.get(a)).is_some_and(|v| !v.is_empty());
        let red = passed == "false" && !allowed;
        match last.iter_mut().find(|(lab, _, _)| lab == label) {
            Some(slot) => *slot = (label.clone(), at_ms, red),
            None => last.push((label.clone(), at_ms, red)),
        }
    }
    last.into_iter()
        .filter(|(_, at_ms, red)| *red && *at_ms > since_ms)
        .filter_map(|(label, at_ms, _)| {
            Some(Red {
                signal: "bench",
                at_ms,
                commit: Some(main_commit(&label)?),
                detail: format!("a bench of main at {label} missed its budget"),
            })
        })
        .collect()
}

/// The ledger kinds that count as the daemon failing.
const ERRORS: [&str; 3] = ["turn.failed", "provider.error", "execution.failed"];
/// An hour with fewer errors than this is never red.
pub const MIN_ERRORS: u64 = 5;
/// An hour is red at this many times the hour before the first join.
pub const TIMES: u64 = 2;
const HOUR: u64 = 3_600_000;

/// Error rows between two times.
fn errors_between(core: &Core, since_ms: u64, until_ms: u64) -> Result<Option<u64>> {
    let tags: Vec<String> = ERRORS.iter().map(|k| ledger_kind(k)).collect();
    let page = Page {
        kind: kinds::LEDGER,
        tags,
        after: None,
        before: None,
        since_ms: Some(since_ms),
        until_ms: Some(until_ms),
        limit: 1,
    };
    Ok(core.store.ledger_page(&page)?.map(|p| p.count))
}

/// The error rate after the first join against the hour before it: the first
/// whole hour after `since_ms` with at least [`MIN_ERRORS`] errors and
/// [`TIMES`] times the hour before. Unread while the ledger's index is built.
fn health_red(core: &Core, since_ms: u64, now_ms: u64) -> Result<Option<Red>> {
    let Some(before) = errors_between(core, since_ms.saturating_sub(HOUR), since_ms)? else {
        return Ok(None);
    };
    let mut start = since_ms;
    while start + HOUR <= now_ms {
        let Some(n) = errors_between(core, start, start + HOUR)? else {
            return Ok(None);
        };
        if n >= MIN_ERRORS && n >= TIMES * before.max(1) {
            return Ok(Some(Red {
                signal: "health",
                at_ms: start,
                commit: None,
                detail: format!(
                    "{n} errors (failed turns, provider errors, failed executions) in the hour \
                     from the join's first hour, against {before} in the hour before it"
                ),
            }));
        }
        start += HOUR;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GATES: &str = "time,label,phase,seconds,status\n\
        2026-10-14T01:00:00+00:00,\"main 1.4-3-g1a2b3c4\",suite,90,ok\n\
        2026-10-14T01:02:00+00:00,\"main 1.4-3-g1a2b3c4\",total,120,1\n\
        2026-10-14T02:02:00+00:00,\"lane/x g9999999-dirty\",total,120,1\n\
        2026-10-14T03:02:00+00:00,\"main g5e6f7a8\",total,100,0\n";

    /// A red gate of main is a `total` row with a status; a green one, a lane's
    /// and one before the join are not.
    #[test]
    fn a_red_gate_of_main_after_the_join_is_a_red() {
        let t = crate::wake::parse_at("2026-10-14T00:00:00+00:00").unwrap();
        let reds = gate_reds(GATES, t);
        assert_eq!(reds.len(), 1, "{reds:?}");
        assert_eq!(reds[0].commit.as_deref(), Some("1a2b3c4"));
        assert_eq!(reds[0].signal, "gate");
        let later = crate::wake::parse_at("2026-10-14T01:30:00+00:00").unwrap();
        assert!(gate_reds(GATES, later).is_empty(), "the red was before it");
    }

    #[test]
    fn a_label_names_main_s_commit_and_no_other_branch_s() {
        assert_eq!(main_commit("main g1a2b3c4").as_deref(), Some("1a2b3c4"));
        assert_eq!(
            main_commit("main 1.4-3-g1a2b3c4-dirty").as_deref(),
            Some("1a2b3c4")
        );
        assert_eq!(main_commit("main 1a2b3c4d").as_deref(), Some("1a2b3c4d"));
        assert_eq!(main_commit("lane/x g1a2b3c4"), None);
        assert_eq!(main_commit("main v1"), None);
    }

    /// A bench miss whose rerun passed, and one the busy allowance carried, are
    /// not red; the last row of a label settles it.
    #[test]
    fn a_bench_is_red_only_when_its_last_row_missed_without_an_allowance() {
        let csv = "time,label,load1,passed,allowance\n\
            2026-10-14T01:00:00Z,main g1111111,0.5,false,\n\
            2026-10-14T01:01:00Z,main g1111111,0.5,true,\n\
            2026-10-14T02:00:00Z,main g2222222,0.5,false,\n\
            2026-10-14T03:00:00Z,main g3333333,0.5,false,65\n\
            2026-10-14T04:00:00Z,lane g4444444,0.5,false,\n";
        let reds = bench_reds(csv, 0);
        assert_eq!(reds.len(), 1, "{reds:?}");
        assert_eq!(reds[0].commit.as_deref(), Some("2222222"));
        assert_eq!(reds[0].signal, "bench");
    }

    #[test]
    fn a_csv_line_keeps_a_quoted_comma_and_a_doubled_quote() {
        assert_eq!(fields("a,\"b,c\",\"d\"\"e\",f"), ["a", "b,c", "d\"e", "f"]);
    }
}
