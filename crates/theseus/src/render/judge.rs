//! The judge's lines (M5 23a): health's `judge:` line, and `theseus judge
//! log`, each judgment from its `judge.call` row; and (step 24) a notified
//! call's score, the line under its notice. Apart from `render.rs`, whose
//! length the shape budget caps (`scripts/long-files.txt`).

use serde_json::Value;
use theseus_protocol::judge::{JudgeHealth, JudgeScored};
use theseus_protocol::LedgerEntry;

use super::{fmt_time, push, Line, Tag};

/// Health's `judge:` line, when the daemon says one.
pub(super) fn push_health(o: &mut Vec<Line>, h: Option<&JudgeHealth>) {
    if let Some(h) = h {
        push(o, Tag::Plain, &judge_line(h));
    }
}

/// A notified call's score, after its notice's two lines: `  ! notified:
/// proc.run · risk 12% (shadow)`. The notice's line gains it as the
/// judgment lands; a stream can only follow it.
pub fn scored_line(j: &JudgeScored) -> String {
    format!("  ! notified: {} · {}", j.tool, j.line())
}

/// Dollars as a judgment's are: micro-dollars show, so six places.
fn usd(v: f64) -> String {
    format!("${v:.6}")
}

/// `judge: off`, or `judge: loop.v1: shadow · breaker closed · 3 calls
/// today (1 failed, 2 skipped) · $0.000267 of $1.00 today`, with `paused
/// until midnight` at the day's limit.
pub fn judge_line(h: &JudgeHealth) -> String {
    if !h.enabled {
        return "judge: off ([judge] enabled = false)".into();
    }
    let mut s = format!(
        "judge: {} · max {} · breaker {}",
        h.packs.join(", "),
        h.max_mode,
        h.breaker
    );
    if h.in_flight > 0 {
        s.push_str(&format!(" · {} in flight", h.in_flight));
    }
    s.push_str(&format!(
        " · {} today ({} failed, {} skipped) · {} of ${:.2} today",
        match h.calls_today {
            1 => "1 call".to_string(),
            n => format!("{n} calls"),
        },
        h.failed_today,
        h.skipped_today,
        usd(h.spend_today_usd),
        h.shadow_limit_usd
    ));
    if h.paused {
        s.push_str(" · shadow paused at the day's limit until midnight");
    }
    s
}

/// One answer, short: `work_state=complete 0.95 act`, `announced_unfinished=no 0.04 act`.
fn answer(a: &Value) -> String {
    let q = a["question"].as_str().unwrap_or("?");
    let band = a["band"]["band"].as_str().unwrap_or("?");
    let ans = &a["answer"];
    match ans["type"].as_str() {
        Some("choice") => format!(
            "{q}={} {:.2} {band}",
            ans["choice"].as_str().unwrap_or("?"),
            ans["confidence"].as_f64().unwrap_or(0.0)
        ),
        Some("score") => format!(
            "{q}={:.2} {:.2} {band}",
            ans["score"].as_f64().unwrap_or(0.0),
            ans["confidence"].as_f64().unwrap_or(0.0)
        ),
        Some("noul") => {
            let p = ans["noul"].as_f64().unwrap_or(0.0);
            format!("{q}={} {p:.2} {band}", if p >= 0.5 { "yes" } else { "no" })
        }
        _ => format!("{q}=?"),
    }
}

