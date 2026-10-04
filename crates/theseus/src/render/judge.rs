//! The judge's lines (M5 23a): health's `judge:` line, and `theseus judge
//! log`, each judgment from its `judge.call` row. Apart from `render.rs`,
//! whose length the shape budget caps (`scripts/long-files.txt`).

use serde_json::Value;
use theseus_protocol::judge::JudgeHealth;
use theseus_protocol::LedgerEntry;

use super::{fmt_time, push, Line, Tag};

/// Health's `judge:` line, when the daemon says one.
pub(super) fn push_health(o: &mut Vec<Line>, h: Option<&JudgeHealth>) {
    if let Some(h) = h {
        push(o, Tag::Plain, &judge_line(h));
    }
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

/// A rerank's own words (M6 32c): its recall, and whether Jev's order
/// changed what would be admitted (`+1 −1`), or why it fell back to the
/// fused order.
fn rerank(rr: &Value) -> String {
    let recall = rr["recall"].as_str().unwrap_or("?");
    let keys = |k: &str| -> Vec<&str> {
        rr[k]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    };
    let (fused, reranked) = (keys("fused_admitted"), keys("reranked_admitted"));
    let what = match rr["fallback"].as_str() {
        Some(f) => format!("fell back to the fused order ({f})"),
        None if rr["changed"].as_bool() == Some(true) => format!(
            "changed what would be admitted (+{} −{})",
            reranked.iter().filter(|k| !fused.contains(k)).count(),
            fused.iter().filter(|k| !reranked.contains(k)).count()
        ),
        None => "kept what would be admitted".into(),
    };
    format!(
        "recall {recall} · {} notes · {what}",
        rr["eligible"].as_u64().unwrap_or(0).min(20)
    )
}

/// `theseus judge log`: one line a judgment, oldest first: its time, pack
/// and mode, session, what it answered (or why it did not), its cost, and
/// how long Jev took. A rerank says its recall and what its order changed
/// instead of its answers, and its deadline beside its time.
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
            let rr = &d["context"]["rerank"];
            let what = match o["outcome"].as_str() {
                _ if rr.is_object() => rerank(rr),
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
            let deadline = match rr["deadline_ms"].as_u64() {
                Some(ms) => format!(" of {ms}"),
                None => String::new(),
            };
            format!(
                "{} {} ({}) {} · {what} · {cost} · {} ms{deadline}{drift}",
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

    /// A rerank's line names its recall and says whether its order changed
    /// what would be admitted, or why it fell back.
    #[test]
    fn a_reranks_line_names_its_recall_and_what_it_changed() {
        let row = |rerank: Value, outcome: Value, ms: u64| LedgerEntry {
            position: 1,
            at_unix_ms: 3_723_000,
            kind: "judge.call".into(),
            session_id: Some("ses_b".into()),
            turn_id: Some("turn_b".into()),
            data: json!({"pack": "rerank.v1", "mode": "shadow", "outcome": outcome,
                "cost_micros": 120, "timing": {"total_ms": ms}, "context": {"rerank": rerank}}),
        };
        let answered = json!({"outcome": "answered"});
        let changed = row(
            json!({"recall": "rcl_a1", "eligible": 3, "fused_admitted": ["n1#0", "n2#0"],
                "reranked_admitted": ["n3#0", "n2#0"], "changed": true, "fallback": null, "deadline_ms": 600}),
            answered.clone(),
            341,
        );
        let kept = row(
            json!({"recall": "rcl_a2", "eligible": 25, "fused_admitted": ["n1#0"],
                "reranked_admitted": ["n1#0"], "changed": false, "fallback": null, "deadline_ms": 600}),
            answered,
            298,
        );
        let fell = row(
            json!({"recall": "rcl_a3", "eligible": 2, "fused_admitted": ["n1#0"],
                "reranked_admitted": ["n1#0"], "changed": false, "fallback": "timeout", "deadline_ms": 600}),
            json!({"outcome": "failed", "class": "timeout"}),
            601,
        );
        assert_eq!(
            judge_log_lines(&[changed, kept, fell]),
            [
                "01:02:03.000Z rerank.v1 (shadow) ses_b · recall rcl_a1 · 3 notes · changed what would be admitted (+1 −1) · $0.000120 · 341 ms of 600",
                "01:02:03.000Z rerank.v1 (shadow) ses_b · recall rcl_a2 · 20 notes · kept what would be admitted · $0.000120 · 298 ms of 600",
                "01:02:03.000Z rerank.v1 (shadow) ses_b · recall rcl_a3 · 2 notes · fell back to the fused order (timeout) · $0.000120 · 601 ms of 600",
            ]
        );
    }
}
