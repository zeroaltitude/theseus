//! `theseus ledger exchanges` and `theseus ledger rebuild-exchanges` (issue
//! 44, step 1): the exchange ledger's measuring rows summed into counts and shares,
//! and the exchanges of older history written from the conversations' nodes
//! (`--dry-run` counts and writes nothing). Ids, kinds and counts only. And
//! `theseus ledger ladder` (step 4): a place's `did` lines and fold nodes built
//! from its history, with a dry run that counts and prices, and a cap. And
//! `theseus ledger gists`: those lines and nodes beside what their writer
//! read, a sheet for the owner to mark. And
//! `theseus ledger topics` (issue 46): the history from before topics were
//! filed per exchange, filed under them once; counts and dollars only.

use std::collections::BTreeMap;

use anyhow::Result;
use theseus_client::render::time::fmt_day;
use theseus_client::render::time::fmt_stamp;
use theseus_client::Conn;
use theseus_protocol::exchange::{
    ExchangeAnswers, ExchangeGistsParams, ExchangeGistsResult, ExchangeLadderParams,
    ExchangeLadderResult, ExchangeRebuildParams, ExchangeRebuildResult, ExchangeReportParams,
    ExchangeReportResult, ExchangeTopicsParams, ExchangeTopicsResult,
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
    Ok(serde_json::to_value(ExchangeTopicsParams {
        dry_run,
        cap_usd,
    })?)
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
    if r.labels_passed_over > 0 {
        s.push_str(&format!(
            "; {} records under labels. could not be read as item labels: left as they are",
            r.labels_passed_over
        ));
    }
    if r.touches_passed_over > 0 {
        s.push_str(&format!(
            "; {} records under the touches' prefix could not be read as touches: left as they are",
            r.touches_passed_over
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

/// The reading sheet: what exists and what was drawn, the seed, what its material is and what a sample
/// holds, fewer than asked said so, then each item numbered: its warnings, its writer's material rebuilt
/// today, it as the view shows it, and a blank verdict.
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
        "Each item's writer's material is rebuilt today, at that writer's cuts, from what the store \
         holds now, with the secret values known today substituted as the writer does: it is not a copy \
         of what the writer was given (none is kept), and today's substitutions cannot prove an \
         earlier day's."
            .to_string(),
        "A sample holds summary lines and first-level fold nodes that exist, from every place when \
         none is named: it can include exchanges opened by a guest or by an agent's process, and \
         lines from before a /new."
            .to_string(),
        "Mark each one: wrong number/name/choice, other error, or ok. An item marked !! may not show \
         what its writer was given: skip it, or judge it knowing that. Nothing here is stored; keep \
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
        out.extend(l.notes.iter().map(|n| format!("!! {n}")));
        out.push("-- Its writer's material, rebuilt today:".into());
        out.push(
            l.read
                .clone()
                .unwrap_or_else(|| "(its exchange can no longer be read)".into()),
        );
        out.push("-- The summary, as the view shows it:".into());
        out.push(l.line.clone());
        out.push(verdict(&l.notes).into());
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
        out.extend(n.notes.iter().map(|n| format!("!! {n}")));
        out.push("-- Its writer's material, rebuilt today:".into());
        out.push(n.read.trim_end().to_string());
        out.push("-- The node, as the view shows it:".into());
        out.push(n.shown.clone());
        out.push(verdict(&n.notes).into());
    }
    out.join("\n")
}

/// An item's blank verdict: an ordinary one only where nothing warns that its material may differ.
fn verdict(notes: &[String]) -> &'static str {
    match notes.is_empty() {
        true => "Verdict: ____",
        false => "Verdict (skip it, or judge it knowing the warning above): ____",
    }
}

/// A filing of the history in two lines: what the ledger holds, and what the
/// run would ask or did. Counts and dollars, never a message's words.
pub fn topics_lines(r: &ExchangeTopicsResult) -> String {
    let cap = cap_dollars(r.cap_usd);
    let mut s = match r.dry_run {
        true => format!("a dry run for a cap of ${cap} a run: nothing is asked\n"),
        false => format!("a run with a cap of ${cap}\n"),
    };
    s.push_str(&format!(
        "{} exchanges: {} already touch a topic, {} are not yours, {} were asked before, {} open \
         with no message of yours, {} are in places the ontology's judge does not act on now",
        r.exchanges, r.touched_before, r.not_owner, r.asked_before, r.no_message, r.not_acting
    ));
    match r.dry_run {
        true => s.push_str(&format!(
            "\nwould ask {n}: ${:.4} at worst for all {n} of them (it may be more than the cap); a run \
             stops at its cap of ${cap}",
            r.worst_usd,
            n = r.to_ask
        )),
        false => s.push_str(&format!(
            "\nasked {} of {} ({} answered): {} exchanges filed with {} touches; spent ${:.4} of a \
             cap of ${cap}",
            r.asked, r.to_ask, r.answered, r.filed, r.touches, r.spent_usd
        )),
    }
    if let Some(why) = &r.stopped {
        s.push_str(&format!("\nstopped: {why}"));
    }
    s
}

/// A cap's dollars: to the cent from a cent up, and under a cent with the
/// decimals it has, so a cap of $0.000001 is never shown as $0.00.
fn cap_dollars(usd: f64) -> String {
    match usd >= 0.01 {
        true => format!("{usd:.2}"),
        false => format!("{usd}"),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use theseus_protocol::exchange::{ExchangeAnswers, ExchangeReportResult};

    use super::report_lines;

    use super::*;
    use theseus_protocol::exchange::{GistLine, GistNode};

    fn map(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
        pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
    }

    /// A rebuild that passed over records under `labels.` it could not read as item labels says how many,
    /// and that they were left as they are (issue 44); one that passed over none says nothing of them.
    #[test]
    fn a_rebuild_says_how_many_records_under_labels_it_passed_over() {
        let of =
            |v: serde_json::Value| -> ExchangeRebuildResult { serde_json::from_value(v).unwrap() };
        let line = rebuild_lines(&of(
            serde_json::json!({"sessions": 1, "labels_passed_over": 3}),
        ));
        assert!(
            line.ends_with(
                "; 3 records under labels. could not be read as item labels: left as they are"
            ),
            "{line}"
        );
        let none = rebuild_lines(&of(serde_json::json!({"sessions": 1})));
        assert!(!none.contains("labels."), "{none}");
    }

    /// A rebuild that passed over touch records it could not read says how many, and that they were left as
    /// they are (issue 46); one that passed over none says nothing of them.
    #[test]
    fn a_rebuild_says_how_many_touch_records_it_passed_over() {
        let of =
            |v: serde_json::Value| -> ExchangeRebuildResult { serde_json::from_value(v).unwrap() };
        let line = rebuild_lines(&of(
            serde_json::json!({"sessions": 1, "touches_passed_over": 2}),
        ));
        assert!(
            line.ends_with(
                "; 2 records under the touches' prefix could not be read as touches: left as they are"
            ),
            "{line}"
        );
        let none = rebuild_lines(&of(serde_json::json!({"sessions": 1})));
        assert!(!none.contains("touches' prefix"), "{none}");
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
                notes: Vec::new(),
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
        assert!(text.contains(
            "-- Its writer's material, rebuilt today:\nThe person's message:\nRecalibrate gauge 4"
        ));
        assert!(text.contains("-- The summary, as the view shows it:\nRecalibrated gauge 4."));
        assert!(
            text.contains("20 exchanges (19 still readable; code, writer 0)"),
            "{text}"
        );
        assert!(text.contains("-- The node, as the view shows it:\n\"Tide gauges\""));
        assert_eq!(text.matches("Verdict: ____").count(), 2);
    }

    /// The sheet says its material is rebuilt today and substituted as today's writer would, and an
    /// item's warning is printed before its material, with a verdict that does not invite an ordinary one.
    #[test]
    fn gists_prints_an_older_writers_warning_on_its_item() {
        theseus_client::render::time::pin_for_tests();
        let warning =
            "Written by an older writer (version 1; today's is 2): the material below is rebuilt \
                       today and may not match what that writer was given."
                .to_string();
        let gone = "1 of its 20 exchanges is no longer in the store.".to_string();
        let r = ExchangeGistsResult {
            seed: 7,
            lines_total: 2,
            nodes_total: 1,
            lines: vec![
                GistLine {
                    turn_id: "trn_old".into(),
                    writer: 1,
                    read: Some("The person's message:\nRecalibrate gauge 4".into()),
                    line: "Recalibrated gauge 4.".into(),
                    notes: vec![warning.clone()],
                    ..GistLine::default()
                },
                GistLine {
                    turn_id: "trn_new".into(),
                    writer: 2,
                    read: Some("The person's message:\nRecalibrate gauge 5".into()),
                    line: "Recalibrated gauge 5.".into(),
                    ..GistLine::default()
                },
            ],
            nodes: vec![GistNode {
                id: "fld_old".into(),
                writer: 1,
                read: "Exchange 1:\nThe person's message: Recalibrate gauge 4".into(),
                shown: "\"Tide gauges\"".into(),
                notes: vec![warning.clone(), gone.clone()],
                ..GistNode::default()
            }],
            ..ExchangeGistsResult::default()
        };
        let text = gists_lines(&ExchangeGistsParams::default(), &r);
        for said in [
            "rebuilt today",
            "not a copy of what the writer was given",
            "today's substitutions cannot prove an earlier day's",
            "opened by a guest or by an agent's process",
            "lines from before a /new",
        ] {
            assert!(text.contains(said), "{said:?} in {text}");
        }
        assert!(
            !text.contains("the writer's error"),
            "no verdict is the writer's by construction: {text}"
        );
        let old = text.find("trn_old").unwrap();
        let warned = text.find(&format!("!! {warning}")).unwrap();
        let material = text.find("Recalibrate gauge 4").unwrap();
        assert!(
            old < warned && warned < material,
            "the warning comes first: {text}"
        );
        assert!(text.contains(&format!("!! {gone}")), "{text}");
        assert_eq!(
            text.matches("Verdict (skip it, or judge it knowing the warning above): ____")
                .count(),
            2,
            "{text}"
        );
        assert_eq!(text.matches("Verdict: ____").count(), 1, "{text}");
    }

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
        // A cap under a cent is shown with its decimals, never as $0.00;
        // one of a cent or more, to the cent.
        let small = ExchangeTopicsResult {
            cap_usd: 0.000001,
            stopped: None,
            ..run
        };
        assert_eq!(
            topics_lines(&small),
            "a run with a cap of $0.000001\n9 exchanges: 2 already touch a topic, 1 are not yours, \
             1 were asked before, 2 open with no message of yours, 0 are in places the \
             ontology's judge does not act on now\nasked 1 of 3 (1 answered): 1 exchanges filed \
             with 2 touches; spent $0.0003 of a cap of $0.000001"
        );
        let dry_small = ExchangeTopicsResult {
            cap_usd: 0.0025,
            ..dry.clone()
        };
        assert_eq!(
            topics_lines(&dry_small),
            "a dry run for a cap of $0.0025 a run: nothing is asked\n9 exchanges: 2 already touch \
             a topic, 1 are not yours, 1 were asked before, 2 open with no message of yours, 0 are \
             in places the ontology's judge does not act on now\nwould ask 3: $0.0012 at worst \
             for all 3 of them (it may be more than the cap); a run stops at its cap of $0.0025"
        );
        let cent = ExchangeTopicsResult {
            cap_usd: 0.01,
            ..dry
        };
        assert!(
            topics_lines(&cent).starts_with("a dry run for a cap of $0.01 a run"),
            "{}",
            topics_lines(&cent)
        );
    }

    /// The cap is read before a request is built: NaN, the infinities, zero,
    /// a negative number and any sum under $0.000001 (not as it rounds) are
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
            "NaN",
            "nan",
            "inf",
            "-inf",
            "infinity",
            "0",
            "-0",
            "-1",
            "0.0000004",
            "0.0000005",
            "0.00000075",
            "0.00000099",
            "ten",
        ] {
            assert!(parse(Some(bad)).is_err(), "{bad} was parsed");
        }
        let bad = [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -1.0,
            4e-7,
            5e-7,
            7.5e-7,
            9.9e-7,
        ];
        for usd in bad {
            assert!(topics_params(false, Some(usd)).is_err(), "{usd} was built");
        }
        let good = [
            (None, json!({"dry_run": false})),
            (Some("2.5"), json!({"dry_run": false, "cap_usd": 2.5})),
            (
                Some("0.000001"),
                json!({"dry_run": false, "cap_usd": 0.000001}),
            ),
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
