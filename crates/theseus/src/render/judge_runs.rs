//! The owner's runs over the learning ledger (M5 25d): a replay, its two
//! versions side by side. Apart from `render.rs`,
//! whose length the shape budget caps.

use theseus_protocol::judge_runs::JudgeReplayResult;
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

/// `theseus judge replay`: the run, both versions side by side, what the
/// candidate fixed and broke, and what was left out.
pub fn judge_replay_lines(r: &JudgeReplayResult) -> Vec<String> {
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
