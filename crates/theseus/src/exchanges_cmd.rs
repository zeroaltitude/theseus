//! `theseus exchanges report` and `theseus exchanges rebuild` (issue 44, step
//! 1): the exchange ledger's measuring rows summed into counts and shares,
//! and the exchanges of older history written from the conversations' nodes
//! (`--dry-run` counts and writes nothing). Ids, kinds and counts only.

use anyhow::Result;
use clap::Subcommand;
use theseus_client::Conn;
use theseus_protocol::exchange::{
    ExchangeLadderParams, ExchangeLadderResult, ExchangeRebuildParams, ExchangeRebuildResult,
    ExchangeReportParams, ExchangeReportResult,
};
use theseus_protocol::method;

#[derive(Subcommand, Debug)]
pub enum ExchangesCmd {
    /// The measuring rows of private turns, summed: whether the place's last exchange was in
    /// view, what the context sent was made of, cold starts, the things named, the judge's
    /// unread answers, gaps and overlaps, and who typed at the terminal.
    Report {
        /// Only the newest N rows.
        #[arg(short, long)]
        n: Option<u64>,
    },
    /// Write the exchanges of turns from before the ledger, from the nodes. A place and a
    /// person are filled only where a stored record proves them; the rest say unknown.
    Rebuild {
        /// Count what would be written, and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Build a place's ladder from its history since its latest /new: a one-line summary of
    /// each exchange without one, then the fold nodes. `--dry-run` counts and prices it.
    Ladder {
        /// The place, as the ledger names it (`discord:channel:<id>`, `local:<dir>`).
        place: String,
        /// Count what would be written and its worst case, and write nothing.
        #[arg(long)]
        dry_run: bool,
        /// The most this run may spend, in dollars (default: what is left of the day's line).
        #[arg(long)]
        cap_usd: Option<f64>,
    },
}

pub async fn run(conn: &mut Conn, json: bool, cmd: ExchangesCmd) -> Result<()> {
    let (m, p) = match &cmd {
        ExchangesCmd::Report { n } => (
            method::EXCHANGE_REPORT,
            serde_json::to_value(ExchangeReportParams { limit: *n })?,
        ),
        ExchangesCmd::Rebuild { dry_run } => (
            method::EXCHANGE_REBUILD,
            serde_json::to_value(ExchangeRebuildParams { dry_run: *dry_run })?,
        ),
        ExchangesCmd::Ladder {
            place,
            dry_run,
            cap_usd,
        } => (
            method::EXCHANGE_LADDER,
            serde_json::to_value(ExchangeLadderParams {
                place: place.clone(),
                dry_run: *dry_run,
                cap_usd: *cap_usd,
            })?,
        ),
    };
    let v = conn.request(m, p).await?;
    match (json, cmd) {
        (true, _) => println!("{}", serde_json::to_string(&v)?),
        (false, ExchangesCmd::Report { .. }) => {
            println!("{}", report_lines(&serde_json::from_value(v)?))
        }
        (false, ExchangesCmd::Rebuild { .. }) => {
            println!("{}", rebuild_lines(&serde_json::from_value(v)?))
        }
        (false, ExchangesCmd::Ladder { .. }) => {
            println!("{}", ladder_lines(&serde_json::from_value(v)?))
        }
    }
    Ok(())
}

fn share(n: u64, of: u64) -> String {
    match of {
        0 => "-".into(),
        _ => format!("{:.0}%", n as f64 * 100.0 / of as f64),
    }
}

/// The report, a group a line: each value's count and its share of the group.
pub fn report_lines(r: &ExchangeReportResult) -> String {
    let mut out = vec![format!("{} turns in private places", r.rows)];
    for (group, values) in &r.counts {
        let all: u64 = values.values().sum();
        let parts: Vec<String> = values
            .iter()
            .map(|(v, n)| format!("{v} {n} ({})", share(*n, all)))
            .collect();
        out.push(format!("  {group}: {}", parts.join(", ")));
    }
    if r.rows > 0 {
        let parts: Vec<String> = r
            .tokens
            .iter()
            .map(|(k, n)| format!("{k} {}", n / r.rows))
            .collect();
        out.push(format!(
            "  sent, mean tokens a turn (est.): {}",
            parts.join(", ")
        ));
    }
    if !r.places_bound.is_empty() {
        let parts: Vec<String> = r
            .places_bound
            .iter()
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        out.push(format!("  places bound: {}", parts.join(", ")));
    }
    out.join("\n")
}

pub fn rebuild_lines(r: &ExchangeRebuildResult) -> String {
    let verb = if r.dry_run { "would write" } else { "wrote" };
    let mut s = format!(
        "{} sessions, {} turns: {} already had an exchange; {verb} {} exchanges ({} with a proven \
         place, {} with a proven person) and {} touches",
        r.sessions, r.turns, r.kept, r.exchanges, r.place_proven, r.person_proven, r.touches
    );
    if let Some(why) = &r.things_skipped {
        s.push_str(&format!(
            "; the things they name were not looked for: {why}"
        ));
    }
    s
}

/// A ladder build in one line, and why it stopped when it did.
pub fn ladder_lines(r: &ExchangeLadderResult) -> String {
    let (lines, folds) = match r.dry_run {
        true => ("would write", "would make"),
        false => ("wrote", "made"),
    };
    let mut s = format!(
        "{}: {} exchanges since its latest /new; {} had a summary, {lines} {}; {folds} {} fold \
         nodes and {} above them; worst case ${:.2}, spent ${:.2}",
        r.place, r.exchanges, r.lines_had, r.lines, r.folds, r.above, r.estimated_usd, r.spent_usd
    );
    if let Some(why) = &r.stopped {
        s.push_str(&format!("; stopped: {why}"));
    }
    s
}
