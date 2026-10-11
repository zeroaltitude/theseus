//! `theseus postmortem` (theseus-m2tiu): one command that writes an
//! incident's timeline from the ledger. It reads `ledger.window` a page at a
//! time (read only: no frame, no lock a turn takes), hands the rows to the
//! library's `render::postmortem_facts::analyze`, and prints the report as
//! markdown, or as data under `--json`.
//!
//! `--rows-out FILE` keeps the rows read as JSON Lines, and `--from-rows
//! FILE` reports from such a file with no daemon: how a real incident's rows
//! become a fixture (`crates/theseus/tests/fixtures/postmortem/`).

use std::io::Write;

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use theseus_client::render::postmortem::markdown;
use theseus_client::render::postmortem_facts::{analyze, Window};
use theseus_client::render::time::local_day_at;
use theseus_client::{resolve, Conn};
use theseus_protocol::{method, LedgerEntry, LedgerWindowParams, LedgerWindowResult};

/// The default window: the last 12 hours.
const DEFAULT_WINDOW_MS: u64 = 12 * 3_600_000;
/// The rows of one page.
const PAGE: usize = 1000;

#[derive(Args, Debug)]
pub struct PostmortemArgs {
    /// The window's start: an age (`2h`, `30m`, `1d`), a time of day (`22:00`, today's, else
    /// yesterday's if it is still ahead), `yesterday 22:00`, `today 06:30`, or an RFC 3339 time.
    /// Default: 12 hours ago.
    #[arg(long, value_name = "T")]
    since: Option<String>,
    /// The window's end, as --since. Default: now.
    #[arg(long, value_name = "T")]
    until: Option<String>,
    /// Only this conversation, and the tasks it started (its id, or the last four characters or
    /// more of it).
    #[arg(long, value_name = "S", conflicts_with = "task")]
    session: Option<String>,
    /// Only this task and the tasks it started (`tsk_…`, `ses_…`, or the end of either).
    #[arg(long, value_name = "T")]
    task: Option<String>,
    /// Write the report to FILE instead of stdout.
    #[arg(long, value_name = "FILE")]
    out: Option<String>,
    /// Keep the ledger rows the report was made from, scrubbed by the daemon, as JSON Lines.
    #[arg(long, value_name = "FILE")]
    rows_out: Option<String>,
    /// Report from rows kept by --rows-out, asking no daemon. The window is the rows' own
    /// unless --since or --until narrow it.
    #[arg(long, value_name = "FILE")]
    from_rows: Option<String>,
}

impl PostmortemArgs {
    /// Whether the report is made from kept rows, with no daemon.
    pub fn is_offline(&self) -> bool {
        self.from_rows.is_some()
    }
}

/// `theseus postmortem`: from kept rows with no daemon, else over a connection.
pub async fn go(socket: &str, spawn: Option<&str>, json: bool, a: PostmortemArgs) -> Result<()> {
    if a.is_offline() {
        return run(None, json, a).await;
    }
    let mut conn = match spawn {
        Some(bin) => Conn::spawn(bin)?,
        None => Conn::socket(socket).await?,
    };
    run(Some(&mut conn), json, a).await
}

/// The report, from `conn` or from kept rows.
async fn run(conn: Option<&mut Conn>, json: bool, a: PostmortemArgs) -> Result<()> {
    let now = theseus_protocol::now_unix_ms();
    let since = a.since.as_deref().map(|s| when(s, now)).transpose()?;
    let until = a.until.as_deref().map(|s| when(s, now)).transpose()?;
    let (rows, scope, since, until) = match (&a.from_rows, conn) {
        (Some(path), _) => {
            let rows = read_rows(path)?;
            let first = rows.first().map_or(now, |r| r.at_unix_ms);
            let last = rows.last().map_or(now, |r| r.at_unix_ms);
            (
                rows,
                Vec::new(),
                since.unwrap_or(first),
                until.unwrap_or(last),
            )
        }
        (None, Some(conn)) => {
            let since = since.unwrap_or(now.saturating_sub(DEFAULT_WINDOW_MS));
            let until = until.unwrap_or(now);
            if until < since {
                bail!("--until is before --since");
            }
            let given = a.session.as_deref().or(a.task.as_deref());
            let session = match given {
                Some(g) if g.starts_with("tsk_") => Some(g.to_string()),
                Some(g) => Some(resolve::session(conn, g).await?),
                None => None,
            };
            let (rows, scope) = fetch(conn, since, until, session).await?;
            (rows, scope, since, until)
        }
        (None, None) => bail!("no daemon connection"),
    };
    if let Some(path) = &a.rows_out {
        write_rows(path, &rows)?;
    }
    let report = analyze(&Window {
        rows: &rows,
        since_ms: since,
        until_ms: until,
        scope: &scope,
    });
    let text = if json {
        format!("{}\n", serde_json::to_string_pretty(&report)?)
    } else {
        markdown(&report)
    };
    match &a.out {
        Some(path) => std::fs::write(path, text).with_context(|| format!("writing {path}"))?,
        None => std::io::stdout().write_all(text.as_bytes())?,
    }
    Ok(())
}

