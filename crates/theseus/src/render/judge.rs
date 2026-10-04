//! The judge's lines (M5 23a): health's `judge:` line, and `theseus judge
//! log`, each judgment from its `judge.call` row; and (step 24) a notified
//! call's score, the line under its notice. Apart from `render.rs`, whose
//! length the shape budget caps (`scripts/long-files.txt`).

use serde_json::Value;
use theseus_protocol::judge::{JudgeGetResult, JudgeHealth, JudgeScored};
use theseus_protocol::{LedgerEntry, Span};

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
    // A breaker of its own (32d), `rerank: closed`: `rerank breaker closed`.
    for b in &h.breakers {
        match b.split_once(": ") {
            Some((name, state)) => s.push_str(&format!(" · {name} breaker {state}")),
            None => s.push_str(&format!(" · {b}")),
        }
    }
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
    if h.shed > 0 {
        s.push_str(&format!(" · {} shed", h.shed));
    }
    if !h.key.is_empty() && h.key != "ready" {
        s.push_str(&format!(" · key {}", h.key));
    }
    s
}

/// A turn's span that marks a judgment's dispatch (M5 23b).
pub(super) fn is_mark(s: &Span) -> bool {
    s.name == "judge" && s.kind == "mark"
}

/// A judgment's mark, as `ask --trace` shows it: `  loop.v1 shadow at
/// loop_end · reply · jdg_… (theseus judge show jdg_…)`, its id whole.
pub(super) fn mark_note(s: &Span) -> String {
    let a = |k: &str| s.attrs[k].as_str().unwrap_or("?");
    let class = s.attrs["class"]
        .as_str()
        .map(|c| format!(" · {c}"))
        .unwrap_or_default();
    let id = a("judgment");
    format!(
        "  {} {} at {}{class} · {id} (theseus judge show {id})",
        a("pack"),
        a("mode"),
        a("point")
    )
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

/// A bar of `p`, ten cells wide.
fn bar(p: f64) -> String {
    let n = (p.clamp(0.0, 1.0) * 10.0).round() as usize;
    format!("{}{}", "█".repeat(n), "·".repeat(10 - n))
}

/// An answer's probabilities, one line each: `    progressing  0.81 ████████··  ←`.
fn answer_lines(a: &Value, out: &mut Vec<String>) {
    let ans = &a["answer"];
    let band = &a["band"];
    out.push(format!(
        "  {} ({}): {} band, {} {:.2}",
        a["question"].as_str().unwrap_or("?"),
        ans["type"].as_str().unwrap_or("?"),
        band["band"].as_str().unwrap_or("?"),
        match ans["type"].as_str() {
            Some("noul") => "p",
            _ => "confidence",
        },
        band["value"].as_f64().unwrap_or(0.0)
    ));
    let row = |label: &str, p: f64, chosen: bool| {
        format!(
            "    {label:<22} {p:.2} {}{}",
            bar(p),
            if chosen { "  ←" } else { "" }
        )
    };
    match ans["type"].as_str() {
        Some("choice") => {
            let chosen = ans["choice"].as_str().unwrap_or_default();
            for o in ans["probabilities"].as_array().into_iter().flatten() {
                let name = o[0].as_str().unwrap_or("?");
                out.push(row(name, o[1].as_f64().unwrap_or(0.0), name == chosen));
            }
        }
        Some("score") => {
            let top = band["top"]["value"].as_u64();
            for (i, p) in ans["probabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                let label = format!("level {i}");
                out.push(row(
                    &label,
                    p.as_f64().unwrap_or(0.0),
                    top == Some(i as u64),
                ));
            }
        }
        Some("noul") => {
            let p = ans["noul"].as_f64().unwrap_or(0.0);
            out.push(row("yes", p, p >= 0.5));
            out.push(row("no", 1.0 - p, p < 0.5));
        }
        _ => {}
    }
}

/// A state's field, on one line: its value as JSON, cut at 160 characters.
fn field(k: &str, v: &Value) -> String {
    let t = match v {
        Value::String(s) => s.clone(),
        v => serde_json::to_string(v).unwrap_or_default(),
    };
    let n = t.chars().count();
    let mut t: String = t.chars().take(160).collect();
    if n > 160 {
        t.push_str(&format!("… ({n} chars; --json shows it whole)"));
    }
    format!("  {k}: {t}")
}

/// `theseus judge show <id>`: the judgment's header, its outcome, the
/// state as fields, and each answer with its probabilities and band.
pub fn judge_show_lines(r: &JudgeGetResult) -> Vec<String> {
    let e = &r.judgment;
    let d = &e.data;
    let s = |v: &Value| v.as_str().unwrap_or("-").to_string();
    let mut out = vec![
        format!(
            "{} · {} v{} ({}) at {} · {}",
            s(&d["id"]),
            s(&d["pack"]),
            d["version"].as_u64().unwrap_or(0),
            s(&d["mode"]),
            s(&d["point"]),
            fmt_time(e.at_unix_ms)
        ),
        format!(
            "session {} · turn {} · class {} · baseline {}",
            e.session_id.as_deref().unwrap_or("-"),
            e.turn_id.as_deref().unwrap_or("-"),
            s(&d["context"]["class"]),
            s(&d["context"]["decision"])
        ),
    ];
    let o = &d["outcome"];
    let outcome = match o["outcome"].as_str() {
        Some("answered") => "answered".to_string(),
        Some("skipped") => format!("skipped: {}", s(&o["reason"])),
        Some("failed") => format!("failed: {}", s(&o["class"])),
        _ => "?".into(),
    };
    let t = &d["timing"];
    out.push(format!(
        "{outcome} · model {}{} · {} ms (queued {}, http {}) · {} · {}",
        s(&d["model"]),
        match d["answered_by"].as_str() {
            Some(m) if d["model_drift"].as_bool() == Some(true) =>
                format!(", answered by {m} (drift)"),
            _ => String::new(),
        },
        t["total_ms"].as_u64().unwrap_or(0),
        t["queued_ms"].as_u64().unwrap_or(0),
        t["http_ms"].as_u64().unwrap_or(0),
        match d["cost_micros"].as_u64() {
            Some(m) => usd(m as f64 / 1_000_000.0),
            None => "no cost".into(),
        },
        if d["disagrees"].as_bool() == Some(true) {
            "disagrees with the baseline"
        } else {
            "agrees with the baseline"
        }
    ));
    out.push(format!(
        "state: {} bytes, ~{} tokens (cap {}), {} v{}, sha256 {}",
        d["state"]["bytes"].as_u64().unwrap_or(0),
        d["state"]["tokens"].as_u64().unwrap_or(0),
        d["state"]["cap_tokens"].as_u64().unwrap_or(0),
        s(&d["state"]["builder"]),
        d["state"]["builder_version"].as_u64().unwrap_or(0),
        s(&d["state"]["sha256"])
    ));
    match (&r.state, &r.state_missing) {
        (Some(Value::Object(m)), _) => out.extend(m.iter().map(|(k, v)| field(k, v))),
        (Some(v), _) => out.push(field("state", v)),
        (None, why) => out.push(format!(
            "  (the state is not shown: {})",
            why.as_deref().unwrap_or("its blob was not read")
        )),
    }
    let answers = d["answers"].as_array().cloned().unwrap_or_default();
    if !answers.is_empty() {
        out.push("answers:".into());
        for a in &answers {
            answer_lines(a, &mut out);
        }
    }
    out
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
            breakers: vec!["rerank: open (42s left)".into()],
            calls_today: 3,
            failed_today: 1,
            skipped_today: 2,
            spend_today_usd: 0.000267,
            shadow_limit_usd: 1.0,
            ..JudgeHealth::default()
        };
        assert_eq!(
            judge_line(&h),
            "judge: loop.v1: shadow · max live · breaker closed · rerank breaker open (42s left) · 3 calls today (1 failed, 2 skipped) · $0.000267 of $1.00 today"
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

    #[test]
    fn a_judgment_shows_its_state_as_fields_and_its_answers_as_bars() {
        let r = JudgeGetResult {
            judgment: LedgerEntry {
                position: 7,
                at_unix_ms: 3_723_000,
                kind: "judge.call".into(),
                session_id: Some("ses_a".into()),
                turn_id: Some("turn_a".into()),
                data: json!({"id": "jdg_a", "pack": "loop.v1", "version": 1, "mode": "shadow",
                "point": "loop_end", "model": "jev-1.13.0", "outcome": {"outcome": "answered"},
                "timing": {"total_ms": 412, "queued_ms": 0, "http_ms": 400}, "cost_micros": 89,
                "disagrees": false, "context": {"class": "reply", "decision": "no_tool_calls"},
                "state": {"bytes": 120, "tokens": 30, "cap_tokens": 4000, "builder": "loop",
                    "builder_version": 1, "sha256": "ab12"},
                "answers": [
                    {"question": "work_state", "answer": {"type": "choice", "choice": "complete",
                        "confidence": 0.9, "probabilities": [["complete", 0.9], ["progressing", 0.1]]},
                        "band": {"band": "act", "value": 0.9}},
                    {"question": "announced_unfinished", "answer": {"type": "noul", "noul": 0.2},
                        "band": {"band": "escalate", "value": 0.2}}
                ]}),
            },
            state: Some(json!({"ask": "Say done.", "loops": 1})),
            state_missing: None,
        };
        assert_eq!(
            judge_show_lines(&r),
            [
                "jdg_a · loop.v1 v1 (shadow) at loop_end · 01:02:03.000Z",
                "session ses_a · turn turn_a · class reply · baseline no_tool_calls",
                "answered · model jev-1.13.0 · 412 ms (queued 0, http 400) · $0.000089 · agrees with the baseline",
                "state: 120 bytes, ~30 tokens (cap 4000), loop v1, sha256 ab12",
                "  ask: Say done.",
                "  loops: 1",
                "answers:",
                "  work_state (choice): act band, confidence 0.90",
                "    complete               0.90 █████████·  ←",
                "    progressing            0.10 █·········",
                "  announced_unfinished (noul): escalate band, p 0.20",
                "    yes                    0.20 ██········",
                "    no                     0.80 ████████··  ←",
            ]
        );
    }

    #[test]
    fn a_trace_names_a_judgments_mark_whole() {
        let s = Span {
            name: "judge".into(),
            kind: "mark".into(),
            start_us: 9,
            end_us: Some(9),
            attrs: json!({"pack": "loop.v1", "point": "loop_end", "mode": "shadow",
                "judgment": "jdg_0123456789abcdef0123456789abcdef", "class": "reply", "loop": 0}),
            children: vec![],
        };
        assert!(is_mark(&s));
        assert_eq!(
            mark_note(&s),
            "  loop.v1 shadow at loop_end · reply · jdg_0123456789abcdef0123456789abcdef (theseus judge show jdg_0123456789abcdef0123456789abcdef)"
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
