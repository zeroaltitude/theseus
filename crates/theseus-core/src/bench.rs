//! `bench.history` (the cockpit's speed wall): the gates' bench history on
//! this machine, read from the CSV the gate appends (theseus-1hk), so the wall
//! can draw every gate's numbers against their limits.
//!
//! The file is `$THESEUS_BENCH_HISTORY`, or `~/.cache/theseus/bench-history.csv`,
//! as `theseus-sim bench history` reads it; this is the same reading, kept
//! here because the daemon does not link the sim. A header line (`time,…`)
//! comes again when a bench's columns change, and each row reads under the
//! last header above it. Every row ends with its newline, written in the same
//! `write`, so a last line without one was cut short and is left out.

use std::path::{Path, PathBuf};

use theseus_protocol::bench::{BenchHistoryResult, BenchPhase, BenchRun};

/// Runs a read returns when it names none, and the most it returns.
pub const DEFAULT_LAST: usize = 500;
pub const MAX_LAST: usize = 5000;

/// Where the history is: `$THESEUS_BENCH_HISTORY`, else under `$HOME`.
pub fn path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("THESEUS_BENCH_HISTORY").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/theseus/bench-history.csv"))
}

/// The history at `path`, its newest `last` runs: `exists: false` when there
/// is no file.
pub fn read(path: Option<&Path>, last: Option<usize>) -> BenchHistoryResult {
    let last = last.unwrap_or(DEFAULT_LAST).min(MAX_LAST);
    let Some(path) = path else {
        return BenchHistoryResult {
            skipped: vec!["neither THESEUS_BENCH_HISTORY nor HOME is set".into()],
            ..Default::default()
        };
    };
    let shown = Some(path.display().to_string());
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return BenchHistoryResult {
                path: shown,
                ..Default::default()
            }
        }
        Err(e) => {
            return BenchHistoryResult {
                path: shown,
                exists: true,
                skipped: vec![format!("reading it failed: {e}")],
                ..Default::default()
            }
        }
    };
    let (mut runs, skipped) = parse_file(&text);
    let total = runs.len() as u64;
    if runs.len() > last {
        runs.drain(..runs.len() - last);
    }
    BenchHistoryResult {
        path: shown,
        exists: true,
        runs,
        total,
        skipped,
    }
}

/// Every run in the file, oldest first, and the lines left out with why.
fn parse_file(text: &str) -> (Vec<BenchRun>, Vec<String>) {
    let (mut runs, mut skipped) = (Vec::new(), Vec::new());
    let mut header: Option<Vec<String>> = None;
    let torn = !text.is_empty() && !text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let n = i + 1;
        if torn && n == lines.len() {
            skipped.push(format!(
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
                Err(why) => skipped.push(format!("line {n}: a header, but {why}")),
            }
            continue;
        }
        let Some(cols) = &header else {
            skipped.push(format!("line {n}: a row before any header"));
            continue;
        };
        match parse(cols, line) {
            Ok(run) => runs.push(run),
            Err(why) => skipped.push(format!("line {n}: {why}")),
        }
    }
    (runs, skipped)
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
        return Err("a quote is never closed".into());
    }
    fields.push(cur);
    Ok(fields)
}

/// One data line, read by the column names of the header above it.
fn parse(header: &[String], line: &str) -> Result<BenchRun, String> {
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
            (Some(p50), Some(p95)) => phases.push(BenchPhase {
                phase: name.to_string(),
                p50,
                p95,
                limit: num(&format!("{name}_limit"))?,
            }),
            (None, None) => {}
            _ => return Err(format!("{name} has a p50 or a p95, not both")),
        }
    }
    Ok(BenchRun {
        time: col("time").ok_or("no time column")?.to_string(),
        label: col("label").unwrap_or_default().to_string(),
        load1: num("load1")?,
        passed: match col("passed") {
            Some("true") => true,
            Some("false") => false,
            other => return Err(format!("passed is {other:?}, not true or false")),
        },
        phases,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows read under the header above each, the newest `last` kept, an
    /// empty phase left out, and a torn last line skipped with its reason.
    #[test]
    fn the_history_reads_under_each_header_and_skips_a_torn_line() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("bench-history.csv");
        let text = "time,label,load1,cold_p50,cold_p95,cold_limit,passed\n\
                    2026-10-01T10:30:06-07:00,lane/a 1a2b3c4,4.88,24.93,29.01,57.08,true\n\
                    time,label,load1,cold_p50,cold_p95,cold_limit,frames_plain_p50,frames_plain_p95,frames_plain_limit,passed\n\
                    2026-10-01T11:00:00-07:00,\"main 5d6e7f8, again\",,,,,5,5,5,true\n\
                    2026-10-01T11:30:00-07:00,main 9a8b7c6,3.5,61.2,70.1,57.08,,,,false\n\
                    2026-10-01T12:00:00-07:00,main torn,3";
        std::fs::write(&p, text).unwrap();
        let h = read(Some(&p), None);
        assert!(h.exists);
        assert_eq!(h.total, 3);
        assert_eq!(h.runs.len(), 3);
        assert_eq!(h.runs[0].phases.len(), 1);
        assert_eq!(h.runs[0].phases[0].limit, Some(57.08));
        assert_eq!(h.runs[1].label, "main 5d6e7f8, again");
        assert_eq!(h.runs[1].load1, None);
        assert_eq!(
            h.runs[1].phases,
            vec![BenchPhase {
                phase: "frames_plain".into(),
                p50: 5.0,
                p95: 5.0,
                limit: Some(5.0)
            }]
        );
        assert!(!h.runs[2].passed);
        assert_eq!(h.skipped.len(), 1);
        assert!(h.skipped[0].contains("torn"), "{:?}", h.skipped);
        let newest = read(Some(&p), Some(1));
        assert_eq!(newest.runs.len(), 1);
        assert_eq!(newest.runs[0].label, "main 9a8b7c6");
        assert_eq!(newest.total, 3, "total counts the file, not the cut");
        let none = read(Some(&d.path().join("nothing.csv")), None);
        assert!(!none.exists && none.runs.is_empty());
    }
}