/// The rows of `[since, until]` and the sessions the window was narrowed to,
/// a page at a time from the position the last page ended at.
async fn fetch(
    conn: &mut Conn,
    since: u64,
    until: u64,
    session: Option<String>,
) -> Result<(Vec<LedgerEntry>, Vec<String>)> {
    let mut rows = Vec::new();
    let mut after = None;
    loop {
        let p = LedgerWindowParams {
            since_ms: since,
            until_ms: Some(until),
            session_id: session.clone(),
            after,
            limit: Some(PAGE),
        };
        let r: LedgerWindowResult = serde_json::from_value(
            conn.request(method::LEDGER_WINDOW, serde_json::to_value(&p)?)
                .await?,
        )?;
        if r.building {
            bail!("the ledger's index is still being built after a start: ask again in a minute");
        }
        rows.extend(r.rows);
        match r.next {
            Some(n) => after = Some(n),
            None => return Ok((rows, r.scope)),
        }
    }
}

fn read_rows(path: &str) -> Result<Vec<LedgerEntry>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let mut rows = Vec::new();
    for (i, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        rows.push(
            serde_json::from_str::<LedgerEntry>(line)
                .map_err(|e| anyhow!("{path} line {}: {e}", i + 1))?,
        );
    }
    rows.sort_by_key(|r| r.position);
    Ok(rows)
}

fn write_rows(path: &str, rows: &[LedgerEntry]) -> Result<()> {
    let mut out = String::new();
    for r in rows {
        out.push_str(&serde_json::to_string(r)?);
        out.push('\n');
    }
    std::fs::write(path, out).with_context(|| format!("writing {path}"))
}

/// A time as the flags write it, in unix ms.
fn when(s: &str, now: u64) -> Result<u64> {
    let t = s.trim();
    let bad = || {
        anyhow!(
            "`{s}` is no time: write an age (30m, 2h, 1d), a time of day (22:00), `yesterday 22:00`, \
             `today 06:30`, or an RFC 3339 time (2026-10-10T22:00:00-07:00)"
        )
    };
    if let Some(ms) = age(t) {
        return Ok(now.saturating_sub(ms));
    }
    if let Ok(ts) = t.parse::<jiff::Timestamp>() {
        return u64::try_from(ts.as_millisecond()).map_err(|_| bad());
    }
    let (days, rest) = match t.split_once(' ') {
        Some(("yesterday", r)) => (Some(1), r.trim()),
        Some(("today", r)) => (Some(0), r.trim()),
        _ if t == "yesterday" => (Some(1), "00:00"),
        _ if t == "today" => (Some(0), "00:00"),
        _ => (None, t),
    };
    let (h, m) = rest.split_once(':').ok_or_else(bad)?;
    let (h, m): (i8, i8) = (h.parse().map_err(|_| bad())?, m.parse().map_err(|_| bad())?);
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return Err(bad());
    }
    match days {
        Some(d) => local_day_at(now, d, h, m).ok_or_else(bad),
        // A bare time of day is the latest one that is not ahead.
        None => {
            let today = local_day_at(now, 0, h, m).ok_or_else(bad)?;
            if today <= now {
                Ok(today)
            } else {
                local_day_at(now, 1, h, m).ok_or_else(bad)
            }
        }
    }
}

/// `30m`, `2h`, `1d`, `1w` in milliseconds.
fn age(s: &str) -> Option<u64> {
    let split = s.find(|c: char| !c.is_ascii_digit())?;
    let (n, unit) = s.split_at(split);
    let n: u64 = n.parse().ok()?;
    let unit = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        "w" => 7 * 86_400_000,
        _ => return None,
    };
    Some(n * unit)
}

#[cfg(test)]
#[path = "postmortem_tests.rs"]
mod tests;