/// `theseus judge log`: one line a judgment, oldest first: its time, pack
/// and mode, session, what it answered (or why it did not), its cost, and
/// how long Jev took.
pub fn judge_log_lines(rows: &[LedgerEntry]) -> Vec<String> {
    if rows.is_empty() {
        return vec![
            "no judgments yet (the judge is off, or no turn has ended since it was on)".into(),
        ];
    }
    rows.iter()
        .map(|r| {
            let d = &r.data;
            let o = &d["outcome"];
            let what = match o["outcome"].as_str() {
                Some("answered") => d["answers"]
                    .as_array()
                    .map(|a| a.iter().map(answer).collect::<Vec<_>>().join(" · "))
                    .unwrap_or_default(),
                Some("skipped") => format!("skipped: {}", o["reason"].as_str().unwrap_or("?")),
                Some("failed") => format!("failed: {}", o["class"].as_str().unwrap_or("?")),
                _ => "?".into(),
            };
            let cost = match d["cost_micros"].as_u64() {
                Some(m) => usd(m as f64 / 1_000_000.0),
                None => "no cost".into(),
            };
            let drift = if d["model_drift"].as_bool() == Some(true) {
                format!(
                    " · answered by {} (drift)",
                    d["answered_by"].as_str().unwrap_or("?")
                )
            } else {
                String::new()
            };
            format!(
                "{} {} ({}) {} · {what} · {cost} · {} ms{drift}",
                fmt_time(r.at_unix_ms),
                d["pack"].as_str().unwrap_or("?"),
                d["mode"].as_str().unwrap_or("?"),
                r.session_id.as_deref().unwrap_or("-"),
                d["timing"]["total_ms"].as_u64().unwrap_or(0)
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A notified call's score follows its notice on the CLI's stream, as
    /// `ask` and `watch` render it (M5 step 24).
    #[test]
    fn a_notices_score_follows_it_as_risk_n_percent_in_shadow() {
        let j = JudgeScored {
            tool: "proc.run".into(),
            mode: "shadow".into(),
            risky: 0.123,
            percent: theseus_protocol::judge::percent(0.123),
            ..Default::default()
        };
        let lines = crate::render::event(
            &theseus_protocol::Event::JudgeScored(j),
            crate::render::Show::default(),
        );
        let text: Vec<&str> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(text, ["  ! notified: proc.run · risk 12% (shadow)"]);
    }

    #[test]
    fn the_judge_line_says_off_or_its_counts() {
        assert_eq!(
            judge_line(&JudgeHealth::default()),
            "judge: off ([judge] enabled = false)"
        );
        let h = JudgeHealth {
            enabled: true,
            max_mode: "live".into(),
            packs: vec!["loop.v1: shadow".into()],
            breaker: "closed".into(),
            calls_today: 3,
            failed_today: 1,
            skipped_today: 2,
            spend_today_usd: 0.000267,
            shadow_limit_usd: 1.0,
            ..JudgeHealth::default()
        };
        assert_eq!(
            judge_line(&h),
            "judge: loop.v1: shadow · max live · breaker closed · 3 calls today (1 failed, 2 skipped) · $0.000267 of $1.00 today"
        );
    }

    #[test]
    fn a_judgments_line_says_its_answers_or_why_it_has_none() {
        let row = |data: Value| LedgerEntry {
            position: 1,
            at_unix_ms: 3_723_000,
            kind: "judge.call".into(),
            session_id: Some("ses_a".into()),
            turn_id: Some("turn_a".into()),
            data,
        };
        let answered = row(
            json!({"pack": "loop.v1", "mode": "shadow", "outcome": {"outcome": "answered"},
            "answers": [
                {"question": "work_state", "answer": {"type": "choice", "choice": "complete", "confidence": 0.95}, "band": {"band": "act"}},
                {"question": "announced_unfinished", "answer": {"type": "noul", "noul": 0.04}, "band": {"band": "act"}}
            ], "cost_micros": 89, "timing": {"total_ms": 412}}),
        );
        let failed = row(json!({"pack": "loop.v1", "mode": "shadow",
            "outcome": {"outcome": "failed", "class": "timeout"}, "timing": {"total_ms": 1000}}));
        assert_eq!(
            judge_log_lines(&[answered, failed]),
            [
                "01:02:03.000Z loop.v1 (shadow) ses_a · work_state=complete 0.95 act · announced_unfinished=no 0.04 act · $0.000089 · 412 ms",
                "01:02:03.000Z loop.v1 (shadow) ses_a · failed: timeout · no cost · 1000 ms",
            ]
        );
    }
}
