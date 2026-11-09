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

/// One question's miss, as `MISS` lists it: `model opus→sonnet 0.71`. A pick
/// that fell under the confirm bar says so, with what Jev said: the line
/// would otherwise show an answer Jev never gave (theseus-mir9c).
fn miss(question: &str, c: &ReplayCheck) -> String {
    let got = if c.got.is_empty() { "–" } else { &c.got };
    let line = format!("{question} {}→{got} {:.2}", c.want, c.confidence);
    match c.sub_confirm {
        true => format!(
            "{line} (under confirm {:.2}: Jev said {}, graded as no_match {got})",
            c.confirm, c.raw
        ),
        false => line,
    }
}

/// A question that fell under the confirm bar and was right all the same
/// (the `no_match` is the label): a line that showed `ok` before and shows
/// `ok` now still says the answer graded is the `no_match`, not Jev's.
fn fell(question: &str, c: &ReplayCheck) -> String {
    format!(
        "{question} fell under confirm {:.2}: Jev said {} at {:.2}, graded as no_match {}",
        c.confirm, c.raw, c.confidence, c.got
    )
}

/// The questions a cases run graded: its own list, or the two a run before
/// theseus-h66v4.6 graded when it names none.
fn asked_of(c: &ReplayCases) -> Vec<&str> {
    match c.asked.is_empty() {
        true => vec!["model", "effort"],
        false => c.asked.iter().map(String::as_str).collect(),
    }
}

/// The check of one deciding question of a case.
fn check_of<'a>(c: &'a ReplayCase, q: &str) -> &'a ReplayCheck {
    match q {
        "model" => &c.model,
        "effort" => &c.effort,
        "work" => &c.work,
        _ => &c.split,
    }
}

/// A case's line: `ok`, `ok` with an effort one rung off, or each question
/// it missed. A case is right on a question when its answer agrees (`effort`:
/// is within one rung, the bar's own count). A question that fell under its
/// confirm bar is named wherever it landed.
fn case_line(c: &ReplayCase, asked: &[&str]) -> String {
    let tag = if c.must { " [must]" } else { "" };
    if !c.answered {
        return format!("{:<4}{tag} NO ANSWER (no judgment came back)", c.id);
    }
    let qs: Vec<(&str, &ReplayCheck, bool)> = asked
        .iter()
        .map(|q| {
            let check = check_of(c, q);
            let right = if *q == "effort" {
                check.close
            } else {
                check.agrees
            };
            (*q, check, right)
        })
        .collect();
    let misses: Vec<String> = qs
        .iter()
        .filter(|(_, _, right)| !right)
        .map(|(q, check, _)| miss(q, check))
        .collect();
    let mut notes: Vec<String> = qs
        .iter()
        .filter(|(_, check, right)| *right && check.sub_confirm)
        .map(|(q, check, _)| fell(q, check))
        .collect();
    let effort_exact = !asked.contains(&"effort") || c.effort.agrees;
    match (misses.is_empty(), effort_exact) {
        (false, _) if notes.is_empty() => format!("{:<4}{tag} MISS {}", c.id, misses.join(", ")),
        (false, _) => format!(
            "{:<4}{tag} MISS {} · also {}",
            c.id,
            misses.join(", "),
            notes.join("; ")
        ),
        (true, true) if notes.is_empty() => format!("{:<4}{tag} ok", c.id),
        (true, exact) => {
            if !exact && !c.effort.sub_confirm {
                notes.push(format!("{}, one rung off", miss("effort", &c.effort)));
            }
            format!("{:<4}{tag} ok ({})", c.id, notes.join("; "))
        }
    }
}

