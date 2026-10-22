//! `theseus model bench` and `theseus model replay` in words (own-benchmark,
//! theseus-pw1q.12): pure functions of the daemon's answers.

use theseus_protocol::own_bench::{RouteReportResult, TurnReplayResult};

use super::postmortem::usd;

fn effort(e: &str) -> &str {
    if e.is_empty() {
        "default"
    } else {
        e
    }
}

/// The report: a line a cell, then the totals and the cap.
pub fn report_lines(r: &RouteReportResult) -> Vec<String> {
    let span = if r.window_days > 1 {
        format!("the last {} days", r.window_days)
    } else {
        "today".to_string()
    };
    let mut out = vec![format!(
        "Your own work, {span}: {} spent over {} cells.",
        usd(r.total_spend_usd),
        r.cells.len()
    )];
    if r.cells.is_empty() {
        out.push("  no turns in the window.".into());
    }
    for c in &r.cells {
        let origin = match c.origin.as_str() {
            "" => String::new(),
            o => format!(" [{o}]"),
        };
        let mut line = format!(
            "  {} @ {}{origin}: {} turns, {}",
            c.model,
            effort(&c.effort),
            c.turns,
            usd(c.spend_usd)
        );
        if c.checked > 0 {
            line += &format!(", you checked {}", c.checked);
        }
        if c.whole_replayed > 0 {
            line += &format!(
                "; whole turns: {} replayed, {} matched, {} with the same tests",
                c.whole_replayed, c.whole_matched, c.tests_same
            );
        }
        match (&c.rung_down, c.replayed) {
            (Some(to), n) if n > 0 => {
                line += &format!(
                    "; {n} replayed on {to}, {} matched (compare.v1 same or better at {:.0}%+)",
                    c.matched,
                    r.bar * 100.0
                );
                if let (Some(floor), Some(ratio)) = (c.match_floor, c.cost_ratio) {
                    line += &format!(
                        "; at {:.0}% confidence at least {:.0}% match, the replays cost {:.0}% of the originals: {} movable",
                        r.confidence * 100.0,
                        floor * 100.0,
                        ratio * 100.0,
                        usd(c.movable_usd)
                    );
                }
            }
            (Some(to), _) => line += &format!("; replays on {to} not judged yet"),
            (None, _) => line += "; not replayed yet",
        }
        out.push(line);
    }
    out.extend(data_lines(r));
    out.extend(draft_lines(r));
    out.push(format!(
        "Movable at {:.0}% confidence: {} of {}.",
        r.confidence * 100.0,
        usd(r.total_movable_usd),
        usd(r.total_spend_usd)
    ));
    if r.labels_fed > 0 {
        out.push(format!(
            "Machine labels fed to routing's learning from whole-turn replays: {} (train split only).",
            r.labels_fed
        ));
    }
    out.push(match &r.sampler {
        Some(why) => format!("Replays today: {} ({why}).", usd(r.replay_today_usd)),
        None => format!(
            "Replays today: {} of {} ([route_bench] replay_day_usd).",
            usd(r.replay_today_usd),
            usd(r.replay_day_usd)
        ),
    });
    out
}

/// The data rule's columns (theseus-a1wvq.11): a line per origin and Jev's
/// data verdict, and one of the gate's skips by reason. Nothing when every
/// turn is from before routing recorded its origin.
fn data_lines(r: &RouteReportResult) -> Vec<String> {
    if r.data.iter().all(|d| d.origin == "unrecorded") {
        return Vec::new();
    }
    let mut out: Vec<String> = r
        .data
        .iter()
        .map(|d| {
            let turns = format!("{} turns, {}", d.turns, usd(d.spend_usd));
            format!("  {} · data {}: {turns}", d.origin, d.data)
        })
        .collect();
    if !r.gates.is_empty() {
        let why = r.gates.iter().map(|g| format!("{} {}", g.why, g.turns));
        let why = why.collect::<Vec<_>>().join(", ");
        out.push(format!("  kept off a restricted-origin model: {why}"));
    }
    out
}

