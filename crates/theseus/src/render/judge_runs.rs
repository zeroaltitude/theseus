//! The owner's runs over the learning ledger (M5 25d): a replay, its two
//! versions side by side; an audit; a backfill. Apart from `render.rs`,
//! whose length the shape budget caps.

use theseus_protocol::judge_runs::{
    JudgeAuditResult, JudgeBackfillResult, JudgeReplayResult, OntoReplayReport, ReplayCase,
    ReplayCases, ReplayCheck,
};
use theseus_protocol::learning::{PackReport, QuestionReport};

fn share(v: Option<f64>) -> String {
    v.map_or_else(|| "–".into(), |x| format!("{:.0}%", x * 100.0))
}

fn num(v: Option<f64>) -> String {
    v.map_or_else(|| "–".into(), |x| format!("{x:.3}"))
}

/// Two columns: a label, then the incumbent's and the candidate's.
fn row(label: &str, a: &str, b: &str) -> String {
    format!("  {label:<34} {a:>14} {b:>14}")
}

fn bands(q: &QuestionReport) -> String {
    q.bands
        .iter()
        .map(|b| format!("{:.0}", b.share * 100.0))
        .collect::<Vec<_>>()
        .join("/")
}

/// One question's numbers on both sides.
fn question_rows(a: &QuestionReport, b: Option<&QuestionReport>, out: &mut Vec<String>) {
    let star = if a.decides { " *" } else { "" };
    out.push(format!("  {}{star} ({})", a.question, a.kind));
    let pick = |f: &dyn Fn(&QuestionReport) -> String| (f(a), b.map_or("–".into(), f));
    let (x, y) = pick(&|q| q.labeled.to_string());
    out.push(row("  labeled", &x, &y));
    let (x, y) = pick(&|q| bands(q));
    out.push(row("  bands act/confirm/escalate %", &x, &y));
    let (x, y) = pick(&|q| num(q.calibration.as_ref().map(|c| c.brier)));
    out.push(row("  Brier", &x, &y));
    let (x, y) = pick(&|q| num(q.calibration.as_ref().map(|c| c.ece)));
    out.push(row("  ECE", &x, &y));
    for c in a.classes.iter().filter(|c| c.predicted + c.actual > 0) {
        let other = b.and_then(|q| q.classes.iter().find(|k| k.class == c.class));
        out.push(row(
            &format!("  {} precision", c.class),
            &share(c.precision),
            &other.map_or("–".into(), |k| share(k.precision)),
        ));
        out.push(row(
            &format!("  {} recall", c.class),
            &share(c.recall),
            &other.map_or("–".into(), |k| share(k.recall)),
        ));
    }
}

fn side_rows(a: &PackReport, b: &PackReport, out: &mut Vec<String>) {
    out.push(row("", &a.pack, &b.pack));
    out.push(row(
        "answered",
        &a.answered.to_string(),
        &b.answered.to_string(),
    ));
    out.push(row(
        "labeled",
        &a.labeled.to_string(),
        &b.labeled.to_string(),
    ));
    out.push(row(
        "agreement with the baseline",
        &share(a.agreement),
        &share(b.agreement),
    ));
    for q in &a.questions {
        question_rows(
            q,
            b.questions.iter().find(|x| x.question == q.question),
            out,
        );
    }
}

/// `theseus judge replay --onto`: the Markdown the +onto report quotes, then
/// what the run cost and the sessions it left out, each with why.
fn onto_lines(r: &JudgeReplayResult, o: &OntoReplayReport) -> Vec<String> {
    let mut out: Vec<String> = o.markdown.lines().map(str::to_string).collect();
    out.push(String::new());
    out.push(format!(
        "{} · {} of an estimated {} (limit {})",
        r.id,
        format_args!("${:.4}", r.cost_usd),
        format_args!("${:.4}", r.estimate_usd),
        format_args!("${:.2}", r.limit_usd),
    ));
    out.extend(
        r.left_out
            .iter()
            .map(|l| format!("  left out {}: {}", l.judgment, l.reason)),
    );
    out
}

/// One question's miss, as `MISS` lists it: `model opus→sonnet 0.71`.
fn miss(question: &str, c: &ReplayCheck) -> String {
    let got = if c.got.is_empty() { "–" } else { &c.got };
    format!("{question} {}→{got} {:.2}", c.want, c.confidence)
}

