//! The lifecycle bench's history (theseus-1hk): one CSV row per bench run,
//! kept outside the tree, so a phase drifting toward its limit shows before
//! the gate fails on it.
//!
//! `bench lifecycle --record FILE --label TEXT` appends the row: the time, the
//! label (the gate's is the branch and `git describe --always --dirty`), the
//! 1-minute load average, each phase's p50, p95, and limit (its budget plus
//! its margin, what the gate judges; empty for restore, which has no budget
//! yet, and all three empty for a phase the run skipped), and whether the run
//! passed. The gate reruns the bench once after a miss and records both runs,
//! so a miss shows even when the rerun passes. `bench history` reads the file
//! back, with each phase's headroom.
//!
//! `bench turn`, `bench idle`, and `bench size` append rows of their own to the
//! same file (theseus-goa8): the columns after the lifecycle's phases (`OTHER`),
//! each with its unit, empty in a row of another bench.
//!
//! The busy allowance (theseus-lew7): when the gate's settle step finds no
//! quiet window, it gives the timing benches `--allowance PERCENT`, and a
//! phase over its limit by no more than that share of it passes. `passed`
//! stays the strict verdict, and the last column, `allowance`, holds the
//! percentage a run passed on (empty for every other run), so a trend of runs
//! that passed only on it shows: `bench history` counts them, and marks each
//! phase it carried "busy: allowance +N% applied (measured X ms, limit Y ms)".
//! A count, such as a turn's frames, never gets an allowance.
//!
//! The file is `$THESEUS_BENCH_HISTORY`, or `~/.cache/theseus/bench-history.csv`,
//! outside the tree: a tracked file the gate rewrote would dirty every commit.
//! Every worktree shares it, and the gate lock serializes its writers. A row
//! is appended with one `write` (`O_APPEND`), so a row is whole or torn, never
//! mixed with another; a torn last line is skipped, and said so.

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::lifecycle::{Summary, Verdict, PHASES};

/// A passing run warns about each phase whose p95 is within this share of its
/// limit (the warning changes no exit status).
const NEAR_PERCENT: i64 = 10;

/// The columns of the benches besides the lifecycle's, in order, each with its
/// unit (theseus-goa8). Each is read as a phase is: a p50, a p95, and a limit
/// when the bench has a budget for it. A single reading (a size, a memory) is
/// its own p50 and p95.
pub const OTHER: [(&str, &str); 18] = [
    // `bench turn`
    ("turn_plain", "ms"),
    ("frames_plain", "frames"),
    ("turn_tool", "ms"),
    ("frames_tool", "frames"),
    ("rss_start", "MB"),
    ("rss_burst", "MB"),
    // `bench turn --session-nodes` (step 33)
    ("turn_long", "ms"),
    ("decodes_long", "nodes"),
    ("rss_long", "MB"),
    // `bench idle`
    ("idle_cpu", "ms"),
    ("idle_wakeups", "per s"),
    ("idle_frames", "frames"),
    ("rss_idle", "MB"),
    ("rss_tender", "MB"),
    ("rss_active", "MB"),
    // `bench size`
    ("size_theseusd", "MB"),
    ("size_theseus", "MB"),
    ("size_theseus_tui", "MB"),
];

/// Every column the history carries: the lifecycle's phases, then the other
/// benches'.
pub fn columns() -> impl Iterator<Item = &'static str> {
    PHASES.iter().copied().chain(OTHER.iter().map(|(n, _)| *n))
}

/// A column's unit: `ms` for the lifecycle's phases.
pub fn unit(column: &str) -> &'static str {
    OTHER
        .iter()
        .find(|(n, _)| *n == column)
        .map_or("ms", |(_, u)| *u)
}

/// `~/.cache/theseus/bench-history.csv`: where the history is when
/// `$THESEUS_BENCH_HISTORY` doesn't say.
pub fn default_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set: name the history's file")?;
    Ok(PathBuf::from(home).join(".cache/theseus/bench-history.csv"))
}

/// One bench run.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Local time with its offset, `2026-10-01T10:20:11-07:00`.
    pub time: String,
    pub label: String,
    /// The 1-minute load average as the run ended.
    pub load1: Option<f64>,
    /// The phases the run measured, in the order of the file's columns.
    pub phases: Vec<PhaseRow>,
    /// The strict verdict: every budget met within its limit.
    pub passed: bool,
    /// The busy allowance the run passed on, a percentage of each limit: set
    /// only on a run that missed strictly (`passed` is false) and passed with
    /// the allowance the gate gave it, the machine having never settled.
    pub allowance: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhaseRow {
    pub phase: String,
    pub p50: f64,
    pub p95: f64,
    /// The budget plus the margin; `None` for a phase with no budget.
    pub limit: Option<f64>,
}

