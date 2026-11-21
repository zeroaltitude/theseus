//! `theseus ledger exchanges` and `theseus ledger rebuild-exchanges` (issue
//! 44, step 1): the exchange ledger's measuring rows summed into counts and shares,
//! and the exchanges of older history written from the conversations' nodes
//! (`--dry-run` counts and writes nothing). Ids, kinds and counts only.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::exchange::{
    ExchangeRebuildParams, ExchangeRebuildResult, ExchangeReportParams, ExchangeReportResult,
};
use theseus_protocol::method;

/// `theseus ledger exchanges`.
pub async fn report(conn: &mut Conn, json: bool, n: Option<u64>) -> Result<()> {
    let p = serde_json::to_value(ExchangeReportParams { limit: n })?;
    let v = conn.request(method::EXCHANGE_REPORT, p).await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => println!("{}", report_lines(&serde_json::from_value(v)?)),
    }
    Ok(())
}

/// `theseus ledger rebuild-exchanges`.
pub async fn rebuild(conn: &mut Conn, json: bool, dry_run: bool) -> Result<()> {
    let p = serde_json::to_value(ExchangeRebuildParams { dry_run })?;
    let v = conn.request(method::EXCHANGE_REBUILD, p).await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => println!("{}", rebuild_lines(&serde_json::from_value(v)?)),
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
    if r.repaired > 0 {
        s.push_str(&format!(
            "; {} touches keyed off their exchange's start were ended",
            r.repaired
        ));
    }
    if let Some(why) = &r.things_skipped {
        s.push_str(&format!(
            "; the things they name were not looked for: {why}"
        ));
    }
    s
}