/// A case's line: `ok`, `ok` with an effort one rung off, or each question
/// it missed. A case is right when its `model` and `context` agree and its
/// `effort` is within one rung (the bar's own count).
fn case_line(c: &ReplayCase) -> String {
    let tag = if c.must { " [must]" } else { "" };
    if !c.answered {
        return format!("{:<4}{tag} NO ANSWER (no judgment came back)", c.id);
    }
    let misses: Vec<String> = [
        ("model", &c.model, c.model.agrees),
        ("effort", &c.effort, c.effort.close),
        ("context", &c.context, c.context.agrees),
    ]
    .iter()
    .filter(|(_, _, right)| !right)
    .map(|(q, check, _)| miss(q, check))
    .collect();
    match (misses.is_empty(), c.effort.agrees) {
        (false, _) => format!("{:<4}{tag} MISS {}", c.id, misses.join(", ")),
        (true, true) => format!("{:<4}{tag} ok", c.id),
        (true, false) => format!(
            "{:<4}{tag} ok ({}, one rung off)",
            c.id,
            miss("effort", &c.effort)
        ),
    }
}

/// `theseus judge replay --cases`: a line for each case, then the totals, the
/// `must` count and the bar.
fn cases_lines(r: &JudgeReplayResult, c: &ReplayCases) -> Vec<String> {
    let n = c.cases.len();
    let mut out = vec![format!(
        "{} replayed {} (sha256 {}) over its cases file: {n} cases · ${:.4} of an estimated ${:.4} \
         (limit ${:.2})",
        r.id,
        r.candidate,
        &r.candidate_sha256[..r.candidate_sha256.len().min(12)],
        r.cost_usd,
        r.estimate_usd,
        r.limit_usd,
    )];
    out.extend(c.cases.iter().map(case_line));
    out.push(format!(
        "model {}/{n} · effort {}/{n} exact, {}/{n} within one rung · context {}/{n} · answered {}/{n}",
        c.model_ok, c.effort_exact, c.effort_close, c.context_ok, c.answered
    ));
    out.push(format!(
        "must {}/{}, {} under the confirm bar",
        c.must_ok, c.must_total, c.must_under_confirm
    ));
    out.push(match c.bar {
        Some(met) => format!(
            "bar (model 54, effort within one rung 50, context 56 of 60, every must case, none \
             under the confirm bar; the {} pin tests W39 and W40 counted as met): {}",
            c.held_elsewhere,
            if met { "met" } else { "NOT met" }
        ),
        None => "bar: not judged (the file is not the design's 58 replay cases)".to_string(),
    });
    out.extend(
        r.left_out
            .iter()
            .map(|l| format!("  left out {}: {}", l.judgment, l.reason)),
    );
    out
}