/// To the hundredth of a ms (10 µs), so a value prints short and reads back
/// exactly.
fn round2(ms: f64) -> f64 {
    (ms * 100.0).round() / 100.0
}

impl Row {
    /// The row for a run's phases and verdicts (`lifecycle::Report`'s).
    pub fn of(
        phases: &[(String, Summary)],
        verdicts: &[Verdict],
        passed: bool,
        label: &str,
        load1: Option<f64>,
        time: String,
    ) -> Self {
        let phases = phases
            .iter()
            .map(|(name, s)| PhaseRow {
                phase: name.clone(),
                p50: round2(s.p50),
                p95: round2(s.p95),
                limit: verdicts
                    .iter()
                    .find(|v| &v.phase == name)
                    .map(|v| round2(v.budget + v.margin)),
            })
            .collect();
        Self {
            time,
            label: label.to_string(),
            load1: load1.map(round2),
            phases,
            passed,
            allowance: None,
        }
    }

    /// The row as one CSV line under `header()`, without its newline.
    pub fn to_csv(&self) -> String {
        let num = |x: Option<f64>| x.map_or(String::new(), |x| x.to_string());
        let mut f = vec![field(&self.time), field(&self.label), num(self.load1)];
        for name in columns() {
            match self.phases.iter().find(|p| p.phase == name) {
                Some(p) => f.extend([p.p50.to_string(), p.p95.to_string(), num(p.limit)]),
                None => f.extend([String::new(), String::new(), String::new()]),
            }
        }
        f.push(self.passed.to_string());
        f.push(self.allowance.map_or(String::new(), |a| a.to_string()));
        f.join(",")
    }
}

/// The history's header: the time, the label, the load, three columns per
/// phase (`cold_p50`, `cold_p95`, `cold_limit`, …; the other benches' too),
/// `passed`, and `allowance`.
pub fn header() -> String {
    let mut h = vec!["time".to_string(), "label".to_string(), "load1".to_string()];
    for name in columns() {
        h.extend([
            format!("{name}_p50"),
            format!("{name}_p95"),
            format!("{name}_limit"),
        ]);
    }
    h.push("passed".to_string());
    h.push("allowance".to_string());
    h.join(",")
}

/// A field as CSV writes it. A label is one line, so control characters
/// (newlines among them) become spaces; a comma or a quote is quoted.
fn field(s: &str) -> String {
    let s: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if s.contains([',', '"']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s
    }
}

/// A CSV line's fields: quoted fields may hold commas, and `""` is a quote.
fn split(line: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut start) = (false, true);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (quoted, c) {
            (false, ',') => {
                fields.push(std::mem::take(&mut cur));
                start = true;
                continue;
            }
            (false, '"') if start => quoted = true,
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
                cur.push('"');
            }
            (true, '"') => quoted = false,
            _ => cur.push(c),
        }
        start = false;
    }
    if quoted {
        return Err("a quote is never closed".to_string());
    }
    fields.push(cur);
    Ok(fields)
}

/// One data line, read by the column names of the header above it, so a file
/// whose phases changed reads under each of its headers.
pub fn parse(header: &[String], line: &str) -> Result<Row, String> {
    let f = split(line)?;
    if f.len() != header.len() {
        return Err(format!(
            "{} fields where the header has {}",
            f.len(),
            header.len()
        ));
    }
    let col = |name: &str| header.iter().position(|h| h == name).map(|i| f[i].as_str());
    let num = |name: &str| -> Result<Option<f64>, String> {
        match col(name) {
            None | Some("") => Ok(None),
            Some(s) => s
                .parse()
                .map(Some)
                .map_err(|_| format!("{name} is {s:?}, not a number")),
        }
    };
    let mut phases = Vec::new();
    for name in header.iter().filter_map(|h| h.strip_suffix("_p50")) {
        match (num(&format!("{name}_p50"))?, num(&format!("{name}_p95"))?) {
            (Some(p50), Some(p95)) => phases.push(PhaseRow {
                phase: name.to_string(),
                p50,
                p95,
                limit: num(&format!("{name}_limit"))?,
            }),
            (None, None) => {}
            _ => return Err(format!("{name} has a p50 or a p95, not both")),
        }
    }
    Ok(Row {
        time: col("time").ok_or("no time column")?.to_string(),
        label: col("label").unwrap_or_default().to_string(),
        load1: num("load1")?,
        phases,
        passed: match col("passed") {
            Some("true") => true,
            Some("false") => false,
            other => return Err(format!("passed is {other:?}, not true or false")),
        },
        // Absent under a header from before theseus-lew7.
        allowance: match col("allowance") {
            None | Some("") => None,
            Some(s) => Some(
                s.parse()
                    .map_err(|_| format!("allowance is {s:?}, not a whole percentage"))?,
            ),
        },
    })
}

