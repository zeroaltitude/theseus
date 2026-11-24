//! `theseus ledger exchanges` and `theseus ledger rebuild-exchanges` (issue
//! 44, step 1): the exchange ledger's measuring rows summed into counts and shares,
//! and the exchanges of older history written from the conversations' nodes
//! (`--dry-run` counts and writes nothing). Ids, kinds and counts only. And
//! `theseus ledger ladder` (step 4): a place's `did` lines and fold nodes built
//! from its history, with a dry run that counts and prices, and a cap. And
//! `theseus ledger gists`: those lines and nodes beside what their writer
//! read, a sheet for the owner to mark.

use anyhow::Result;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::exchange::{
    ExchangeGistsParams, ExchangeGistsResult, ExchangeLadderParams, ExchangeLadderResult,
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

/// `theseus ledger ladder`.
pub async fn ladder(
    conn: &mut Conn,
    json: bool,
    place: String,
    (dry_run, cap_usd, over_day_line): (bool, Option<f64>, bool),
) -> Result<()> {
    let p = serde_json::to_value(ExchangeLadderParams {
        place,
        dry_run,
        cap_usd,
        over_day_line,
    })?;
    let v = conn.request(method::EXCHANGE_LADDER, p).await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => println!("{}", ladder_lines(&serde_json::from_value(v)?)),
    }
    Ok(())
}

/// `theseus ledger gists`.
pub async fn gists(conn: &mut Conn, json: bool, p: ExchangeGistsParams) -> Result<()> {
    let v = conn
        .request(method::EXCHANGE_GISTS, serde_json::to_value(&p)?)
        .await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => println!("{}", gists_lines(&p, &serde_json::from_value(v)?)),
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
            "; {} touches keyed off their exchange's start were moved to it",
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
    s.push_str(&format!("; ${:.2} left of today's line", r.day_left_usd));
    if r.needs_over_day_line {
        s.push_str("; finishing would pass it: run again with --cap-usd N --over-day-line");
    }
    s
}

/// The reading sheet: what exists and what was drawn, the seed, fewer than asked said so, then each item
/// numbered: what its writer read, it as the view shows it, and a blank verdict.
pub fn gists_lines(asked: &ExchangeGistsParams, r: &ExchangeGistsResult) -> String {
    let whence = match &r.place {
        Some(p) => format!("in {p}"),
        None => "in every place".to_string(),
    };
    let mut out = vec![
        format!(
            "Reading sheet, {whence}: {} of {} one-line summaries and {} of {} first-level fold nodes \
             (seed {}; --seed {} draws the same ones again while no more are written).",
            r.lines.len(),
            r.lines_total,
            r.nodes.len(),
            r.nodes_total,
            r.seed,
            r.seed
        ),
        "Mark each one: wrong number/name/choice, other error, or ok. Nothing here is stored; keep \
         your counts."
            .to_string(),
    ];
    for (what, total, want) in [
        ("One-line summaries", r.lines_total, asked.lines),
        ("First-level fold nodes", r.nodes_total, asked.nodes),
    ] {
        if total < u64::from(want) {
            out.push(format!(
                "{what} {whence}: {total}, fewer than the {want} asked, so all are shown. Read the \
                 rest when they exist."
            ));
        }
    }
    for (i, l) in r.lines.iter().enumerate() {
        out.push(String::new());
        out.push(format!(
            "== Summary {} of {}: {} in {}, {} ({}, writer {})",
            i + 1,
            r.lines.len(),
            l.turn_id,
            l.place,
            fmt_stamp(l.at_ms),
            l.model,
            l.writer
        ));
        out.push("-- What its writer read:".into());
        out.push(
            l.read
                .clone()
                .unwrap_or_else(|| "(its exchange can no longer be read)".into()),
        );
        out.push("-- The summary, as the view shows it:".into());
        out.push(l.line.clone());
        out.push("Verdict: ____".into());
    }
    for (i, n) in r.nodes.iter().enumerate() {
        out.push(String::new());
        out.push(format!(
            "== Fold node {} of {}: {} in {}, {} to {}, {} exchanges ({} still readable; {}, writer {})",
            i + 1,
            r.nodes.len(),
            n.id,
            n.place,
            fmt_stamp(n.first_ms),
            fmt_stamp(n.last_ms),
            n.exchanges,
            n.readable,
            n.model.as_deref().unwrap_or("code"),
            n.writer
        ));
        out.push("-- What its writer read:".into());
        out.push(n.read.trim_end().to_string());
        out.push("-- The node, as the view shows it:".into());
        out.push(n.shown.clone());
        out.push("Verdict: ____".into());
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::exchange::{GistLine, GistNode};

    /// Fewer nodes exist than were asked: the sheet says how many, and that all are shown.
    #[test]
    fn gists_says_how_many_nodes_exist_when_fewer_than_asked() {
        theseus_client::render::time::pin_for_tests();
        let r = ExchangeGistsResult {
            place: None,
            seed: 4711,
            lines_total: 80,
            nodes_total: 1,
            lines: vec![GistLine {
                turn_id: "trn_a".into(),
                place: "local:harbour".into(),
                at_ms: 1_760_000_000_000,
                model: "fake-small-1".into(),
                writer: 2,
                read: Some("The person's message:\nRecalibrate gauge 4".into()),
                line: "Recalibrated gauge 4.".into(),
            }],
            nodes: vec![GistNode {
                id: "fld_a".into(),
                place: "local:harbour".into(),
                exchanges: 20,
                readable: 19,
                read: "Exchange 1 (2025-10-09):\nThe person's message: Recalibrate gauge 4".into(),
                shown: "\"Tide gauges\": earlier here\nwhat Theseus did: recalibrated".into(),
                ..GistNode::default()
            }],
        };
        let text = gists_lines(&ExchangeGistsParams::default(), &r);
        assert!(
            text.contains("1 of 80 one-line summaries and 1 of 1 first-level fold nodes"),
            "{text}"
        );
        assert!(text.contains("seed 4711; --seed 4711"), "{text}");
        assert!(
            text.contains("First-level fold nodes in every place: 1, fewer than the 10 asked, so all are shown"),
            "{text}"
        );
        assert!(
            !text.contains("One-line summaries in"),
            "enough summaries exist: {text}"
        );
        assert!(
            text.contains("-- What its writer read:\nThe person's message:\nRecalibrate gauge 4")
        );
        assert!(text.contains("-- The summary, as the view shows it:\nRecalibrated gauge 4."));
        assert!(
            text.contains("20 exchanges (19 still readable; code, writer 0)"),
            "{text}"
        );
        assert!(text.contains("-- The node, as the view shows it:\n\"Tide gauges\""));
        assert_eq!(text.matches("Verdict: ____").count(), 2);
    }
}