/// `theseus judge replay`: the run, both versions side by side, what the
/// candidate fixed and broke, and what was left out.
pub fn judge_replay_lines(r: &JudgeReplayResult) -> Vec<String> {
    if let Some(o) = &r.onto {
        return onto_lines(r, o);
    }
    if let Some(c) = &r.cases {
        return cases_lines(r, c);
    }
    let mut out = vec![format!(
        "{} replayed {} (sha256 {}) beside {} over the {}{}{}: {} judgments, labels {}.",
        r.id,
        r.candidate,
        &r.candidate_sha256[..r.candidate_sha256.len().min(12)],
        r.incumbent,
        r.set,
        r.report
            .as_deref()
            .map(|id| format!(" of {id}"))
            .unwrap_or_default(),
        if r.errors {
            ", the incumbent's errors only"
        } else {
            ""
        },
        r.judgments,
        r.labels
    )];
    out.push(format!(
        "  {}: {} called ({} stored states, {} rebuilt), {} re-banded, {} left out · {} of an \
         estimated {} (limit {})",
        if r.change == "thresholds_only" {
            "thresholds only, no call"
        } else {
            "asked again"
        },
        r.called,
        r.stored,
        r.rebuilt,
        r.rebanded,
        r.left_out.len(),
        format_args!("${:.4}", r.cost_usd),
        format_args!("${:.4}", r.estimate_usd),
        format_args!("${:.2}", r.limit_usd),
    ));
    side_rows(&r.incumbent_report, &r.candidate_report, &mut out);
    out.push(format!(
        "  agreement with the incumbent: {} · fixed {} · broken {}",
        share(r.agreement),
        r.fixed,
        r.broken
    ));
    for j in r
        .per_judgment
        .iter()
        .filter(|j| !j.fixed.is_empty() || !j.broken.is_empty())
    {
        let mut s = format!("    {}", j.judgment);
        if !j.fixed.is_empty() {
            s.push_str(&format!(" fixed {}", j.fixed.join(", ")));
        }
        if !j.broken.is_empty() {
            s.push_str(&format!(" broke {}", j.broken.join(", ")));
        }
        out.push(s);
    }
    for f in &r.fell {
        let what: Vec<&str> = [(f.precision_fell, "precision"), (f.recall_fell, "recall")]
            .iter()
            .filter(|(x, _)| *x)
            .map(|(_, w)| *w)
            .collect();
        out.push(format!(
            "  worse: {} {} ({})",
            f.question,
            f.class,
            what.join(" and ")
        ));
    }
    if let Some(e) = &r.eval {
        out.push(format!(
            "  planted injections ({}, {} cases): {} {} met {} missed · {} {} met {} missed",
            e.set,
            e.cases,
            e.incumbent.pack,
            e.incumbent.met,
            e.incumbent.missed,
            e.candidate.pack,
            e.candidate.met,
            e.candidate.missed
        ));
    }
    for l in &r.left_out {
        out.push(format!("  left out {}: {}", l.judgment, l.reason));
    }
    out
}

/// `theseus judge audit`: the run's counts and cost.
pub fn judge_audit_lines(r: &JudgeAuditResult) -> Vec<String> {
    let mut out = vec![format!(
        "{} audited {} with {} ({}, seed {}): {} of {} eligible judgments sampled, {} asked, {} \
         failed; {} audit labels, {} answers dropped · ${:.4} (limit ${:.2})",
        r.id,
        r.pack,
        r.profile,
        r.model,
        r.seed,
        r.sampled,
        r.eligible,
        r.asked,
        r.failed,
        r.labels,
        r.dropped,
        r.cost_usd,
        r.limit_usd
    )];
    if let Some(s) = &r.stopped {
        out.push(format!("  stopped early: {s}"));
    }
    out
}

/// `theseus judge backfill`: the run's counts, cost, and consent.
pub fn judge_backfill_lines(r: &JudgeBackfillResult) -> Vec<String> {
    let mut out = vec![format!(
        "{} backfilled {}: {} events, {} judged now, {} judged before, {} failed, {} left out · \
         ${:.4} of an estimated ${:.4} (limit ${:.2}) · consent of config {}",
        r.id,
        r.pack,
        r.events,
        r.judged,
        r.already,
        r.failed,
        r.left_out.len(),
        r.cost_usd,
        r.estimate_usd,
        r.limit_usd,
        &r.consent[..r.consent.len().min(12)]
    )];
    for l in &r.left_out {
        out.push(format!("  left out {}: {}", l.judgment, l.reason));
    }
    out
}

#[cfg(test)]
mod tests {
    use theseus_protocol::judge_runs::{
        ReplayCase, ReplayCases, ReplayCheck, ReplayLeftOut, UndoRate,
    };

    use super::*;

    fn check(want: &str, got: &str, confidence: f64, close: bool) -> ReplayCheck {
        ReplayCheck {
            want: want.into(),
            got: got.into(),
            confidence,
            agrees: want == got,
            close,
        }
    }

    fn case(
        id: &str,
        must: bool,
        model: ReplayCheck,
        effort: ReplayCheck,
        context: ReplayCheck,
    ) -> ReplayCase {
        ReplayCase {
            id: id.into(),
            must,
            answered: true,
            model,
            effort,
            context,
        }
    }

