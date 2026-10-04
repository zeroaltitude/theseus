//! The report over the arms (row 55; §2.9's report): the plan's digest and
//! the data's window; each arm's pass rate with its interval and n, over
//! all items and each half; the paired differences, clustered by item
//! (`baseline − none`, `bm25 − none`, `oracle − none`, `baseline − bm25`,
//! `oracle − baseline`), each saying `gain`, `loss` or `insufficient`; cost
//! per pass and recall's latency; the decision per feature, with the clause
//! of the plan's rule that decided it; what could not be measured, and why;
//! then each item. Markdown, written frozen (`freeze`).
//!
//! When a cell ran more than once (an errored cell runs again on resume), its
//! last record with a verdict counts; without one, its last record.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::drive::{CellRecall, Record};
use crate::item::Exam;
use crate::stats::{paired, Cell, Interval};

/// The record that counts for each (item, arm, run).
pub fn latest(records: &[Record]) -> Vec<Record> {
    let mut m: BTreeMap<(String, String, u32), Record> = BTreeMap::new();
    for r in records {
        let k = (r.item.clone(), r.arm.clone(), r.run);
        match m.get(&k) {
            Some(prev) if prev.pass.is_some() && r.pass.is_none() => {}
            _ => {
                m.insert(k, r.clone());
            }
        }
    }
    m.into_values().collect()
}

/// Score stored replies again with `exam`'s checks (a corrected exam is a
/// new version; the report says which scored them). Calls are not stored in
/// full form, so a check on calls reads them as stored.
pub fn rescore(records: &mut [Record], exam: &Exam) {
    for r in records.iter_mut().filter(|r| r.error.is_none()) {
        let Some(check) = exam.checks.get(&r.item) else {
            continue;
        };
        let calls = r
            .calls
            .iter()
            .map(|c| crate::check::Call {
                tool: c["tool"].as_str().unwrap_or("").into(),
                input: c["input"].clone(),
            })
            .collect();
        let a = crate::check::Answer {
            reply: r.reply.clone(),
            calls,
            root: None,
        };
        r.lines = check.run(&a);
        r.pass = Some(r.lines.iter().all(|l| l.pass));
        r.digest = exam.digest.clone();
        r.exam = exam.file.version.clone();
    }
}

fn pct(x: f64) -> String {
    if x.is_nan() {
        "–".into()
    } else {
        format!("{:.0}%", x * 100.0)
    }
}

fn signed_pts(x: f64) -> String {
    if x.is_nan() {
        "–".into()
    } else {
        format!("{:+.0}", x * 100.0)
    }
}

/// `62% [48, 76]`.
fn rate(i: &Interval) -> String {
    match (i.lo, i.hi) {
        (Some(lo), Some(hi)) => format!("{} [{:.0}, {:.0}]", pct(i.mean), lo * 100.0, hi * 100.0),
        _ => pct(i.mean),
    }
}

/// `+38 pts [+24, +52]`.
fn diff(i: &Interval) -> String {
    match (i.lo, i.hi) {
        (Some(lo), Some(hi)) => format!(
            "{} [{}, {}]",
            signed_pts(i.mean),
            signed_pts(lo),
            signed_pts(hi)
        ),
        _ => signed_pts(i.mean),
    }
}

fn p(x: f64) -> String {
    if x < 0.001 {
        "<0.001".into()
    } else {
        format!("{x:.3}")
    }
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((v.len() as f64 - 1.0) * q).round() as usize;
    v[i]
}

/// The plan every report is read under (§2.9: "Every report cites its
/// digest"), as this build holds it.
pub const PLAN: &str = include_str!("../../../docs/m6-ablation-plan.md");

