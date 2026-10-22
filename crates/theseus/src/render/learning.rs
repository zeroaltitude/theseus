//! The learning ledger's lines (M5 25c): a label as written, and the
//! learning report, per pack version and question, with its holdout. Apart
//! from `render.rs`, whose length the shape budget caps.

use theseus_protocol::judge_runs::UndoRate;
use theseus_protocol::learning::{
    Calibration, Holdout, JudgeLabelResult, LearningReport, PackReport, QuestionReport,
};

/// `lbl_… labeled jdg_… (loop.v1) work_state: "progressing"`.
pub fn judge_label_line(r: &JudgeLabelResult) -> String {
    let what = r.question.as_deref().map_or_else(
        || "the whole judgment".to_string(),
        |q| match &r.about {
            Some(item) => format!("{q} (about {item})"),
            None => q.to_string(),
        },
    );
    format!(
        "{} labeled {} ({}) {what}: {} ({}, weight {})",
        r.id, r.judgment, r.pack, r.label, r.source, r.weight
    )
}

fn share(v: Option<f64>) -> String {
    v.map_or_else(|| "–".into(), |x| format!("{:.0}%", x * 100.0))
}

fn ms(v: Option<u64>) -> String {
    v.map_or_else(|| "–".into(), |x| format!("{x} ms"))
}

fn calibration(c: &Calibration) -> String {
    format!("Brier {:.3} · ECE {:.3} over {}", c.brier, c.ece, c.n)
}

fn question_lines(q: &QuestionReport, indent: &str, out: &mut Vec<String>) {
    let bands: Vec<String> = q
        .bands
        .iter()
        .filter(|b| b.n > 0)
        .map(|b| format!("{} {:.0}%", b.band, b.share * 100.0))
        .collect();
    let mut s = format!(
        "{indent}{}{} ({}): {} answered, {} labeled",
        q.question,
        if q.decides { " *" } else { "" },
        q.kind,
        q.answered,
        q.labeled
    );
    if !bands.is_empty() {
        s.push_str(&format!(" · bands {}", bands.join(", ")));
    }
    if let Some(c) = &q.calibration {
        s.push_str(&format!(" · {}", calibration(c)));
    }
    out.push(s);
    for c in q.classes.iter().filter(|c| c.predicted + c.actual > 0) {
        out.push(format!(
            "{indent}  {}: precision {} ({} predicted) · recall {} ({} labeled)",
            c.class,
            share(c.precision),
            c.predicted,
            share(c.recall),
            c.actual
        ));
    }
    if let Some(c) = q.calibration.as_ref().filter(|c| c.n > 0) {
        let bins: Vec<String> = c
            .bins
            .iter()
            .filter(|b| b.n > 0)
            .map(|b| format!("{:.1}-{:.1}: {:.2} of {}", b.lo, b.hi, b.frequency, b.n))
            .collect();
        out.push(format!("{indent}  reliability: {}", bins.join(" · ")));
    }
}

fn holdout_lines(h: &Holdout, date: &str, out: &mut Vec<String>) {
    out.push(format!(
        "  holdout, the {} days before {date} (frozen): {} judgments, {} labels; {} before it, \
         {} after · {}",
        (h.end_ms - h.start_ms) / 86_400_000,
        h.judgments.len(),
        h.labels.len(),
        h.train,
        h.later,
        h.insufficient.as_deref().unwrap_or("sufficient")
    ));
}

/// The ontology pack's undo rate per kind, with its window (theseus-2tqqs), in
/// the cockpit's words (`ontoKindWords`): `undo rate by kind (Jev's writes, 7
/// days to 2026-10-21): person 1 of 1 (100%) · topic 1 of 2 (50%)`. None for a
/// pack that carries none.
pub fn undo_words(undo: &[UndoRate], date: &str) -> Option<String> {
    if undo.is_empty() {
        return None;
    }
    let kinds: Vec<String> = undo
        .iter()
        .map(|u| {
            let pct = (u.rate.unwrap_or(0.0) * 100.0).round();
            format!("{} {} of {} ({pct}%)", u.kind, u.undone, u.writes)
        })
        .collect();
    Some(format!(
        "undo rate by kind (Jev's writes, 7 days to {date}): {}",
        kinds.join(" · ")
    ))
}

fn pack_lines(p: &PackReport, date: &str, out: &mut Vec<String>) {
    out.push(format!(
        "{}: {} calls ({} answered, {} failed, {} skipped) · {} labeled · agrees with the \
         baseline ({}) {} · ${:.6}",
        p.pack,
        p.calls,
        p.answered,
        p.failed,
        p.skipped,
        p.labeled,
        p.baseline,
        share(p.agreement),
        p.cost_usd
    ));
    if let Some(words) = undo_words(&p.undo, date) {
        out.push(format!("  {words}"));
    }
    for l in &p.latency {
        out.push(format!(
            "  latency {} ({}): p50 {} · p95 {} · p99 {}",
            l.class,
            l.n,
            ms(l.p50_ms),
            ms(l.p95_ms),
            ms(l.p99_ms)
        ));
    }
    for q in &p.questions {
        question_lines(q, "  ", out);
    }
    holdout_lines(&p.holdout, date, out);
}

/// The report: its day and labels, then each pack version's lines.
pub fn learning_report_lines(r: &LearningReport) -> Vec<String> {
    let mut out = vec![format!(
        "learning report {} ({}, {}): labels {} operator, {} system ({} new), {} audit",
        r.date,
        r.trigger,
        super::fmt_time(r.at_unix_ms),
        r.labels.operator,
        r.labels.system,
        r.labels.system_written,
        r.labels.audit
    )];
    if r.packs.is_empty() {
        out.push("no judgments yet: nothing to report".into());
    }
    for p in &r.packs {
        pack_lines(p, &r.date, &mut out);
    }
    out.push(
        "(* decides; holdouts need 200 labeled per deciding question, 30 per acting class)".into(),
    );
    out
}

#[cfg(test)]
mod tests {
    use theseus_protocol::judge_runs::UndoRate;
    use theseus_protocol::learning::{LearningReport, PackReport};

    use super::learning_report_lines;

    fn rate(kind: &str, writes: u32, undone: u32) -> UndoRate {
        UndoRate {
            kind: kind.into(),
            writes,
            undone,
            rate: Some(f64::from(undone) / f64::from(writes)),
        }
    }

    /// The stored report's `onto.v1` line is printed under its pack, with its
    /// window, in the cockpit's words; a pack with none prints no such line.
    #[test]
    fn the_onto_packs_undo_line_is_printed_with_its_window() {
        let r = LearningReport {
            date: "2026-10-21".into(),
            packs: vec![
                PackReport {
                    pack: "onto.v1".into(),
                    undo: vec![rate("person", 1, 1), rate("topic", 2, 1)],
                    ..PackReport::default()
                },
                PackReport {
                    pack: "loop.v1".into(),
                    ..PackReport::default()
                },
            ],
            ..LearningReport::default()
        };
        let lines = learning_report_lines(&r);
        let undo: Vec<&String> = lines.iter().filter(|l| l.contains("undo rate")).collect();
        assert_eq!(
            undo,
            [
                "  undo rate by kind (Jev's writes, 7 days to 2026-10-21): person 1 of 1 (100%) · \
              topic 1 of 2 (50%)"
            ]
        );
        let at = |p: &str| lines.iter().position(|l| l.starts_with(p)).unwrap();
        let line = lines.iter().position(|l| l.contains("undo rate")).unwrap();
        assert_eq!(line, at("onto.v1") + 1, "the line is the onto pack's");
        assert!(at("loop.v1") > line);
    }
}