/// A history file, read.
#[derive(Debug, Default)]
pub struct History {
    pub rows: Vec<Row>,
    /// The lines skipped, each with its number and why: a torn last line (a
    /// write cut short), or a line that doesn't parse.
    pub skipped: Vec<String>,
}

/// The history's rows. A header line (`time,…`) may come again later, when
/// the bench's phases changed; each row reads under the last one above it.
pub fn parse_file(text: &str) -> History {
    let mut h = History::default();
    let mut header: Option<Vec<String>> = None;
    // Every row ends with its newline, written in the same `write`: a last
    // line without one was cut short, even if what is left still parses.
    let torn = !text.is_empty() && !text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        if torn && n == lines.len() {
            h.skipped.push(format!(
                "line {n}: the last line is torn (a write cut short)"
            ));
            break;
        }
        if line.is_empty() {
            continue;
        }
        if line.starts_with("time,") {
            match split(line) {
                Ok(cols) => header = Some(cols),
                Err(why) => h.skipped.push(format!("line {n}: a header, but {why}")),
            }
            continue;
        }
        let Some(cols) = &header else {
            h.skipped.push(format!("line {n}: a row before any header"));
            continue;
        };
        match parse(cols, line) {
            Ok(row) => h.rows.push(row),
            Err(why) => h.skipped.push(format!("line {n}: {why}")),
        }
    }
    h
}

/// The history at `path`, or `None` when there is no file yet.
pub fn read(path: &Path) -> Result<Option<History>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(parse_file(&String::from_utf8_lossy(&bytes)))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

/// Append `row` to the history at `path` in one `write`, creating the file
/// (and its directory) with the header when it is new. The header is written
/// again when the file's last one differs (the bench's phases changed), and a
/// torn last line is ended first, so this row is whole.
pub fn append(path: &Path, row: &Row) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let mut f = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let mut old = Vec::new();
    f.read_to_end(&mut old)?;
    let old = String::from_utf8_lossy(&old);
    let header = header();
    let mut out = String::new();
    if !old.is_empty() && !old.ends_with('\n') {
        out.push('\n');
    }
    if old.lines().rfind(|l| l.starts_with("time,")) != Some(header.as_str()) {
        out.push_str(&header);
        out.push('\n');
    }
    out.push_str(&row.to_csv());
    out.push('\n');
    let n = f
        .write(out.as_bytes())
        .with_context(|| format!("appending to {}", path.display()))?;
    if n != out.len() {
        bail!(
            "appended {n} of {} bytes to {}: the row is torn",
            out.len(),
            path.display()
        );
    }
    Ok(())
}

/// The 1-minute load average (`/proc/loadavg`).
pub fn load1() -> Option<f64> {
    std::fs::read_to_string("/proc/loadavg")
        .ok()?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Now, in local time with its offset: `2026-10-01T10:20:11-07:00`.
pub fn now() -> String {
    let t = theseus_core::wake::local(theseus_protocol::now_unix_ms());
    let sign = if t.offset_secs < 0 { '-' } else { '+' };
    let off = t.offset_secs.unsigned_abs();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
        t.year,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
        off / 3600,
        off % 3600 / 60
    )
}

/// Whether `p95` is at or under `limit` with at most 10% of the limit left.
/// In whole microseconds, so the edge is exact: 51.3 ms against 57 is in.
pub fn near(p95: f64, limit: f64) -> bool {
    let us = |ms: f64| (ms * 1000.0).round() as i64;
    let (p, l) = (us(p95), us(limit));
    p <= l && (l - p) * 100 <= l * NEAR_PERCENT
}

/// The busy allowance (theseus-lew7): whether `p95` is at or under `limit`
/// plus `pct` % of it. In whole microseconds, as [`near`] is, so the edge is
/// exact: 94.18 ms against 57.08 plus 65% is in.
pub fn within(p95: f64, limit: f64, pct: u32) -> bool {
    let us = |ms: f64| (ms * 1000.0).round() as i64;
    us(p95) * 100 <= us(limit) * (100 + i64::from(pct))
}