/// `sha256:` and the hex of the plan's bytes.
pub fn plan_digest() -> String {
    use sha2::{Digest, Sha256};
    format!("sha256:{}", hex(&Sha256::digest(PLAN.as_bytes())))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The arms in the report's order.
pub const ARMS: [&str; 4] = ["none", "bm25", "baseline", "oracle"];

/// The paired differences the report gives, `b − a`, as `(a, b)`.
pub const PAIRS: [(&str, &str); 5] = [
    ("none", "baseline"),
    ("none", "bm25"),
    ("none", "oracle"),
    ("bm25", "baseline"),
    ("baseline", "oracle"),
];

/// What a difference's interval can say: `gain` when it lies above zero,
/// `loss` below, and `insufficient` when it holds zero or there is none.
pub fn verdict(i: &Interval) -> &'static str {
    match (i.lo, i.hi) {
        (Some(lo), _) if lo > 0.0 => "gain",
        (_, Some(hi)) if hi < 0.0 => "loss",
        _ => "insufficient",
    }
}

/// Write `md` to `path`, frozen: a report is never overwritten.
pub fn freeze(path: &std::path::Path, md: &str) -> anyhow::Result<()> {
    use std::io::Write as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            anyhow::anyhow!(
                "{}: {e}: a report is frozen; write a new one under another name",
                path.display()
            )
        })?;
    f.write_all(md.as_bytes())?;
    Ok(())
}

/// One arm's pass rate: the mean of its items' rates, with the t interval
/// over items, and its cells.
pub struct ArmRate {
    pub rate: Interval,
    pub cells: usize,
    pub errors: usize,
}

pub fn arm_rate(cells: &[Cell], arm: &str, keep: impl Fn(&Cell) -> bool) -> ArmRate {
    let t = crate::stats::tallies(cells, |c| c.arm == arm && keep(c));
    let rates: Vec<f64> = t.values().filter_map(crate::stats::Tally::rate).collect();
    ArmRate {
        rate: crate::stats::mean_interval(&rates, (0.0, 1.0)),
        cells: t.values().map(|t| t.runs).sum(),
        errors: t.values().map(|t| t.errors).sum(),
    }
}

fn cells_of(recs: &[Record]) -> Vec<Cell> {
    recs.iter()
        .map(|r| Cell {
            item: r.item.clone(),
            group: r.family.clone(),
            held_out: r.held_out,
            arm: r.arm.clone(),
            pass: r.pass,
        })
        .collect()
}

/// The halves a table is read over.
const HALVES: [(&str, Half); 3] = [
    ("all", Half::All),
    ("held in", Half::In),
    ("held out", Half::Out),
];

#[derive(Clone, Copy)]
enum Half {
    All,
    In,
    Out,
}

impl Half {
    fn keeps(self, c: &Cell) -> bool {
        match self {
            Half::All => true,
            Half::In => !c.held_out,
            Half::Out => c.held_out,
        }
    }
}