/// The draft columns (theseus-a1wvq.12): a line per draft and review model
/// pair, and one line of the turns whose class drafts that did not, by why.
/// Nothing when both lists are empty.
fn draft_lines(r: &RouteReportResult) -> Vec<String> {
    let mut out: Vec<String> = r
        .drafts
        .iter()
        .map(|d| {
            let review = if d.review_model == "none" {
                "no review".to_string()
            } else {
                d.review_model.clone()
            };
            format!(
                "  draft {} \u{2192} {review}: {} drafts, {} applied, {} + {} = {}, {} lines changed",
                d.draft_model,
                d.drafts,
                d.applied,
                usd(d.draft_usd),
                usd(d.review_usd),
                usd(d.total_usd),
                d.lines_changed
            )
        })
        .collect();
    if !r.draft_skips.is_empty() {
        let why = r
            .draft_skips
            .iter()
            .map(|g| format!("{} {}", g.why, g.turns));
        out.push(format!(
            "  draft skips: {}",
            why.collect::<Vec<_>>().join(" \u{b7} ")
        ));
    }
    out
}

/// A replay: both replies' shapes, the verdict, the price.
pub fn replay_lines(r: &TurnReplayResult) -> Vec<String> {
    let mut out = vec![format!(
        "Turn {} replayed on {} ({}{}) in {} ms, {}; the request was the original byte for byte ({}).",
        r.turn,
        r.profile,
        r.replay.model,
        r.effort
            .as_deref()
            .map(|e| format!(", {e}"))
            .unwrap_or_default(),
        r.elapsed_ms,
        usd(r.cost_usd),
        &r.request_digest[..r.request_digest.len().min(12)],
    )];
    let side = |name: &str, x: &theseus_protocol::own_bench::ReplayReply| {
        let calls = if x.tool_calls.is_empty() {
            String::new()
        } else {
            format!(" [asked to call: {}]", x.tool_calls.join(", "))
        };
        format!(
            "  {name} ({}, {} out, {}): {}{calls}",
            x.model,
            x.output_tokens,
            usd(x.cost_usd),
            x.text.trim().replace('\n', " ")
        )
    };
    out.push(side("original", &r.original));
    out.push(side("replay  ", &r.replay));
    out.push(match (&r.verdict, &r.unjudged) {
        (Some(v), _) => format!(
            "  compare.v1 ({}): {} ({:.0}%); same or better {:.0}%.",
            v.mode,
            v.verdict,
            v.p * 100.0,
            v.same_or_better * 100.0
        ),
        (None, Some(why)) => format!("  not judged: {why}."),
        (None, None) => "  not judged (--no-compare).".into(),
    });
    if let Some(w) = &r.whole {
        out.extend(whole_lines(w));
    }
    out
}

