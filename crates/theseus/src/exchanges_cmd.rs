//! `theseus ledger exchanges` and `theseus ledger rebuild-exchanges` (issue
//! 44, step 1): the exchange ledger's measuring rows summed into counts and shares,
//! and the exchanges of older history written from the conversations' nodes
//! (`--dry-run` counts and writes nothing). Ids, kinds and counts only. And
//! `theseus ledger ladder` (step 4): a place's `did` lines and fold nodes built
//! from its history, with a dry run that counts and prices, and a cap.

use std::collections::BTreeMap;

use anyhow::Result;
use theseus_client::render::time::fmt_day;
use theseus_client::Conn;
use theseus_protocol::exchange::{
    ExchangeAnswers, ExchangeLadderParams, ExchangeLadderResult, ExchangeRebuildParams,
    ExchangeRebuildResult, ExchangeReportParams, ExchangeReportResult,
};
use theseus_protocol::method;

/// What a daemon older than the answers gets instead of a report: its groups can carry a task's
/// id, and it never read `--since`.
const UPDATE: &str = "this daemon does not report the answers; update it";

/// `theseus ledger exchanges`: a daemon that does not report the answers is told to update, in
/// either mode, and none of its groups are printed.
pub async fn report(
    conn: &mut Conn,
    json: bool,
    n: Option<u64>,
    since: Option<&str>,
) -> Result<()> {
    let now = theseus_protocol::now_unix_ms();
    let since_ms = since
        .map(|s| theseus_client::render::task_history::since_ms(s, now))
        .transpose()?;
    let p = serde_json::to_value(ExchangeReportParams { limit: n, since_ms })?;
    let v = conn.request(method::EXCHANGE_REPORT, p).await?;
    if v.get("answers").is_none_or(serde_json::Value::is_null) {
        anyhow::bail!(UPDATE);
    }
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

/// `pairs`, each as its label, its count and its share of `of`.
fn shares(m: &BTreeMap<String, u64>, pairs: &[(&str, &str)], of: u64) -> String {
    let parts: Vec<String> = (pairs.iter())
        .map(|(k, label)| {
            let n = m.get(*k).copied().unwrap_or(0);
            format!("{label} {n} ({})", share(n, of))
        })
        .collect();
    parts.join(", ")
}

/// The sent context's parts, by their names in the row, and as the report says them.
const PARTS: [(&str, &str); 10] = [
    ("header", "header"),
    ("tools", "tools"),
    ("files", "project files"),
    ("replies", "replies"),
    ("calls", "tool calls"),
    ("words", "his words"),
    ("results_whole", "tool results"),
    ("results_cleared", "cleared results"),
    ("recall", "recall"),
    ("other", "other"),
];

/// The three answers, a line each, with the whole each share is of.
fn answer_lines(a: &ExchangeAnswers, out: &mut Vec<String>) {
    let get = |m: &BTreeMap<String, u64>, k: &str| m.get(k).copied().unwrap_or(0);
    out.push(format!("  his own messages: {} turns", a.his_turns));
    if a.unreadable > 0 {
        out.push(format!(
            "    whose exchange does not read: {}",
            a.unreadable
        ));
    }
    let not_measured = get(&a.previous, "not_measured");
    let measured = a.his_turns.saturating_sub(not_measured);
    let missed = get(&a.previous, "missed");
    out.push(format!(
        "  the previous exchange here, of his {measured} measured turns: {}, missed {missed} ({}; held \
         by a seal or a gate, or a real miss), {}; not measured {not_measured}",
        shares(&a.previous, &[("shown", "shown")], measured),
        share(missed, measured),
        shares(
            &a.previous,
            &[("after_new", "held after a /new"), ("first_here", "first here")],
            measured
        ),
    ));
    if missed > 0 {
        let (same, other) = (
            get(&a.missed_by_session, "same"),
            get(&a.missed_by_session, "other"),
        );
        out.push(format!(
            "    missed, where the previous ran: this session {same}, another session {other}"
        ));
    }
    let (some, unknown) = (get(&a.named, "some"), get(&a.named, "unknown"));
    let known = some + get(&a.named, "none");
    out.push(format!(
        "  he named something the index knows: {some} of {known} turns ({}); not known (index late) \
         {unknown}",
        share(some, known)
    ));
    if !a.named_kinds.is_empty() {
        let kinds: Vec<String> = (a.named_kinds.iter())
            .map(|(k, n)| format!("{k} {n}"))
            .collect();
        out.push(format!("    turns naming each kind: {}", kinds.join(", ")));
    }
    let n = a.context_turns;
    if n == 0 {
        out.push("  the context sent: no turn's first call was measured".into());
        return;
    }
    let mean: BTreeMap<String, u64> = (a.context.iter())
        .map(|(k, v)| (k.clone(), v / n))
        .collect();
    let all: u64 = PARTS.iter().map(|(k, _)| get(&mean, k)).sum();
    let parts: Vec<(&str, &str)> = (PARTS.iter().copied())
        .filter(|(k, _)| get(&mean, k) > 0)
        .collect();
    out.push(format!(
        "  the context sent, mean a turn (est., {n} turns measured): {all} tokens: {}; the compiler's \
         own estimate {}; cache read {}, written {} a turn",
        shares(&mean, &parts, all),
        get(&mean, "total"),
        a.cache_read / n,
        a.cache_write / n,
    ));
}

/// The report: the three answers first, then every group a line, each value's count and its share of
/// the group.
pub fn report_lines(r: &ExchangeReportResult) -> String {
    let mut head = format!("{} turns in private places", r.rows);
    if let (Some(from), Some(to)) = (r.from_ms, r.to_ms) {
        head.push_str(&format!(", {} to {}", fmt_day(from), fmt_day(to)));
    }
    if r.rows == 0 {
        head.push_str(": nothing to report yet");
    }
    let mut out = vec![head];
    match &r.answers {
        Some(a) if r.rows > 0 => answer_lines(a, &mut out),
        Some(_) => {}
        None if r.rows > 0 => {
            out.push("  this daemon does not report the answers; update it".into())
        }
        None => {}
    }
    if !r.counts.is_empty() {
        out.push("  all groups:".into());
    }
    for (group, values) in &r.counts {
        let all: u64 = values.values().sum();
        let parts: Vec<String> = values
            .iter()
            .map(|(v, n)| format!("{v} {n} ({})", share(*n, all)))
            .collect();
        out.push(format!("    {group}: {}", parts.join(", ")));
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use theseus_protocol::exchange::{ExchangeAnswers, ExchangeReportResult};

    use super::report_lines;

    fn map(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
    }

    #[test]
    fn an_empty_report_says_there_is_nothing_yet() {
        assert_eq!(
            report_lines(&ExchangeReportResult::default()),
            "0 turns in private places: nothing to report yet"
        );
    }

    #[test]
    fn the_three_answers_come_first_with_their_wholes() {
        let r = ExchangeReportResult {
            rows: 12,
            counts: [("prev".to_string(), map(&[("missed", 3), ("transcript", 9)]))].into(),
            tokens: map(&[("header", 1_000)]),
            places_bound: map(&[("dm", 1)]),
            answers: Some(ExchangeAnswers {
                his_turns: 10,
                unreadable: 0,
                previous: map(&[
                    ("shown", 5),
                    ("missed", 2),
                    ("after_new", 1),
                    ("first_here", 1),
                    ("not_measured", 1),
                ]),
                missed_by_session: map(&[("same", 1), ("other", 1)]),
                named: map(&[("some", 3), ("none", 6), ("unknown", 1)]),
                named_kinds: map(&[("path", 3), ("commit", 1)]),
                context_turns: 10,
                context: map(&[
                    ("header", 1_200),
                    ("tools", 1_500),
                    ("words", 240),
                    ("recall", 60),
                    ("total", 3_100),
                ]),
                cache_read: 50_000,
                cache_write: 7_000,
            }),
            ..ExchangeReportResult::default()
        };
        let want = [
            "12 turns in private places",
            "  his own messages: 10 turns",
            "  the previous exchange here, of his 9 measured turns: shown 5 (56%), missed 2 (22%; held by \
             a seal or a gate, or a real miss), held after a /new 1 (11%), first here 1 (11%); not \
             measured 1",
            "    missed, where the previous ran: this session 1, another session 1",
            "  he named something the index knows: 3 of 9 turns (33%); not known (index late) 1",
            "    turns naming each kind: commit 1, path 3",
            "  the context sent, mean a turn (est., 10 turns measured): 300 tokens: header 120 (40%), \
             tools 150 (50%), his words 24 (8%), recall 6 (2%); the compiler's own estimate 310; cache \
             read 5000, written 700 a turn",
            "  all groups:",
            "    prev: missed 3 (25%), transcript 9 (75%)",
            "  places bound: dm 1",
        ];
        assert_eq!(report_lines(&r), want.join("\n"));
    }

    /// The answers `ledger::report` gives for his turns, as its own tests assert them: an unmeasured
    /// first turn alone, and beside two measured misses and a measured first turn.
    #[test]
    fn an_unmeasured_first_turn_is_out_of_every_measured_whole() {
        let report = |his_turns, context_turns, previous: &[(&str, u64)]| ExchangeReportResult {
            rows: his_turns,
            answers: Some(ExchangeAnswers {
                his_turns,
                previous: map(previous),
                missed_by_session: map(&[("same", 2)]),
                named: map(&[("none", his_turns)]),
                context_turns,
                context: map(&[
                    ("header", 50 * context_turns),
                    ("total", 60 * context_turns),
                ]),
                ..ExchangeAnswers::default()
            }),
            ..ExchangeReportResult::default()
        };
        let alone = report_lines(&report(1, 0, &[("not_measured", 1)]));
        let lines: Vec<&str> = alone.lines().collect();
        assert_eq!(
            lines[2],
            "  the previous exchange here, of his 0 measured turns: shown 0 (-), missed 0 (-; held by a \
             seal or a gate, or a real miss), held after a /new 0 (-), first here 0 (-); not measured 1"
        );
        assert_eq!(
            lines[4],
            "  the context sent: no turn's first call was measured"
        );
        let beside = report_lines(&report(
            4,
            3,
            &[("not_measured", 1), ("missed", 2), ("first_here", 1)],
        ));
        assert_eq!(
            beside.lines().nth(2),
            Some(
                "  the previous exchange here, of his 3 measured turns: shown 0 (0%), missed 2 (67%; held \
                 by a seal or a gate, or a real miss), held after a /new 0 (0%), first here 1 (33%); not \
                 measured 1"
            )
        );
    }

    /// The groups and kinds `ledger::report` gives for rows whose stored strings it does not list, as
    /// its own test asserts them: the words show as the one word, and nothing else.
    #[test]
    fn an_unlisted_word_shows_as_the_one_word() {
        let r = ExchangeReportResult {
            rows: 4,
            counts: [
                (
                    "situation",
                    map(&[("unlisted", 2), ("none", 1), ("continuation", 1)]),
                ),
                ("judge.kind", map(&[("unlisted", 2), ("follow_up", 1)])),
                ("judge", map(&[("present", 2), ("none", 1)])),
            ]
            .into_iter()
            .map(|(g, m)| (g.to_string(), m))
            .collect(),
            answers: Some(ExchangeAnswers {
                his_turns: 4,
                named: map(&[("some", 3), ("unknown", 1)]),
                named_kinds: map(&[("unlisted", 2), ("path", 3), ("commit", 1)]),
                ..ExchangeAnswers::default()
            }),
            ..ExchangeReportResult::default()
        };
        let text = report_lines(&r);
        assert!(
            text.contains("    turns naming each kind: commit 1, path 3, unlisted 2"),
            "{text}"
        );
        assert!(
            text.contains("    judge.kind: follow_up 1 (33%), unlisted 2 (67%)"),
            "{text}"
        );
        assert!(
            !text.contains("PRIVATE") && !text.contains("task_"),
            "{text}"
        );
    }

    #[test]
    fn an_older_daemon_is_told_to_update() {
        let r = ExchangeReportResult {
            rows: 3,
            ..ExchangeReportResult::default()
        };
        assert_eq!(
            report_lines(&r).lines().nth(1),
            Some("  this daemon does not report the answers; update it")
        );
    }
}
