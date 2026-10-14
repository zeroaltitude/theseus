//! `theseus route report` and `theseus turn replay` in words (own-benchmark,
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
        let mut line = format!(
            "  {} @ {}: {} turns, {}",
            c.model,
            effort(&c.effort),
            c.turns,
            usd(c.spend_usd)
        );
        if c.checked > 0 {
            line += &format!(", you checked {}", c.checked);
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
    out.push(format!(
        "Movable at {:.0}% confidence: {} of {}.",
        r.confidence * 100.0,
        usd(r.total_movable_usd),
        usd(r.total_spend_usd)
    ));
    out.push(match &r.sampler {
        Some(why) => format!("Replays today: {} ({why}).", usd(r.replay_today_usd)),
        None => format!(
            "Replays today: {} of {} ([route] replay_day_usd).",
            usd(r.replay_today_usd),
            usd(r.replay_day_usd)
        ),
    });
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
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::own_bench::RouteCell;

    fn cell() -> RouteCell {
        RouteCell {
            model: "claude-sonnet-5-5".into(),
            effort: "high".into(),
            turns: 40,
            spend_usd: 12.5,
            rung_down: Some("haiku".into()),
            replayed: 9,
            matched: 8,
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
            replay_today_usd: 0.31,
            replay_day_usd: 1.0,
            sampler: None,
        };
        assert_eq!(
            report_lines(&r),
            [
                "Your own work, the last 7 days: $16.50 spent over 2 cells.",
                "  claude-sonnet-5-5 @ high: 40 turns, $12.50, you checked 3; 9 replayed on haiku, 8 matched (compare.v1 same or better at 80%+); at 90% confidence at least 63% match, the replays cost 20% of the originals: $6.30 movable",
                "  claude-fable-5-1 @ default: 2 turns, $4.00; not replayed yet",
                "Movable at 90% confidence: $6.30 of $16.50.",
                "Replays today: $0.3100 of $1.00 ([route] replay_day_usd).",
            ]
        );
    }
}