/// Whether a verdict passes with the busy allowance `pct`: it met its limit,
/// or it is a time over its limit by no more than the allowance. A count (a
/// turn's frames) is held to its budget exactly, whatever the allowance, and
/// an allowance of 0 is the strict verdict.
pub fn allowed(v: &Verdict, pct: u32) -> bool {
    v.ok || (pct > 0 && unit(&v.phase) == "ms" && within(v.p95, v.budget + v.margin, pct))
}

/// What a phase the allowance carried says, in the gate's log and in `bench
/// history`: `busy: allowance +65% applied (measured 112.0 ms, limit 104 ms)`.
fn applied(pct: u32, what: &str, p95: f64, limit: f64) -> String {
    format!("busy: allowance +{pct}% applied{what} (measured {p95:.1} ms, limit {limit} ms)")
}

/// For a run that missed strictly: one line for each verdict the allowance
/// `pct` carried, such as `lifecycle: busy: allowance +65% applied to clean
/// shutdown (measured 112.0 ms, limit 104 ms)`.
pub fn allowance_applied(bench: &str, verdicts: &[Verdict], pct: u32) -> Vec<String> {
    verdicts
        .iter()
        .filter(|v| !v.ok && allowed(v, pct))
        .map(|v| {
            let what = format!(" to {}", name(&v.phase));
            format!(
                "{bench}: {}",
                applied(pct, &what, v.p95, round2(v.budget + v.margin))
            )
        })
        .collect()
}

/// A phase's name in a sentence.
fn name(phase: &str) -> &str {
    match phase {
        "l1_start" => "an L1 job's start",
        "cold" => "cold start",
        "vault" => "cold start from the config copy",
        "shutdown" => "clean shutdown",
        "kill" => "restart after SIGKILL",
        "swap" => "binary swap",
        "restore" => "restore from a local WAL",
        "turn_plain" => "a plain turn's wall time",
        "frames_plain" => "a plain turn's frames",
        "turn_tool" => "a tool-call turn's wall time",
        "frames_tool" => "a tool-call turn's frames",
        "rss_start" => "resident memory after the start",
        "rss_burst" => "resident memory after a burst of turns",
        "idle_cpu" => "an idle daemon's CPU time over the window",
        "idle_wakeups" => "an idle daemon's wakeups",
        "idle_frames" => "frames an idle daemon wrote",
        "rss_idle" => "resident memory at idle",
        "turn_long" => "a turn's wall time in a long session",
        "decodes_long" => "nodes a turn in a long session decoded",
        "rss_long" => "resident memory after turns in a long session, the tender's included",
        "rss_tender" => "the index tender's resident memory at idle",
        "rss_active" => "resident memory after turns in active sessions, the tender's included",
        "size_theseusd" => "theseusd's size",
        "size_theseus" => "theseus's size",
        "size_theseus_tui" => "theseus-tui's size",
        other => other,
    }
}

/// For a passing run: one line for each phase whose p95 is within 10% of its
/// limit, such as `lifecycle: cold start's p95 51.3 ms is within 10% of its
/// 57 ms limit`.
pub fn near_limits(verdicts: &[Verdict]) -> Vec<String> {
    near_limits_of("lifecycle", verdicts)
}

/// [`near_limits`] for the bench `bench`. A count of frames is held at its
/// budget exactly, so being at it is no drift and never warns.
pub fn near_limits_of(bench: &str, verdicts: &[Verdict]) -> Vec<String> {
    verdicts
        .iter()
        .filter(|v| warns_near(&v.phase) && near(v.p95, v.budget + v.margin))
        .map(|v| {
            let unit = unit(&v.phase);
            format!(
                "{bench}: {}'s p95 {:.1} {unit} is within {NEAR_PERCENT}% of its {} {unit} limit",
                name(&v.phase),
                v.p95,
                round2(v.budget + v.margin)
            )
        })
        .collect()
}

/// Whether a column's nearness to its limit is drift worth a word: not a
/// count of frames, which a budget holds exactly.
fn warns_near(column: &str) -> bool {
    unit(column) != "frames"
}

/// Append a bench's row to the history at `path`, and say where. A failure to
/// write it is reported and changes nothing else: the gate's verdict comes
/// from the run, not from its record.
pub fn record(
    path: &Path,
    bench: &str,
    label: &str,
    phases: &[(String, Summary)],
    verdicts: &[Verdict],
    passed: bool,
) {
    let row = Row::of(phases, verdicts, passed, label, load1(), now());
    match append(path, &row) {
        Ok(()) => println!("{bench}: recorded in {} as {label:?}", path.display()),
        Err(e) => eprintln!("{bench}: the run was NOT recorded: {e:#}"),
    }
}