/// `theseus judge replay --cases`: a line for each case, then the totals, the
/// `must` count and each bar, a ratio of its own set with its rounding rule.
fn cases_lines(r: &JudgeReplayResult, c: &ReplayCases) -> Vec<String> {
    let n = c.cases.len();
    let asked = asked_of(c);
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
    out.extend(c.cases.iter().map(|x| case_line(x, &asked)));
    let part = |q: &str, raw: bool| match (q, raw) {
        ("model", false) => format!("model {}/{n}", c.model_ok),
        ("model", true) => format!("model {}/{n}", c.raw_model_ok),
        ("effort", false) => format!(
            "effort {}/{n} exact, {}/{n} within one rung",
            c.effort_exact, c.effort_close
        ),
        ("effort", true) => format!(
            "effort {}/{n} exact, {}/{n} within one rung",
            c.raw_effort_exact, c.raw_effort_close
        ),
        ("work", false) => format!("work {}/{n}", c.work_ok),
        ("work", true) => format!("work {}/{n}", c.raw_work_ok),
        (_, false) => format!("split {}/{n}", c.split_ok),
        (_, true) => format!("split {}/{n}", c.raw_split_ok),
    };
    let join = |raw: bool| {
        asked
            .iter()
            .map(|q| part(q, raw))
            .collect::<Vec<_>>()
            .join(" · ")
    };
    out.push(format!("{} · answered {}/{n}", join(false), c.answered));
    let fell_in = |q: &str| {
        c.cases
            .iter()
            .filter(|x| check_of(x, q).sub_confirm)
            .count()
    };
    out.push(format!(
        "under the confirm bar, graded as the question's no_match: {}",
        asked
            .iter()
            .map(|q| format!("{q} {}", fell_in(q)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push(format!("raw top picks, no bar applied: {}", join(true)));
    if asked.contains(&"model") {
        out.push(format!(
            "must {}/{}, {} under the confirm bar",
            c.must_ok, c.must_total, c.must_under_confirm
        ));
    }
    for b in &c.bars {
        let what = match b.question.as_str() {
            "effort" => "effort within one rung",
            "must" => "must (W40 is the daemon's test, outside this set)",
            q => q,
        };
        out.push(format!(
            "bar {what}: {}/{} against {} (80% of {}, rounded up): {}",
            b.ok,
            b.of,
            b.need,
            b.of,
            if b.met { "met" } else { "NOT met" }
        ));
    }
    out.push(match (c.bar, c.held_elsewhere) {
        // A run whose cases file holds no pin has none to name (work and split).
        (Some(met), 0) => format!(
            "bars of this run: {}",
            if met { "all met" } else { "NOT all met" }
        ),
        (Some(met), n) => format!(
            "bars of this run ({n} W39 and W40 are the daemon's tests, outside every denominator): {}",
            if met { "all met" } else { "NOT all met" }
        ),
        (None, _) => "bar: not judged (this run graded no question; read the counts)".to_string(),
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
        ReplayBar, ReplayCase, ReplayCases, ReplayCheck, ReplayLeftOut, UndoRate,
    };

    use super::*;

    fn check(want: &str, got: &str, confidence: f64, close: bool) -> ReplayCheck {
        ReplayCheck {
            want: want.into(),
            got: got.into(),
            confidence,
            agrees: want == got,
            close,
            raw: got.into(),
            confirm: 0.6,
            sub_confirm: false,
            raw_agrees: want == got,
            raw_close: close,
        }
    }

    /// A question whose band fell under `confirm`: Jev said `raw` at
    /// `confidence`, and the graded answer is the question's `no_match`.
    fn fell(want: &str, raw: &str, no_match: &str, confidence: f64) -> ReplayCheck {
        ReplayCheck {
            want: want.into(),
            got: no_match.into(),
            confidence,
            agrees: want == no_match,
            close: want == no_match,
            raw: raw.into(),
            confirm: 0.6,
            sub_confirm: true,
            raw_agrees: want == raw,
            raw_close: want == raw,
        }
    }

    fn case(id: &str, must: bool, model: ReplayCheck, effort: ReplayCheck) -> ReplayCase {
        ReplayCase {
            id: id.into(),
            must,
            answered: true,
            model,
            effort,
            ..Default::default()
        }
    }

    /// `judge replay --cases` prints a line for each case (`ok`, or what it
    /// missed with Jev's option and confidence), then the totals, the
    /// `must` count and the bar.
    #[test]
    fn a_cases_replay_prints_a_line_a_case_then_the_totals_and_the_bar() {
        let cases = vec![
            case(
                "L1",
                false,
                check("haiku", "haiku", 0.95, true),
                check("low", "low", 0.9, true),
            ),
            case(
                "W26",
                true,
                check("sonnet", "fable", 0.71, false),
                check("high", "high", 0.9, true),
            ),
            case(
                "L11",
                false,
                check("opus", "opus", 0.9, true),
                check("high", "medium", 0.8, true),
            ),
            case(
                "W5",
                false,
                check("haiku", "haiku", 0.9, true),
                check("low", "max", 0.9, false),
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
                must_total: 2,
                must_ok: 0,
                must_under_confirm: 0,
                raw_model_ok: 3,
                raw_effort_exact: 2,
                raw_effort_close: 3,
                held_elsewhere: 2,
                bars: vec![
                    ReplayBar {
                        question: "model".into(),
                        ok: 3,
                        of: 5,
                        need: 4,
                        met: false,
                    },
                    ReplayBar {
                        question: "must".into(),
                        ok: 0,
                        of: 2,
                        need: 2,
                        met: false,
                    },
                ],
                bar: Some(false),
                ..Default::default()
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
                "W5   MISS effort low→max 0.90",
                "W9   [must] NO ANSWER (no judgment came back)",
                "model 3/5 · effort 2/5 exact, 3/5 within one rung · answered 4/5",
                "under the confirm bar, graded as the question's no_match: model 0, effort 0",
                "raw top picks, no bar applied: model 3/5 · effort 2/5 exact, 3/5 within one rung",
                "must 0/2, 0 under the confirm bar",
                "bar model: 3/5 against 4 (80% of 5, rounded up): NOT met",
                "bar must (W40 is the daemon's test, outside this set): 0/2 against 2 (80% of 2, rounded up): NOT met",
                "bars of this run (2 W39 and W40 are the daemon's tests, outside every denominator): NOT all met",
                "  left out W9: the run reached [judge] replay_limit_usd",
            ]
        );
    }

    /// Met bars say so; a run that graded nothing says it judges none.
    #[test]
    fn met_bars_say_so_and_a_run_that_grades_nothing_says_it_judges_none() {
        let mut r = JudgeReplayResult {
            id: "rpl_1".into(),
            candidate: "route.v10".into(),
            candidate_sha256: "abcdef0123456789".into(),
            cases: Some(ReplayCases {
                pack: "route.v10".into(),
                held_elsewhere: 2,
                bar: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert!(judge_replay_lines(&r)
            .iter()
            .any(|l| l.ends_with("): all met")));
        r.cases.as_mut().unwrap().bar = None;
        assert!(judge_replay_lines(&r)
            .iter()
            .any(|l| l.starts_with("bar: not judged (this run graded no question")));
    }

    /// A run over a cases file with no pin in it (work, split) does not name the two the model
    /// file's tests hold elsewhere (theseus-h66v4.6 fix pass).
    #[test]
    fn a_run_with_no_pin_cases_does_not_name_the_pins() {
        let r = JudgeReplayResult {
            id: "rpl_1".into(),
            candidate: "split.v1".into(),
            candidate_sha256: "abcdef0123456789".into(),
            cases: Some(ReplayCases {
                pack: "split.v1".into(),
                held_elsewhere: 0,
                bar: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let lines = judge_replay_lines(&r);
        assert!(lines.iter().any(|l| l == "bars of this run: NOT all met"));
        assert!(!lines.iter().any(|l| l.contains("W39")), "{lines:?}");
    }

    /// A pick under the confirm bar says so: the line names the question, what
    /// Jev said and at what confidence, the bar and the `no_match` that was
    /// graded, whether the case is a miss or an `ok` that changed. The totals
    /// count the questions that fell, and the raw picks beside them
    /// (theseus-mir9c).
    #[test]
    fn a_pick_under_the_confirm_bar_is_named_not_reported_as_a_different_answer() {
        let cases = vec![
            // Right pick under the bar: the daemon runs the question's `no_match`
            // (`sonnet` is this sample's; route.v10's own is `parent`), a miss.
            case(
                "W18",
                true,
                fell("opus", "opus", "sonnet", 0.55),
                check("medium", "medium", 0.9, true),
            ),
            // An effort under the bar is `high`, which the case wants: ok.
            case(
                "L3",
                false,
                check("sonnet", "sonnet", 0.9, true),
                fell("high", "medium", "high", 0.55),
            ),
            // A miss, a fell-and-right, and an effort that fell: all named.
            case(
                "L11",
                false,
                fell("opus", "opus", "sonnet", 0.50),
                fell("high", "medium", "high", 0.40),
            ),
        ];
        let r = JudgeReplayResult {
            id: "rpl_2".into(),
            candidate: "route.v10".into(),
            candidate_sha256: "abcdef0123456789".into(),
            cases: Some(ReplayCases {
                pack: "route.v10".into(),
                cases,
                answered: 3,
                model_ok: 1,
                effort_exact: 3,
                effort_close: 3,
                must_total: 1,
                must_ok: 0,
                must_under_confirm: 1,
                raw_model_ok: 3,
                raw_effort_exact: 2,
                raw_effort_close: 2,
                held_elsewhere: 2,
                bar: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        let lines = judge_replay_lines(&r);
        assert_eq!(
            &lines[1..7],
            [
                "W18  [must] MISS model opus→sonnet 0.55 (under confirm 0.60: Jev said opus, graded as no_match sonnet)",
                "L3   ok (effort fell under confirm 0.60: Jev said medium at 0.55, graded as no_match high)",
                "L11  MISS model opus→sonnet 0.50 (under confirm 0.60: Jev said opus, graded as no_match sonnet) · also effort fell under confirm 0.60: Jev said medium at 0.40, graded as no_match high",
                "model 1/3 · effort 3/3 exact, 3/3 within one rung · answered 3/3",
                "under the confirm bar, graded as the question's no_match: model 2, effort 2",
                "raw top picks, no bar applied: model 3/3 · effort 2/3 exact, 2/3 within one rung",
            ]
        );
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