    /// `judge replay --cases` prints a line for each case (`ok`, or what it
    /// missed with Jev's option and confidence), then the totals, the
    /// `must` count and the bar.
    #[test]
    fn a_cases_replay_prints_a_line_a_case_then_the_totals_and_the_bar() {
        let right = || check("full", "full", 0.9, true);
        let cases = vec![
            case(
                "L1",
                false,
                check("haiku", "haiku", 0.95, true),
                check("low", "low", 0.9, true),
                right(),
            ),
            case(
                "W26",
                true,
                check("sonnet", "fable", 0.71, false),
                check("high", "high", 0.9, true),
                right(),
            ),
            case(
                "L11",
                false,
                check("opus", "opus", 0.9, true),
                check("high", "medium", 0.8, true),
                right(),
            ),
            case(
                "W5",
                false,
                check("haiku", "haiku", 0.9, true),
                check("low", "max", 0.9, false),
                check("full", "short", 0.88, false),
            ),
            ReplayCase {
                id: "W9".into(),
                must: true,
                ..Default::default()
            },
        ];
        let r = JudgeReplayResult {
            id: "rpl_1".into(),
            candidate: "route.v10".into(),
            candidate_sha256: "abcdef0123456789".into(),
            cost_usd: 0.0123,
            estimate_usd: 0.02,
            limit_usd: 5.0,
            left_out: vec![ReplayLeftOut {
                judgment: "W9".into(),
                reason: "the run reached [judge] replay_limit_usd".into(),
            }],
            cases: Some(ReplayCases {
                pack: "route.v10".into(),
                cases,
                answered: 4,
                model_ok: 3,
                effort_exact: 2,
                effort_close: 3,
                context_ok: 3,
                must_total: 2,
                must_ok: 0,
                must_under_confirm: 0,
                held_elsewhere: 2,
                bar: Some(false),
            }),
            ..Default::default()
        };
        let lines = judge_replay_lines(&r);
        assert_eq!(
            lines,
            [
                "rpl_1 replayed route.v10 (sha256 abcdef012345) over its cases file: 5 cases · $0.0123 of an estimated $0.0200 (limit $5.00)",
                "L1   ok",
                "W26  [must] MISS model sonnet→fable 0.71",
                "L11  ok (effort high→medium 0.80, one rung off)",
                "W5   MISS effort low→max 0.90, context full→short 0.88",
                "W9   [must] NO ANSWER (no judgment came back)",
                "model 3/5 · effort 2/5 exact, 3/5 within one rung · context 3/5 · answered 4/5",
                "must 0/2, 0 under the confirm bar",
                "bar (model 54, effort within one rung 50, context 56 of 60, every must case, none under the confirm bar; the 2 pin tests W39 and W40 counted as met): NOT met",
                "  left out W9: the run reached [judge] replay_limit_usd",
            ]
        );
        // A met bar says so; a file that is not the 58 says it judges none.
        let mut met = r.clone();
        met.cases.as_mut().unwrap().bar = Some(true);
        assert!(judge_replay_lines(&met)
            .iter()
            .any(|l| l.ends_with("): met")));
        let mut other = r;
        other.cases.as_mut().unwrap().bar = None;
        assert!(judge_replay_lines(&other)
            .iter()
            .any(|l| l.starts_with("bar: not judged")));
    }

    /// `judge replay --onto` prints the Markdown the +onto report quotes,
    /// then the run's cost and each session left out with its reason.
    #[test]
    fn an_onto_replay_prints_its_markdown_then_its_cost() {
        let r = JudgeReplayResult {
            id: "rpl_1".into(),
            cost_usd: 0.0123,
            estimate_usd: 0.02,
            limit_usd: 1.0,
            left_out: vec![ReplayLeftOut {
                judgment: "ses_a".into(),
                reason: "no first message from a person can be read".into(),
            }],
            onto: Some(OntoReplayReport {
                pack: "onto.v1".into(),
                undo: vec![UndoRate {
                    kind: "topic".into(),
                    writes: 4,
                    undone: 1,
                    rate: Some(0.25),
                }],
                markdown: "# onto.v1 against the import's labels\n\n3 sessions replayed.\n".into(),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            judge_replay_lines(&r),
            [
                "# onto.v1 against the import's labels",
                "",
                "3 sessions replayed.",
                "",
                "rpl_1 · $0.0123 of an estimated $0.0200 (limit $1.00)",
                "  left out ses_a: no first message from a person can be read",
            ]
        );
    }
}