/// One phase of one run, as `bench history` shows it: its limit and headroom,
/// what it did against the limit, and what its run did.
struct Marks {
    limit: String,
    headroom: String,
    mark: String,
    run: &'static str,
}

/// A phase over its limit is `MISSED`, or carried by the run's busy
/// allowance; one within 10% of it says so when `warn`; and a phase of a run
/// that missed, or passed only on the allowance, says that.
fn marks(r: &Row, p: &PhaseRow, warn: bool) -> Marks {
    let over = p.limit.is_some_and(|l| p.p95 > l);
    let run = match (r.passed || over, r.allowance) {
        (true, _) => "",
        (false, Some(_)) => "  (the run passed on the busy allowance)",
        (false, None) => "  (the run missed)",
    };
    let Some(l) = p.limit else {
        return Marks {
            limit: "-".to_string(),
            headroom: "-".to_string(),
            mark: String::new(),
            run,
        };
    };
    let mark = match r.allowance {
        Some(a) if over && within(p.p95, l, a) => format!("  {}", applied(a, "", p.p95, l)),
        _ if over => "  MISSED".to_string(),
        _ if warn && near(p.p95, l) => "  within 10%".to_string(),
        _ => String::new(),
    };
    Marks {
        limit: l.to_string(),
        headroom: format!("{:.1}", l - p.p95),
        mark,
        run,
    }
}