/// What a whole-turn replay did beyond its reply.
fn whole_lines(w: &theseus_protocol::own_bench::WholeReplay) -> Vec<String> {
    let mut out = Vec::new();
    out.push(format!(
        "  whole turn: {} model calls{}; {}",
        w.loops,
        w.capped
            .as_deref()
            .map(|c| format!(" (stopped at its {c} cap)"))
            .unwrap_or_default(),
        if w.calls.is_empty() {
            "no tool calls".to_string()
        } else {
            w.calls
                .iter()
                .map(|c| match (c.exit, &c.why) {
                    (Some(e), _) => format!("{} {} (exit {e})", c.tool, c.how),
                    (None, Some(why)) => format!("{} {}: {why}", c.tool, c.how),
                    (None, None) => format!("{} {}", c.tool, c.how),
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    ));
    let paths = |p: &[String]| {
        if p.is_empty() {
            "none".to_string()
        } else {
            p.join(", ")
        }
    };
    let word = |b: bool| if b { "same" } else { "differ" };
    out.push(format!(
        "  paths changed: {} (original: {}); tests {}; paths {}; exit codes {}.",
        paths(&w.changed),
        paths(&w.original_changed),
        w.same.tests.map_or("not run on either side", word),
        word(w.same.files),
        word(w.same.exits),
    ));
    for (name, runs) in [("original", &w.original_tests), ("replay", &w.tests)] {
        for t in runs {
            out.push(format!(
                "  {name} test run: {} -> {}",
                t.argv.join(" "),
                t.exit
                    .map_or("no exit".to_string(), |e| format!("exit {e}"))
            ));
        }
    }
    for c in &w.caveats {
        out.push(format!("  caveat: {c}"));
    }
    if let Some(l) = &w.label {
        out.push(format!("  routing label: {l}"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::own_bench::RouteCell;

    fn cell() -> RouteCell {
        RouteCell {
            model: "claude-sonnet-5-5".into(),
            origin: String::new(),
            effort: "high".into(),
            turns: 40,
            spend_usd: 12.5,
            rung_down: Some("haiku".into()),
            replayed: 9,
            matched: 8,
            whole_replayed: 0,
            whole_matched: 0,
            tests_same: 0,
            checked: 3,
            cost_ratio: Some(0.2),
            match_floor: Some(0.63),
            movable_usd: 6.3,
            replay_usd: 0.31,
        }
    }

    /// The report, line by line, from planted numbers.
    #[test]
    fn the_report_says_each_cell_and_the_cap() {
        let r = RouteReportResult {
            window_days: 7,
            bar: 0.8,
            confidence: 0.9,
            cells: vec![
                cell(),
                RouteCell {
                    model: "claude-fable-5-1".into(),
                    turns: 2,
                    spend_usd: 4.0,
                    ..RouteCell::default()
                },
            ],
            total_spend_usd: 16.5,
            total_movable_usd: 6.3,
            labels_fed: 0,
            replay_today_usd: 0.31,
            replay_day_usd: 1.0,
            sampler: None,
            data: vec![],
            gates: vec![],
            drafts: vec![],
            draft_skips: vec![],
        };
        assert_eq!(
            report_lines(&r),
            [
                "Your own work, the last 7 days: $16.50 spent over 2 cells.",
                "  claude-sonnet-5-5 @ high: 40 turns, $12.50, you checked 3; 9 replayed on haiku, 8 matched (compare.v1 same or better at 80%+); at 90% confidence at least 63% match, the replays cost 20% of the originals: $6.30 movable",
                "  claude-fable-5-1 @ default: 2 turns, $4.00; not replayed yet",
                "Movable at 90% confidence: $6.30 of $16.50.",
                "Replays today: $0.31 of $1.00 ([route_bench] replay_day_usd).",
            ]
        );
    }

    /// A whole-turn cell says its columns and the label count, and a whole
    /// replay says what ran, what was stubbed, the facts and the label.
    #[test]
    fn the_whole_turn_columns_and_a_whole_replay_are_worded() {
        use theseus_protocol::own_bench::{
            TestRun, TurnReplayResult, WholeCall, WholeReplay, WholeSame,
        };
        let r = RouteReportResult {
            window_days: 1,
            bar: 0.8,
            confidence: 0.9,
            cells: vec![RouteCell {
                whole_replayed: 4,
                whole_matched: 3,
                tests_same: 2,
                ..cell()
            }],
            total_spend_usd: 12.5,
            total_movable_usd: 6.3,
            labels_fed: 2,
            replay_today_usd: 0.31,
            replay_day_usd: 1.0,
            sampler: None,
            data: vec![],
            gates: vec![],
            drafts: vec![],
            draft_skips: vec![],
        };
        let lines = report_lines(&r);
        assert!(lines[1].contains("; whole turns: 4 replayed, 3 matched, 2 with the same tests"));
        assert!(lines.contains(
            &"Machine labels fed to routing's learning from whole-turn replays: 2 (train split only)."
                .to_string()
        ));
        let w = WholeReplay {
            loops: 3,
            capped: Some("loops".into()),
            calls: vec![
                WholeCall {
                    tool: "fs_edit".into(),
                    how: "ran".into(),
                    ..Default::default()
                },
                WholeCall {
                    tool: "proc_run".into(),
                    how: "ran".into(),
                    exit: Some(0),
                    ..Default::default()
                },
                WholeCall {
                    tool: "wake_at".into(),
                    how: "stub".into(),
                    ..Default::default()
                },
            ],
            changed: vec!["a.rs".into()],
            original_changed: vec!["a.rs".into()],
            tests: vec![TestRun {
                argv: vec!["cargo".into(), "test".into()],
                exit: Some(0),
            }],
            original_tests: vec![TestRun {
                argv: vec!["cargo".into(), "test".into()],
                exit: Some(101),
            }],
            same: WholeSame {
                tests: Some(false),
                files: true,
                exits: false,
            },
            caveats: vec!["2 programs ran".into()],
            label: Some("none: the replay ended at its loops cap".into()),
            ..WholeReplay::default()
        };
        let got = whole_lines(&w);
        assert_eq!(
            got,
            [
                "  whole turn: 3 model calls (stopped at its loops cap); fs_edit ran, proc_run ran (exit 0), wake_at stub",
                "  paths changed: a.rs (original: a.rs); tests differ; paths same; exit codes differ.",
                "  original test run: cargo test -> exit 101",
                "  replay test run: cargo test -> exit 0",
                "  caveat: 2 programs ran",
                "  routing label: none: the replay ended at its loops cap",
            ]
        );
        let _ = TurnReplayResult::default();
    }

    /// The data rule's lines (theseus-a1wvq.11): the origin on each cell, a
    /// line per origin and verdict, one of the gate's skips; and nothing more
    /// when every turn is from before the keys.
    #[test]
    fn the_report_says_origin_data_and_gate_skips() {
        use theseus_protocol::own_bench::{DataCell, GateCount};
        let data = |origin: &str, data: &str, turns: u64, spend_usd: f64| DataCell {
            origin: origin.into(),
            data: data.into(),
            turns,
            spend_usd,
        };
        let mut r = RouteReportResult {
            window_days: 1,
            cells: vec![RouteCell {
                model: "glm-x".into(),
                origin: "cn".into(),
                turns: 3,
                spend_usd: 0.3,
                ..RouteCell::default()
            }],
            data: vec![data("cn", "open", 3, 0.3), data("us", "private", 1, 1.0)],
            gates: vec![GateCount {
                why: "verdict_private".into(),
                turns: 1,
            }],
            ..RouteReportResult::default()
        };
        let lines = report_lines(&r);
        assert_eq!(
            lines[1],
            "  glm-x @ default [cn]: 3 turns, $0.30; not replayed yet"
        );
        assert_eq!(lines[2], "  cn \u{b7} data open: 3 turns, $0.30");
        assert_eq!(lines[3], "  us \u{b7} data private: 1 turns, $1.00");
        assert_eq!(
            lines[4],
            "  kept off a restricted-origin model: verdict_private 1"
        );
        r.data = vec![data("unrecorded", "none", 3, 0.3)];
        r.gates.clear();
        assert!(report_lines(&r)[2].starts_with("Movable"));
    }

    /// The draft columns (theseus-a1wvq.12): a cell line and the skips line
    /// after the data lines; a report with neither is what it was.
    #[test]
    fn the_report_words_drafts_and_their_skips_and_is_unchanged_without() {
        use theseus_protocol::own_bench::{DraftCell, GateCount};
        let plain = RouteReportResult {
            window_days: 1,
            cells: vec![cell()],
            ..RouteReportResult::default()
        };
        let before = report_lines(&plain);
        assert!(before.iter().all(|l| !l.contains("draft")));
        let r = RouteReportResult {
            drafts: vec![DraftCell {
                draft_model: "dsflash".into(),
                review_model: "sonnet".into(),
                drafts: 4,
                applied: 3,
                reviewed: 4,
                draft_usd: 0.19,
                review_usd: 0.88,
                lines_changed: 212,
                total_usd: 1.07,
            }],
            draft_skips: vec![
                GateCount {
                    why: "dir_not_open".into(),
                    turns: 5,
                },
                GateCount {
                    why: "not_git".into(),
                    turns: 1,
                },
            ],
            ..plain
        };
        let lines = report_lines(&r);
        let at = lines
            .iter()
            .position(|l| l.starts_with("  draft "))
            .unwrap();
        assert_eq!(
            lines[at],
            "  draft dsflash \u{2192} sonnet: 4 drafts, 3 applied, $0.19 + $0.88 = $1.07, 212 lines changed"
        );
        assert_eq!(
            lines[at + 1],
            "  draft skips: dir_not_open 5 \u{b7} not_git 1"
        );
        let rest: Vec<_> = lines
            .iter()
            .filter(|l| !l.contains("draft"))
            .cloned()
            .collect();
        assert_eq!(rest, before);
        let skips_only = RouteReportResult {
            drafts: vec![],
            ..r
        };
        assert!(report_lines(&skips_only)
            .iter()
            .any(|l| l.starts_with("  draft skips:")));
    }
}
