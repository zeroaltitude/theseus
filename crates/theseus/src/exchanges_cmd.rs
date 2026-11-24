//! `theseus ledger exchanges` and `theseus ledger rebuild-exchanges` (issue
//! 44, step 1): the exchange ledger's measuring rows summed into counts and shares,
//! and the exchanges of older history written from the conversations' nodes
//! (`--dry-run` counts and writes nothing). Ids, kinds and counts only. And
//! `theseus ledger ladder` (step 4): a place's `did` lines and fold nodes built
//! from its history, with a dry run that counts and prices, and a cap. And
//! `theseus ledger topics` (issue 46): the history from before topics were
//! filed per exchange, filed under them once; counts and dollars only.

use anyhow::Result;
use theseus_client::Conn;
use theseus_protocol::exchange::{
    ExchangeLadderParams, ExchangeLadderResult, ExchangeRebuildParams, ExchangeRebuildResult,
    ExchangeReportParams, ExchangeReportResult, ExchangeTopicsParams, ExchangeTopicsResult,
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

/// `theseus ledger topics`.
pub async fn topics(
    conn: &mut Conn,
    json: bool,
    dry_run: bool,
    cap_usd: Option<f64>,
) -> Result<()> {
    let p = topics_params(dry_run, cap_usd)?;
    let v = conn.request(method::EXCHANGE_TOPICS, p).await?;
    match json {
        true => println!("{}", serde_json::to_string(&v)?),
        false => println!("{}", topics_lines(&serde_json::from_value(v)?)),
    }
    Ok(())
}

/// The request of `theseus ledger topics`: a cap is checked before it is
/// built (JSON has no NaN or infinity, and would send one as no cap at all).
pub fn topics_params(dry_run: bool, cap_usd: Option<f64>) -> Result<serde_json::Value> {
    if let Some(usd) = cap_usd {
        theseus_protocol::exchange::topics_cap(usd).map_err(anyhow::Error::msg)?;
    }
    Ok(serde_json::to_value(ExchangeTopicsParams { dry_run, cap_usd })?)
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

/// A filing of the history in two lines: what the ledger holds, and what the
/// run would ask or did. Counts and dollars, never a message's words.
pub fn topics_lines(r: &ExchangeTopicsResult) -> String {
    let mut s = match r.dry_run {
        true => format!("a dry run for a cap of ${:.2} a run: nothing is asked\n", r.cap_usd),
        false => format!("a run with a cap of ${:.2}\n", r.cap_usd),
    };
    s.push_str(&format!(
        "{} exchanges: {} already touch a topic, {} are not yours, {} were asked before, {} open \
         with no message of yours, {} are in places the ontology's judge does not act on now",
        r.exchanges, r.touched_before, r.not_owner, r.asked_before, r.no_message, r.not_acting
    ));
    match r.dry_run {
        true => s.push_str(&format!(
            "\nwould ask {n}: ${:.4} at worst for all {n} of them (it may be more than the cap); a run \
             stops at its cap of ${:.2}",
            r.worst_usd,
            r.cap_usd,
            n = r.to_ask
        )),
        false => s.push_str(&format!(
            "\nasked {} of {} ({} answered): {} exchanges filed with {} touches; spent ${:.4} of a \
             cap of ${:.2}",
            r.asked, r.to_ask, r.answered, r.filed, r.touches, r.spent_usd, r.cap_usd
        )),
    }
    if let Some(why) = &r.stopped {
        s.push_str(&format!("\nstopped: {why}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The command prints counts and dollars, whole and in order: the cap in
    /// use first, then what the ledger holds, what the dry run would ask and
    /// its worst case, what a run asked and spent, and why it stopped.
    #[test]
    fn the_topics_lines_are_counts_and_dollars() {
        let dry = ExchangeTopicsResult {
            dry_run: true,
            cap_usd: 10.0,
            exchanges: 9,
            touched_before: 2,
            not_owner: 1,
            asked_before: 1,
            no_message: 2,
            not_acting: 0,
            to_ask: 3,
            worst_usd: 0.0012,
            ..ExchangeTopicsResult::default()
        };
        assert_eq!(
            topics_lines(&dry),
            "a dry run for a cap of $10.00 a run: nothing is asked\n9 exchanges: 2 already touch \
             a topic, 1 are not yours, 1 were asked before, 2 open with no message of yours, 0 are \
             in places the ontology's judge does not act on now\nwould ask 3: $0.0012 at worst \
             for all 3 of them (it may be more than the cap); a run stops at its cap of $10.00"
        );
        let run = ExchangeTopicsResult {
            dry_run: false,
            asked: 1,
            answered: 1,
            filed: 1,
            touches: 2,
            spent_usd: 0.0003,
            left: 2,
            stopped: Some("stopped at the cap".into()),
            ..dry
        };
        assert_eq!(
            topics_lines(&run),
            "a run with a cap of $10.00\n9 exchanges: 2 already touch a topic, 1 are not yours, 1 \
             were asked before, 2 open with no message of yours, 0 are in places the ontology's \
             judge does not act on now\nasked 1 of 3 (1 answered): 1 exchanges filed with 2 \
             touches; spent $0.0003 of a cap of $10.00\nstopped: stopped at the cap"
        );
    }

    /// The cap is read before a request is built: NaN, the infinities, zero,
    /// a negative number and a sum too small to round to any money are
    /// refused by the parser, and by the request's builder; a cap that holds
    /// goes on the wire as it was typed, and none goes as none (the daemon's
    /// $10).
    #[test]
    fn a_cap_that_is_no_money_never_reaches_the_wire() {
        use clap::Parser;
        use serde_json::json;
        #[derive(Parser, Debug)]
        struct T {
            #[command(subcommand)]
            cmd: crate::ledger_verify::LedgerCmd,
        }
        let parse = |cap: Option<&str>| {
            let mut argv = vec!["t".to_string(), "topics".to_string()];
            argv.extend(cap.map(|c| format!("--cap-usd={c}")));
            T::try_parse_from(argv)
        };
        for bad in [
            "NaN", "nan", "inf", "-inf", "infinity", "0", "-0", "-1", "0.0000004", "ten",
        ] {
            assert!(parse(Some(bad)).is_err(), "{bad} was parsed");
        }
        let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 4e-7];
        for usd in bad {
            assert!(topics_params(false, Some(usd)).is_err(), "{usd} was built");
        }
        let good = [
            (None, json!({"dry_run": false})),
            (Some("2.5"), json!({"dry_run": false, "cap_usd": 2.5})),
            (Some("0.000001"), json!({"dry_run": false, "cap_usd": 0.000001})),
            (Some("1e9"), json!({"dry_run": false, "cap_usd": 1e9})),
        ];
        for (cap, wire) in good {
            let T { cmd } = parse(cap).unwrap();
            let crate::ledger_verify::LedgerCmd::Topics { dry_run, cap_usd } = cmd else {
                panic!("not topics: {cmd:?}");
            };
            assert_eq!(topics_params(dry_run, cap_usd).unwrap(), wire, "{cap:?}");
        }
    }
}