/// `bench history`: the last `last` runs of each phase, with the headroom
/// left (the limit minus the p95).
pub fn render(path: &Path, history: Option<&History>, last: usize) -> String {
    let mut o = String::new();
    let Some(h) = history else {
        let _ = writeln!(
            o,
            "bench history · {}: no history yet (the gate records each run with `bench lifecycle --record`)",
            path.display()
        );
        return o;
    };
    let missed = h.rows.iter().filter(|r| !r.passed).count();
    let carried = h.rows.iter().filter(|r| r.allowance.is_some()).count();
    let _ = writeln!(
        o,
        "bench history · {} · {} run(s), {missed} missed{}{}",
        path.display(),
        h.rows.len(),
        if carried > 0 {
            format!(" ({carried} of them passed on the busy allowance)")
        } else {
            String::new()
        },
        if h.rows.is_empty() {
            String::new()
        } else {
            format!(" · the last {last} of each phase")
        }
    );
    for s in &h.skipped {
        let _ = writeln!(o, "  skipped {s}");
    }
    if h.rows.is_empty() {
        let _ = writeln!(o, "  no runs recorded yet");
        return o;
    }
    // The columns in the benches' order, then any only an older header had.
    let mut phases: Vec<&str> = columns().collect();
    for r in &h.rows {
        for p in &r.phases {
            if !phases.contains(&p.phase.as_str()) {
                phases.push(p.phase.as_str());
            }
        }
    }
    for phase in phases {
        let rows: Vec<(&Row, &PhaseRow)> = h
            .rows
            .iter()
            .filter_map(|r| r.phases.iter().find(|p| p.phase == phase).map(|p| (r, p)))
            .collect();
        if rows.is_empty() {
            continue;
        }
        let shown = &rows[rows.len().saturating_sub(last)..];
        let width = shown
            .iter()
            .map(|(r, _)| r.label.chars().count())
            .max()
            .unwrap_or(0)
            .max(5);
        let u = unit(phase);
        let (h50, h95) = (format!("p50 {u}"), format!("p95 {u}"));
        let w = h50.chars().count().max(7);
        let _ = writeln!(o, "{} ({phase})", name(phase));
        let _ = writeln!(
            o,
            "  {:<25}  {:<width$}  {:>5}  {h50:>w$}  {h95:>w$}  {:>6}  {:>8}",
            "time", "label", "load", "limit", "headroom"
        );
        for (r, p) in shown {
            let load = r.load1.map_or("-".to_string(), |l| format!("{l:.2}"));
            let Marks {
                limit,
                headroom,
                mark,
                run,
            } = marks(r, p, warns_near(phase));
            let _ = writeln!(
                o,
                "  {:<25}  {:<width$}  {load:>5}  {:>w$.1}  {:>w$.1}  {limit:>6}  {headroom:>8}{mark}{run}",
                r.time, r.label, p.p50, p.p95
            );
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::verdicts;

    fn summary(p50: f64, p95: f64) -> Summary {
        Summary {
            n: 10,
            p50,
            p95,
            min: p50 - 1.0,
            max: p95,
        }
    }

    /// A gate run's row: every phase, as `main`'s last gate measured them
    /// (128b3f6), with its limits.
    fn row(label: &str, cold_p95: f64) -> Row {
        let phases: Vec<(String, Summary)> = [
            ("cold", summary(23.8, cold_p95)),
            ("vault", summary(23.8, 35.3)),
            ("shutdown", summary(41.6, 51.9)),
            ("kill", summary(42.4, 49.9)),
            ("swap", summary(58.2, 71.0)),
            ("restore", summary(115.9, 125.2)),
            ("seed", summary(1.2, 1.9)),
            ("cancel", summary(41.1, 43.2)),
        ]
        .into_iter()
        .map(|(n, s)| (n.to_string(), s))
        .collect();
        let v = verdicts(&phases, 0, None);
        let passed = v.iter().all(|v| v.ok);
        Row::of(
            &phases,
            &v,
            passed,
            label,
            Some(3.25),
            "2026-10-01T10:20:11-07:00".to_string(),
        )
    }

    #[test]
    fn a_row_reads_back_as_written() {
        let r = row("lane/fastgate ecfc574-dirty", 33.5);
        assert_eq!(
            r.phases.iter().map(|p| p.limit).collect::<Vec<_>>(),
            [
                Some(57.0),
                Some(57.0),
                Some(104.0),
                Some(175.0),
                Some(202.0),
                None,
                None,
                Some(250.0)
            ],
            "each limit is the budget plus the margin; restore and the seed have none"
        );
        let line = r.to_csv();
        assert_eq!(
            line,
            format!(
                "2026-10-01T10:20:11-07:00,lane/fastgate ecfc574-dirty,3.25,\
             23.8,33.5,57,23.8,35.3,57,41.6,51.9,104,,,,42.4,49.9,175,58.2,71,202,115.9,125.2,,1.2,1.9,,\
             41.1,43.2,250{},true,",
                // The other benches' columns, empty: three cells for each.
                // The last cell, the allowance, is empty: the run passed strictly.
                ",".repeat(3 * OTHER.len())
            )
        );
        let cols = split(&header()).unwrap();
        assert_eq!(cols.len(), 3 + 3 * columns().count() + 2);
        assert_eq!(parse(&cols, &line).unwrap(), r);

        // A label with a comma, a quote, and a newline: one line, and back.
        let mut odd = r.clone();
        odd.label = "lane/x \"y\", z\nw".to_string();
        let line = odd.to_csv();
        assert!(!line.contains('\n'));
        assert_eq!(parse(&cols, &line).unwrap().label, "lane/x \"y\", z w");

        // A run of some phases: the others' cells are empty, and stay out.
        let mut some = r;
        some.phases.retain(|p| p.phase == "cold");
        assert_eq!(parse(&cols, &some.to_csv()).unwrap(), some);

        // Values round to 10 µs, so they print short.
        let p = Row::of(
            &[("cold".to_string(), summary(23.456_789, 33.333_333))],
            &[],
            true,
            "",
            None,
            String::new(),
        );
        assert_eq!((p.phases[0].p50, p.phases[0].p95), (23.46, 33.33));
        assert_eq!(p.phases[0].limit, None);
    }

    #[test]
    fn a_miss_and_its_rerun_are_two_rows_under_one_header() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("cache/theseus/bench-history.csv");
        let miss = row("main 128b3f6", 58.1);
        let rerun = row("main 128b3f6", 33.5);
        assert!(!miss.passed && rerun.passed);
        append(&path, &miss).unwrap();
        append(&path, &rerun).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.lines().filter(|l| l.starts_with("time,")).count(),
            1,
            "the header is written once:\n{text}"
        );
        assert_eq!(text.lines().count(), 3);
        let h = read(&path).unwrap().unwrap();
        assert!(h.skipped.is_empty(), "{:?}", h.skipped);
        assert_eq!(h.rows, [miss, rerun]);
    }

    #[test]
    fn an_older_header_reads_and_a_new_one_is_written_after_it() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("h.csv");
        // A bench before the vault phase: its own columns, one row.
        std::fs::write(
            &path,
            "time,label,load1,cold_p50,cold_p95,cold_limit,passed\n\
             2026-09-30T09:49:00-07:00,main 4c6b72c,1.5,24,56.7,57,true\n",
        )
        .unwrap();
        append(&path, &row("main ecfc574", 33.5)).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("time,")).count(), 2);
        let h = read(&path).unwrap().unwrap();
        assert!(h.skipped.is_empty(), "{:?}", h.skipped);
        assert_eq!(h.rows.len(), 2);
        assert_eq!(h.rows[0].phases.len(), 1);
        assert_eq!(h.rows[0].phases[0].p95, 56.7);
        assert_eq!(
            h.rows[0].allowance, None,
            "no allowance column, no allowance"
        );
        // Every phase the row measured: all but `inflight` (theseus-ndw).
        assert_eq!(h.rows[1].phases.len(), PHASES.len() - 1);
    }

    #[test]
    fn a_torn_row_is_ended_before_the_next_and_skipped_when_read() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("h.csv");
        append(&path, &row("main a", 33.5)).unwrap();
        let whole = std::fs::read_to_string(&path).unwrap();
        // A write cut short: the second row lost its tail and its newline.
        let second = row("main b", 34.0).to_csv();
        std::fs::write(&path, format!("{whole}{}", &second[..40])).unwrap();
        let h = read(&path).unwrap().unwrap();
        assert_eq!(h.rows.len(), 1);
        assert_eq!(h.skipped.len(), 1);
        assert!(
            h.skipped[0].starts_with("line 3: the last line is torn"),
            "{:?}",
            h.skipped
        );

        // Even a torn row that still parses is skipped: its newline is
        // part of the write.
        std::fs::write(&path, format!("{whole}{second}")).unwrap();
        assert_eq!(read(&path).unwrap().unwrap().rows.len(), 1);

        // The next append ends the torn line first, so its own row is whole;
        // the torn one is skipped where it stands.
        std::fs::write(&path, format!("{whole}{}", &second[..40])).unwrap();
        append(&path, &row("main c", 35.0)).unwrap();
        let h = read(&path).unwrap().unwrap();
        assert_eq!(
            h.rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>(),
            ["main a", "main c"]
        );
        assert_eq!(h.skipped.len(), 1);
        assert!(h.skipped[0].starts_with("line 3: "), "{:?}", h.skipped);
    }

    #[test]
    fn the_warning_starts_at_ten_percent_of_the_limit() {
        // At the edge, exactly: 51.3 ms is 10% under 57.
        assert!(near(51.3, 57.0));
        assert!(!near(51.29, 57.0));
        assert!(near(57.0, 57.0), "no headroom is near");
        assert!(!near(57.01, 57.0), "past the limit is a miss, not near");
        assert!(near(93.6, 104.0));
        assert!(!near(93.59, 104.0));

        let phases = |cold_p95: f64, shutdown_p95: f64| {
            vec![
                ("cold".to_string(), summary(23.8, cold_p95)),
                ("shutdown".to_string(), summary(41.6, shutdown_p95)),
                ("restore".to_string(), summary(115.9, 125.2)),
            ]
        };
        let v = verdicts(&phases(51.3, 93.59), 0, None);
        assert_eq!(
            near_limits(&v),
            ["lifecycle: cold start's p95 51.3 ms is within 10% of its 57 ms limit"]
        );
        assert!(near_limits(&verdicts(&phases(51.29, 93.59), 0, None)).is_empty());
        assert_eq!(
            near_limits(&verdicts(&phases(56.9, 103.9), 0, None)).len(),
            2
        );
    }

    #[test]
    fn history_reads_a_missing_an_empty_and_a_torn_file() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("h.csv");
        let missing = render(&path, read(&path).unwrap().as_ref(), 5);
        assert!(missing.contains("no history yet"), "{missing}");

        std::fs::write(&path, "").unwrap();
        let empty = render(&path, read(&path).unwrap().as_ref(), 5);
        assert!(empty.contains("0 run(s), 0 missed"), "{empty}");
        assert!(empty.contains("no runs recorded yet"), "{empty}");

        for (label, cold) in [("main a", 40.0), ("main b", 58.1), ("main b", 51.3)] {
            append(&path, &row(label, cold)).unwrap();
        }
        let whole = std::fs::read_to_string(&path).unwrap();
        std::fs::write(
            &path,
            format!("{whole}2026-10-01T11:00:00-07:00,main c,1.0,23"),
        )
        .unwrap();
        let out = render(&path, read(&path).unwrap().as_ref(), 2);
        assert!(out.contains("3 run(s), 1 missed"), "{out}");
        assert!(
            out.contains("skipped line 5: the last line is torn"),
            "{out}"
        );
        let cold: Vec<&str> = out
            .lines()
            .skip_while(|l| !l.starts_with("cold start (cold)"))
            .skip(2)
            .take_while(|l| l.starts_with("  2026"))
            .collect();
        assert_eq!(cold.len(), 2, "the last 2 runs of the phase:\n{out}");
        assert!(
            cold[0].contains("main b") && cold[0].ends_with("-1.1  MISSED"),
            "{out}"
        );
        assert!(cold[1].ends_with("5.7  within 10%"), "{out}");
        // A phase within its limit in a run that missed says so.
        assert!(
            out.lines()
                .any(|l| l.contains("35.3") && l.ends_with("(the run missed)")),
            "{out}"
        );
        // Restore has no limit, so no headroom.
        assert!(out.contains("restore from a local WAL (restore)"), "{out}");
    }

    #[test]
    fn the_busy_allowance_carries_a_time_and_never_a_count() {
        // At the edge, exactly: 57.08 ms plus 65% is 94.182 ms.
        assert!(within(94.18, 57.08, 65));
        assert!(!within(94.19, 57.08, 65));
        assert!(within(57.0, 57.0, 0), "at the limit is in, with none");
        assert!(!within(57.01, 57.0, 0));

        // A cold start 10% over its 57 ms limit: carried by 65% and by 11%, not
        // by 5%, nor by 0, which is the strict verdict.
        let v = verdicts(&[("cold".to_string(), summary(23.8, 62.8))], 0, None);
        assert!(!v[0].ok);
        assert!(allowed(&v[0], 65) && allowed(&v[0], 11));
        assert!(!allowed(&v[0], 5));
        assert!(
            !allowed(&v[0], 0),
            "an allowance of 0 is the strict verdict"
        );

        // A count is held to its budget whatever the allowance: one frame over.
        let frames = crate::perf::turn_verdicts(&summary(6.0, 6.0), &summary(10.0, 10.0));
        assert!(!frames[0].ok && !frames[1].ok);
        assert!(!allowed(&frames[0], 100) && !allowed(&frames[1], 100));

        // A phase within its limit passes with or without one.
        let ok = verdicts(&[("shutdown".to_string(), summary(41.6, 51.9))], 0, None);
        assert!(allowed(&ok[0], 0) && allowed(&ok[0], 65));

        // The lines name each phase the allowance carried, and only those: not a
        // phase within its limit, and not one past the allowance too.
        let v = verdicts(
            &[
                ("cold".to_string(), summary(23.8, 33.5)),
                ("shutdown".to_string(), summary(70.0, 112.0)),
                ("kill".to_string(), summary(300.0, 400.0)),
            ],
            0,
            None,
        );
        assert_eq!(
            allowance_applied("lifecycle", &v, 65),
            ["lifecycle: busy: allowance +65% applied to clean shutdown (measured 112.0 ms, limit 104 ms)"]
        );
        assert!(allowance_applied("lifecycle", &v, 0).is_empty());
    }

    #[test]
    fn a_run_the_allowance_carried_reads_back_and_is_counted_apart() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("h.csv");
        // A busy run: the cold start 7% over its limit, carried by 65%. Its
        // strict verdict is a miss.
        let mut carried = row("main busy", 61.2);
        assert!(!carried.passed);
        carried.allowance = Some(65);
        assert!(
            carried.to_csv().ends_with(",false,65"),
            "{}",
            carried.to_csv()
        );
        append(&path, &row("main quiet", 33.5)).unwrap();
        append(&path, &carried).unwrap();
        append(&path, &row("main miss", 99.0)).unwrap();
        let h = read(&path).unwrap().unwrap();
        assert!(h.skipped.is_empty(), "{:?}", h.skipped);
        assert_eq!(h.rows[1], carried);
        assert_eq!((h.rows[0].allowance, h.rows[2].allowance), (None, None));

        let out = render(&path, Some(&h), 3);
        assert!(
            out.contains("3 run(s), 2 missed (1 of them passed on the busy allowance)"),
            "{out}"
        );
        let line = |label: &str, end: &str| {
            out.lines()
                .any(|l| l.contains(label) && l.contains("  2026") && l.ends_with(end))
        };
        assert!(
            line(
                "main busy",
                "busy: allowance +65% applied (measured 61.2 ms, limit 57 ms)"
            ),
            "{out}"
        );
        assert!(line("main miss", "MISSED"), "{out}");
        // Its phases within their limits say how the run passed.
        assert!(
            line("main busy", "(the run passed on the busy allowance)"),
            "{out}"
        );
    }
}