/// The report: see the module's note and `docs/m6-ablation-plan.md`.
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn render(records: &[Record], exam: &Exam, source: &str) -> String {
    let recs = latest(records);
    let cells = cells_of(&recs);
    let arms: Vec<&str> = ARMS
        .into_iter()
        .filter(|a| recs.iter().any(|r| r.arm == *a))
        .collect();
    let mut o = String::new();
    let digests: BTreeSet<&str> = recs.iter().map(|r| r.digest.as_str()).collect();
    let models: BTreeSet<&str> = recs
        .iter()
        .flat_map(|r| r.models.iter().map(String::as_str))
        .collect();
    let runs = recs.iter().map(|r| r.run).max().unwrap_or(0);
    let (first, last) = (
        recs.iter().map(|r| r.started_at_ms).min().unwrap_or(0),
        recs.iter()
            .map(|r| r.started_at_ms + r.latency_ms)
            .max()
            .unwrap_or(0),
    );
    let _ = writeln!(o, "# M6's memory exam over the real recall pipeline");
    let _ = writeln!(o);
    let _ = writeln!(o, "- Plan: `docs/m6-ablation-plan.md`, {}.", plan_digest());
    let _ = writeln!(
        o,
        "- Records: `{source}`, {} cells counted ({} lines read).",
        recs.len(),
        records.len()
    );
    let _ = writeln!(
        o,
        "- Exam: {} ({}); scored by {}.",
        exam.file.version,
        exam.digest,
        digests.iter().copied().collect::<Vec<_>>().join(", ")
    );
    let _ = writeln!(
        o,
        "- Models that answered: {}. Runs per item and arm: up to {runs}. Data window: {} to {} (UTC).",
        models.iter().copied().collect::<Vec<_>>().join(", "),
        crate::time::format_local(first, 0),
        crate::time::format_local(last, 0)
    );
    for arm in &arms {
        let rows: Vec<&CellRecall> = recs
            .iter()
            .filter(|r| r.arm == *arm)
            .flat_map(|r| &r.recall)
            .collect();
        let sciences: BTreeSet<&str> = rows.iter().map(|c| c.science.as_str()).collect();
        let sources: BTreeSet<&str> = rows
            .iter()
            .flat_map(|c| c.sources.keys().map(String::as_str))
            .collect();
        let what = match *arm {
            "none" => "today's compiler: the `none` daemon, which asks the index nothing".to_string(),
            "oracle" => "the gold, rendered as the core renders a `Recall` node, sent after the task to the `none` daemon".to_string(),
            _ => format!(
                "the `{arm}` daemon: {} recall rows; science {}; sources that answered: {}",
                rows.len(),
                if sciences.is_empty() { "(none recorded)".into() } else { sciences.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ") },
                if sources.is_empty() { "none".into() } else { sources.into_iter().collect::<Vec<_>>().join(", ") }
            ),
        };
        let _ = writeln!(o, "- `{arm}`: {what}.");
    }
    let _ = writeln!(o);

    let _ = writeln!(o, "## Each arm");
    let _ = writeln!(o);
    let _ = writeln!(o, "The unit is the item: an item's rate is the share of its runs that passed, and an arm's is the mean over items, with a 95% t interval over items; n is the items, then the cells with a verdict.");
    let _ = writeln!(o);
    let _ = writeln!(o, "| arm | all | held in | held out | errors |");
    let _ = writeln!(o, "|---|---|---|---|---|");
    for arm in &arms {
        let mut row = format!("| {arm} |");
        let mut errors = 0;
        for (_, h) in HALVES {
            let r = arm_rate(&cells, arm, |c| h.keeps(c));
            errors = errors.max(r.errors);
            let _ = write!(row, " {} (n = {}, {}) |", rate(&r.rate), r.rate.n, r.cells);
        }
        let _ = writeln!(o, "{row} {errors} |");
    }
    let _ = writeln!(o);

    let _ = writeln!(o, "## Paired differences");
    let _ = writeln!(o);
    let _ = writeln!(o, "`b − a` per item, in points, with its 95% t interval over the items both arms have a verdict on (clustered by item: runs are never counted as samples); W/L/T counts items that gained, lost, or tied; p is an exact two-sided sign test. An interval that holds zero, or that one item cannot give, is `insufficient`.");
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "| difference | half | items | a | b | b − a (pts) | W/L/T | p | says |"
    );
    let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|");
    let mut said: BTreeMap<(&str, &str, &str), (&'static str, f64)> = BTreeMap::new();
    for (a, b) in PAIRS {
        if !arms.contains(&a) || !arms.contains(&b) {
            continue;
        }
        for (name, h) in HALVES {
            let pr = paired(&cells, a, b, |c| h.keeps(c));
            if pr.items == 0 {
                continue;
            }
            let v = verdict(&pr.diff);
            said.insert((a, b, name), (v, pr.diff.mean));
            let _ = writeln!(
                o,
                "| `{b} − {a}` | {name} | {} | {} | {} | {} | {}/{}/{} | {} | {v} |",
                pr.items,
                rate(&pr.a),
                rate(&pr.b),
                diff(&pr.diff),
                pr.gained,
                pr.lost,
                pr.tied,
                p(pr.p_sign)
            );
        }
    }
    let _ = writeln!(o);

    // Cost per pass, tokens, latency, and recall, per arm.
    let _ = writeln!(o, "## Cost and recall");
    let _ = writeln!(o);
    let _ = writeln!(o, "| arm | cells | passes | cost | cost per pass | input tok (mean) | p50 latency | p95 latency | recall p95 | gold admitted |");
    let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|---|");
    let mut total = 0.0;
    let mut recall_p95: BTreeMap<&str, f64> = BTreeMap::new();
    for arm in &arms {
        let rs: Vec<&Record> = recs.iter().filter(|r| r.arm == *arm).collect();
        let n = rs.len() as f64;
        let cost: f64 = rs.iter().map(|r| r.cost_usd).sum();
        let passes = rs.iter().filter(|r| r.pass == Some(true)).count();
        total += cost;
        let mut lat: Vec<f64> = rs.iter().map(|r| r.latency_ms as f64 / 1000.0).collect();
        let mut rlat: Vec<f64> = rs
            .iter()
            .flat_map(|r| r.recall.iter().map(|c| c.total_ms))
            .collect();
        let rp95 = quantile(&mut rlat, 0.95);
        if !rp95.is_nan() {
            recall_p95.insert(arm, rp95);
        }
        let gold: usize = rs
            .iter()
            .filter_map(|r| r.recall.first())
            .map(|c| c.gold_admitted)
            .sum();
        let wanted: usize = rs
            .iter()
            .filter(|r| !r.recall.is_empty())
            .filter_map(|r| exam.item(&r.item))
            .map(|i| i.gold.len())
            .sum();
        let _ = writeln!(
            o,
            "| {arm} | {} | {passes} | ${cost:.4} | {} | {:.0} | {:.1} s | {:.1} s | {} | {} |",
            rs.len(),
            if passes > 0 {
                format!("${:.5}", cost / passes as f64)
            } else {
                "–".into()
            },
            rs.iter().map(|r| r.input_tokens as f64).sum::<f64>() / n.max(1.0),
            quantile(&mut lat.clone(), 0.5),
            quantile(&mut lat, 0.95),
            if rp95.is_nan() {
                "–".into()
            } else {
                format!("{rp95:.0} ms")
            },
            if wanted > 0 {
                format!("{gold} of {wanted}")
            } else {
                "–".into()
            },
        );
    }
    let all_cost: f64 = records.iter().map(|r| r.cost_usd).sum();
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "Spend: ${total:.4} in the cells counted; ${all_cost:.4} over every line read (retries included). Gold admitted counts each cell's first recall: of the gold nodes its items name, how many the pack admitted."
    );
    let _ = writeln!(o);

    decisions(&mut o, &recs, &cells, &arms, &said);
    unmeasured(&mut o, &recs, &arms);
    per_item(&mut o, &recs, exam, &arms);
    let errs: Vec<&Record> = recs.iter().filter(|r| r.error.is_some()).collect();
    if !errs.is_empty() {
        let _ = writeln!(o);
        let _ = writeln!(o, "## Errors (no verdict)");
        let _ = writeln!(o);
        for r in errs {
            let _ = writeln!(
                o,
                "- {} {} r{}: {}",
                r.item,
                r.arm,
                r.run,
                r.error
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
        }
    }
    o
}

/// The features the rule decides, as `(feature, a, b)`: `baseline` (recall
/// itself) against `none`, and vectors, `baseline` against `bm25`.
pub const FEATURES: [(&str, &str, &str); 2] = [
    ("recall (`baseline` against `none`)", "none", "baseline"),
    ("vectors (`baseline` against `bm25`)", "bm25", "baseline"),
];

/// The private family's items that failed under `b` in a run where `a`
/// passed the same run number: what the exam can show of a disclosure.
fn disclosures(recs: &[Record], a: &str, b: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    for r in recs
        .iter()
        .filter(|r| r.arm == b && r.family == "private" && r.pass == Some(false))
    {
        let base_passed = recs
            .iter()
            .any(|x| x.arm == a && x.item == r.item && x.run == r.run && x.pass == Some(true));
        if base_passed {
            out.insert(r.item.clone());
        }
    }
    out.into_iter().collect()
}

/// The decision per feature (§2.9's rule; the plan's "Decision rule"), with
/// the clause that decided it.
fn decisions(
    o: &mut String,
    recs: &[Record],
    cells: &[Cell],
    arms: &[&str],
    said: &BTreeMap<(&str, &str, &str), (&'static str, f64)>,
) {
    let _ = writeln!(o, "## The decision per feature");
    let _ = writeln!(o);
    let _ = writeln!(o, "Under the plan's rule: clause 1, a disclosure the feature caused: off, and a P1 bug; clause 2, on by default only if the canary shows no harm (its one-sided 90% interval excludes a drop of more than 5 points), it gains on a primary metric, the exam's held-out half agrees in sign, and its recall p95 stays within budget; clause 3, otherwise off by default, and \"insufficient\" counts as otherwise.");
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "| feature | decision | clause | why | the exam says (held in; held out) |"
    );
    let _ = writeln!(o, "|---|---|---|---|---|");
    for (name, a, b) in FEATURES {
        if !arms.contains(&a) || !arms.contains(&b) {
            let _ = writeln!(
                o,
                "| {name} | off by default | 3 | not run: the records hold no `{}` cells | – |",
                if arms.contains(&a) { b } else { a }
            );
            continue;
        }
        let exam = |h: &str| -> String {
            match said.get(&(a, b, h)) {
                Some((v, mean)) => format!("{v} ({})", signed_pts(*mean)),
                None => "not run".into(),
            }
        };
        let held_in = exam("held in");
        let held_out = exam("held out");
        let leaked = disclosures(recs, a, b);
        let (decision, clause, why) = if !leaked.is_empty() {
            (
                "off, and a P1 bug",
                "1",
                format!(
                    "private items failed under `{b}` in a run `{a}` passed: {}",
                    leaked.join(", ")
                ),
            )
        } else {
            let pr = paired(cells, a, b, |c| !c.held_out);
            (
                "off by default",
                "3",
                format!(
                    "insufficient: clause 2 needs the canary, which has no data yet (n = 0 sessions per arm; the plan's minimum is 120); the exam alone never turns a feature on (held-in n = {} items)",
                    pr.items
                ),
            )
        };
        let _ = writeln!(
            o,
            "| {name} | {decision} | {clause} | {why} | {held_in}; {held_out} |"
        );
    }
    let _ = writeln!(o);
}

/// What could not be measured, and why.
fn unmeasured(o: &mut String, recs: &[Record], arms: &[&str]) {
    let _ = writeln!(o, "## What could not be measured, and why");
    let _ = writeln!(o);
    let _ = writeln!(o, "- **The canary**: no canary data exists yet (the operator's daemon runs `[memory] mode = \"shadow\"`), so task success, false completion, stale recall, re-supply, and disclosure in live traffic have n = 0, and clause 2 cannot hold for any feature.");
    let vectorless: Vec<&CellRecall> = recs
        .iter()
        .filter(|r| r.arm == "baseline")
        .flat_map(|r| &r.recall)
        .filter(|c| c.skipped.contains_key("vector"))
        .collect();
    let base_rows = recs
        .iter()
        .filter(|r| r.arm == "baseline")
        .map(|r| r.recall.len())
        .sum::<usize>();
    if !vectorless.is_empty() {
        let why: BTreeSet<&str> = vectorless
            .iter()
            .filter_map(|c| c.skipped.get("vector").map(String::as_str))
            .collect();
        let _ = writeln!(
            o,
            "- **Vectors**: `baseline` recalled without vectors in {} of its {base_rows} recalls ({}), so where that is all of them, `baseline` is `bm25` here and `baseline − bm25` measures nothing.",
            vectorless.len(),
            why.into_iter().collect::<Vec<_>>().join("; ")
        );
    }
    if !recs.iter().any(|r| r.held_out) {
        let _ = writeln!(o, "- **The held-out half**: not run in these records, so no feature's sign can be checked against it.");
    }
    for arm in ["none", "bm25", "baseline", "oracle"] {
        if !arms.contains(&arm) {
            let _ = writeln!(o, "- **`{arm}`**: not run in these records.");
        }
    }
    let _ = writeln!(o, "- **The replay** (recall and precision at k, MRR, and the stale rate against the silver labels): not in these records. It is instrument 2, run over a copy of a store's recorded turns (`theseus-exam replay`), and never decides alone.");
    let _ = writeln!(o, "- **Recall's own spend**: the arms here spend nothing of their own (no rerank, no syntheses), so cost per pass is the model's alone.");
    let _ = writeln!(o);
}

/// Each item, by arm: passes of runs, and what failed.
fn per_item(o: &mut String, recs: &[Record], exam: &Exam, arms: &[&str]) {
    let _ = writeln!(o, "## Each item");
    let _ = writeln!(o);
    let mut head = "| item | family | half |".to_string();
    let mut rule = "|---|---|---|".to_string();
    for a in arms {
        let _ = write!(head, " {a} |");
        rule.push_str("---|");
    }
    let _ = writeln!(o, "{head} what failed |");
    let _ = writeln!(o, "{rule}---|");
    for item in &exam.file.items {
        let mut row = format!(
            "| {} | {} | {} |",
            item.id,
            item.family.as_str(),
            if item.held_out { "held out" } else { "held in" }
        );
        let mut any = false;
        let mut fails = Vec::new();
        for arm in arms {
            let rs: Vec<&Record> = recs
                .iter()
                .filter(|r| r.item == item.id && r.arm == *arm && r.pass.is_some())
                .collect();
            any |= !rs.is_empty();
            let passes = rs.iter().filter(|r| r.pass == Some(true)).count();
            let _ = write!(row, " {passes}/{} |", rs.len());
            let mut f: BTreeMap<&str, usize> = BTreeMap::new();
            for r in &rs {
                for l in r.lines.iter().filter(|l| !l.pass) {
                    *f.entry(l.line.as_str()).or_default() += 1;
                }
            }
            fails.extend(f.into_iter().map(|(l, k)| format!("{arm} {k}× `{l}`")));
        }
        if any {
            let _ = writeln!(o, "{row} {} |", fails.join("; ").replace('|', "\\|"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::EXAM_V2;

    fn rec(item: &str, arm: &str, run: u32, pass: Option<bool>) -> Record {
        Record {
            item: item.into(),
            family: "fact".into(),
            arm: arm.into(),
            run,
            pass,
            ..Default::default()
        }
    }

    #[test]
    fn the_last_verdict_counts_and_an_error_never_hides_one() {
        let rs = vec![
            rec("a", "none", 1, None),
            rec("a", "none", 1, Some(false)),
            rec("a", "none", 1, None),
            rec("b", "none", 1, Some(true)),
            rec("b", "none", 1, Some(false)),
            rec("c", "none", 1, None),
        ];
        let l = latest(&rs);
        let get = |i: &str| l.iter().find(|r| r.item == i).unwrap().pass;
        assert_eq!(
            (get("a"), get("b"), get("c")),
            (Some(false), Some(false), None)
        );
    }

    #[test]
    fn rescoring_applies_the_given_checks_to_stored_replies() {
        let exam = Exam::parse(EXAM_V2).unwrap();
        let mut rs = vec![Record {
            reply: "It listens on 7433.".into(),
            ..rec("fact-1", "oracle", 1, Some(false))
        }];
        rescore(&mut rs, &exam);
        assert_eq!(rs[0].pass, Some(true));
        assert_eq!(rs[0].digest, exam.digest);
    }

    /// Four held-in items (3 runs) and two held-out (1 run) under four
    /// arms, every cell costing a cent: see `the_report_reads_as_computed_by_hand`.
    fn fixture(exam: &Exam) -> Vec<Record> {
        let held_in: Vec<&str> = exam
            .file
            .items
            .iter()
            .filter(|i| !i.held_out)
            .take(4)
            .map(|i| i.id.as_str())
            .collect();
        let held_out: Vec<&str> = exam
            .file
            .items
            .iter()
            .filter(|i| i.held_out)
            .take(2)
            .map(|i| i.id.as_str())
            .collect();
        // Passes by run, per held-in item: none passes only the fourth;
        // bm25 3, 2, 0 and 3 of 3; baseline 3, 3, 1 and 3; oracle every run.
        let wins: [(&str, [[bool; 3]; 4]); 4] = [
            ("none", [[false; 3], [false; 3], [false; 3], [true; 3]]),
            (
                "bm25",
                [[true; 3], [true, true, false], [false; 3], [true; 3]],
            ),
            (
                "baseline",
                [[true; 3], [true; 3], [true, false, false], [true; 3]],
            ),
            ("oracle", [[true; 3]; 4]),
        ];
        // Held out, once: none fails both, bm25 and baseline pass the first,
        // oracle both.
        let out: [(&str, [bool; 2]); 4] = [
            ("none", [false, false]),
            ("bm25", [true, false]),
            ("baseline", [true, false]),
            ("oracle", [true, true]),
        ];
        let mut rs = Vec::new();
        for (arm, items) in wins {
            for (k, runs) in items.iter().enumerate() {
                for (r, pass) in runs.iter().enumerate() {
                    rs.push(Record {
                        cost_usd: 0.01,
                        ..rec(held_in[k], arm, r as u32 + 1, Some(*pass))
                    });
                }
            }
        }
        for (arm, passes) in out {
            for (k, pass) in passes.iter().enumerate() {
                rs.push(Record {
                    cost_usd: 0.01,
                    held_out: true,
                    ..rec(held_out[k], arm, 1, Some(*pass))
                });
            }
        }
        rs
    }

    /// Every number below is computed by hand from the fixture, with
    /// t(1) = 12.706, t(3) = 3.1824 and t(5) = 2.5706.
    #[test]
    fn the_report_reads_as_computed_by_hand() {
        let exam = Exam::parse(EXAM_V2).unwrap();
        let md = render(&fixture(&exam), &exam, "x.jsonl");
        let has = |s: &str| assert!(md.contains(s), "no {s:?} in:\n{md}");
        has(&format!(
            "- Plan: `docs/m6-ablation-plan.md`, {}.",
            plan_digest()
        ));
        // none, over all six items: one passes, so 1/6 = 17%; sd √(1/6) =
        // 0.408, se 0.167, half-width 0.428: [0, 60]. Held in: 1/4 = 25%, sd
        // 0.5, se 0.25, half-width 0.796: [0, 100]. Held out: none passes.
        // n is the items, then the cells: 4 × 3 + 2 = 14.
        has("| none | 17% [0, 60] (n = 6, 14) | 25% [0, 100] (n = 4, 12) | 0% [0, 0] (n = 2, 2) | 0 |");
        // baseline held in: 1, 1, 1/3, 1: mean 83%, sd 1/3, se 1/6,
        // half-width 0.530: [30, 100].
        has("| baseline | ");
        has("| 83% [30, 100] (n = 4, 12) |");
        // baseline − none, held in: 1, 1, 1/3, 0: mean +58, sd 0.5, se 0.25,
        // half-width 0.796: [-21, +100], which holds zero. Sign test 3 to 0:
        // 2/8 = 0.250.
        has("| `baseline − none` | held in | 4 | 25% [0, 100] | 83% [30, 100] | +58 [-21, +100] | 3/0/1 | 0.250 | insufficient |");
        // Held out, two items: +1 and 0, mean +50, se 0.5, t(1) 12.7: [-100, +100].
        has("| `baseline − none` | held out | 2 | 0% [0, 0] | 50% [0, 100] | +50 [-100, +100] | 1/0/1 | 1.000 | insufficient |");
        // oracle − none over all six: 1, 1, 1, 0, 1, 1: mean +83, sd 0.408,
        // se 0.167, half-width 0.428: [+40, +100], above zero.
        has("| `oracle − none` | all | 6 | 17% [0, 60] | 100% [100, 100] | +83 [+40, +100] | 5/0/1 |");
        assert!(
            md.lines()
                .any(|l| l.starts_with("| `oracle − none` | all |") && l.ends_with("| gain |")),
            "{md}"
        );
        // baseline − bm25 held in: 0, 1/3, 1/3, 0: mean +17, sd 0.192, se
        // 0.0962, half-width 0.306: [-14, +47].
        has("| `baseline − bm25` | held in | 4 | 67% [");
        has("| +17 [-14, +47] | 2/0/2 | 0.500 | insufficient |");
        // Cost per pass: none's 14 cells cost $0.14 for 3 passes.
        has("| none | 14 | 3 | $0.1400 | $0.04667 |");
        // oracle's 14 for 14.
        has("| oracle | 14 | 14 | $0.1400 | $0.01000 |");
        // The decisions: no canary, so clause 3, with the exam's n.
        has("| recall (`baseline` against `none`) | off by default | 3 | insufficient: clause 2 needs the canary, which has no data yet (n = 0 sessions per arm; the plan's minimum is 120); the exam alone never turns a feature on (held-in n = 4 items) | insufficient (+58); insufficient (+50) |");
        has("| vectors (`baseline` against `bm25`) | off by default | 3 |");
        has("- **The canary**: no canary data exists yet");
        has("- **The replay**");
    }

    /// A private item `baseline` fails in a run `none` passed is clause 1.
    #[test]
    fn a_disclosure_the_feature_caused_is_clause_one() {
        let exam = Exam::parse(EXAM_V2).unwrap();
        let private = exam
            .file
            .items
            .iter()
            .find(|i| i.family == crate::item::Family::Private)
            .unwrap();
        let mut rs = Vec::new();
        for (arm, pass) in [("none", true), ("bm25", true), ("baseline", false)] {
            let mut r = rec(&private.id, arm, 1, Some(pass));
            r.family = "private".into();
            rs.push(r);
        }
        let md = render(&rs, &exam, "x.jsonl");
        assert!(
            md.contains(&format!(
                "| recall (`baseline` against `none`) | off, and a P1 bug | 1 | private items failed under `baseline` in a run `none` passed: {} |",
                private.id
            )),
            "{md}"
        );
        assert!(
            md.contains("| vectors (`baseline` against `bm25`) | off, and a P1 bug | 1 |"),
            "{md}"
        );
        assert!(
            md.contains("- **`oracle`**: not run in these records."),
            "{md}"
        );
        assert!(md.contains("- **The held-out half**: not run"), "{md}");
    }

    #[test]
    fn an_interval_that_holds_zero_or_is_missing_cannot_decide() {
        let i = |lo: Option<f64>, hi: Option<f64>| Interval {
            n: 2,
            mean: 0.1,
            sd: 0.1,
            se: 0.1,
            lo,
            hi,
        };
        assert_eq!(verdict(&i(Some(0.01), Some(0.3))), "gain");
        assert_eq!(verdict(&i(Some(-0.3), Some(-0.01))), "loss");
        assert_eq!(verdict(&i(Some(-0.1), Some(0.3))), "insufficient");
        assert_eq!(verdict(&i(Some(0.0), Some(0.3))), "insufficient");
        assert_eq!(verdict(&i(None, None)), "insufficient");
    }

    #[test]
    fn a_report_is_frozen() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("report.md");
        freeze(&p, "first").unwrap();
        let e = freeze(&p, "second").unwrap_err().to_string();
        assert!(e.contains("frozen"), "{e}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "first");
    }
}
