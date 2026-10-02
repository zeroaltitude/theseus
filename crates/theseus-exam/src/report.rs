//! The headroom report: `oracle` against `none`, overall, per family, and per
//! half (tuning and held out), with intervals (`stats.rs`), then each item,
//! the spend, the latency, and what failed. Markdown, for the lane's report.
//!
//! When a cell ran more than once (an errored cell runs again on resume), its
//! last record with a verdict counts; without one, its last record.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::drive::Record;
use crate::item::{Exam, Family};
use crate::stats::{paired, Cell, Interval, Paired};

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

fn row(name: &str, pr: &Paired) -> String {
    format!(
        "| {name} | {} | {} | {} | {} | {}/{}/{} | {} |",
        pr.items,
        rate(&pr.a),
        rate(&pr.b),
        diff(&pr.diff),
        pr.gained,
        pr.lost,
        pr.tied,
        p(pr.p_sign)
    )
}

fn quantile(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let i = ((v.len() as f64 - 1.0) * q).round() as usize;
    v[i]
}

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
pub fn render(records: &[Record], exam: &Exam, source: &str) -> String {
    let recs = latest(records);
    let cells: Vec<Cell> = recs
        .iter()
        .map(|r| Cell {
            item: r.item.clone(),
            group: r.family.clone(),
            held_out: r.held_out,
            arm: r.arm.clone(),
            pass: r.pass,
        })
        .collect();
    let mut o = String::new();
    let digests: std::collections::BTreeSet<&str> =
        recs.iter().map(|r| r.digest.as_str()).collect();
    let models: std::collections::BTreeSet<&str> = recs
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
        "- Models that answered: {}. Runs per item and arm: up to {runs}. Wall clock: {} to {} (MST).",
        models.iter().copied().collect::<Vec<_>>().join(", "),
        crate::time::format_local(first, -420),
        crate::time::format_local(last, -420)
    );
    let all = paired(&cells, "none", "oracle", |_| true);
    let _ = writeln!(
        o,
        "- Cells with a verdict: none {}, oracle {}; errors (no verdict) {}; items paired {} (unpaired {}).",
        all.runs_a, all.runs_b, all.errors, all.items, all.unpaired
    );
    let _ = writeln!(o);
    let _ = writeln!(o, "Pass rates are per item (the share of its runs that passed), averaged over items, with a 95% t interval over items; headroom is `oracle − none` per item, in points, with its interval; W/L/T counts items that gained, lost, or tied; p is an exact two-sided sign test over the items that moved.");
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "| | items | none | oracle | headroom (pts) | W/L/T | p |"
    );
    let _ = writeln!(o, "|---|---|---|---|---|---|---|");
    let _ = writeln!(o, "{}", row("**all**", &all));
    let tune = paired(&cells, "none", "oracle", |c| !c.held_out);
    let held = paired(&cells, "none", "oracle", |c| c.held_out);
    let _ = writeln!(o, "{}", row("tuning half", &tune));
    let _ = writeln!(o, "{}", row("held-out half", &held));
    let needs = paired(&cells, "none", "oracle", |c| c.group != "needs_nothing");
    let _ = writeln!(o, "{}", row("all but needs-nothing", &needs));
    for f in Family::ALL {
        let pr = paired(&cells, "none", "oracle", |c| c.group == f.as_str());
        if pr.items > 0 {
            let _ = writeln!(o, "{}", row(f.as_str(), &pr));
        }
    }
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "Consistency: {} of {} item-and-arm cells had runs that disagreed.",
        all.mixed, all.cells
    );
    let _ = writeln!(o);

    // Spend, tokens, latency, per arm.
    let _ = writeln!(o, "| arm | cells | cost | mean cost | input tok (mean) | output tok (mean) | p50 latency | p95 latency | mean loops | declined |");
    let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|---|");
    let mut total = 0.0;
    for arm in ["none", "oracle"] {
        let rs: Vec<&Record> = recs.iter().filter(|r| r.arm == arm).collect();
        if rs.is_empty() {
            continue;
        }
        let n = rs.len() as f64;
        let cost: f64 = rs.iter().map(|r| r.cost_usd).sum();
        total += cost;
        let mut lat: Vec<f64> = rs.iter().map(|r| r.latency_ms as f64 / 1000.0).collect();
        let _ = writeln!(
            o,
            "| {arm} | {} | ${cost:.4} | ${:.5} | {:.0} | {:.0} | {:.1} s | {:.1} s | {:.2} | {} |",
            rs.len(),
            cost / n,
            rs.iter().map(|r| r.input_tokens as f64).sum::<f64>() / n,
            rs.iter().map(|r| r.output_tokens as f64).sum::<f64>() / n,
            quantile(&mut lat.clone(), 0.5),
            quantile(&mut lat, 0.95),
            rs.iter().map(|r| r.loops as f64).sum::<f64>() / n,
            rs.iter().map(|r| r.declined).sum::<u32>(),
        );
    }
    let all_cost: f64 = records.iter().map(|r| r.cost_usd).sum();
    let _ = writeln!(o);
    let _ = writeln!(
        o,
        "Spend: ${total:.4} in the cells counted; ${all_cost:.4} over every line read (retries included)."
    );
    let _ = writeln!(o);

    // Per item.
    let _ = writeln!(
        o,
        "| item | family | half | none | oracle | Δ | what failed (oracle, then none) |"
    );
    let _ = writeln!(o, "|---|---|---|---|---|---|---|");
    for item in &exam.file.items {
        let of = |arm: &str| -> (usize, usize, BTreeMap<String, usize>) {
            let mut fails = BTreeMap::new();
            let rs: Vec<&Record> = recs
                .iter()
                .filter(|r| r.item == item.id && r.arm == arm && r.pass.is_some())
                .collect();
            for r in &rs {
                for l in r.lines.iter().filter(|l| !l.pass) {
                    *fails.entry(l.line.clone()).or_default() += 1;
                }
            }
            (
                rs.iter().filter(|r| r.pass == Some(true)).count(),
                rs.len(),
                fails,
            )
        };
        let (np, nn, nf) = of("none");
        let (op, on, of_) = of("oracle");
        if nn + on == 0 {
            continue;
        }
        let d = if nn > 0 && on > 0 {
            signed_pts(op as f64 / on as f64 - np as f64 / nn as f64)
        } else {
            "–".into()
        };
        let fails: Vec<String> = of_
            .iter()
            .map(|(l, k)| format!("oracle {k}× `{l}`"))
            .chain(nf.iter().map(|(l, k)| format!("none {k}× `{l}`")))
            .collect();
        let _ = writeln!(
            o,
            "| {} | {} | {} | {np}/{nn} | {op}/{on} | {d} | {} |",
            item.id,
            item.family.as_str(),
            if item.held_out { "held out" } else { "tuning" },
            fails.join("; ").replace('|', "\\|")
        );
    }
    let errs: Vec<&Record> = recs.iter().filter(|r| r.error.is_some()).collect();
    if !errs.is_empty() {
        let _ = writeln!(o);
        let _ = writeln!(o, "Errors (no verdict):");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::EXAM_V1;

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
        let exam = Exam::parse(EXAM_V1).unwrap();
        let mut rs = vec![Record {
            reply: "It listens on 7433.".into(),
            ..rec("fact-1", "oracle", 1, Some(false))
        }];
        rescore(&mut rs, &exam);
        assert_eq!(rs[0].pass, Some(true));
        assert_eq!(rs[0].digest, exam.digest);
    }

    #[test]
    fn the_report_names_the_headroom_and_each_family() {
        let exam = Exam::parse(EXAM_V1).unwrap();
        let mut rs = Vec::new();
        for (i, item) in exam.file.items.iter().enumerate() {
            for run in 1..=3 {
                let mut a = rec(&item.id, "none", run, Some(i % 4 == 0));
                a.family = item.family.as_str().into();
                let mut b = rec(&item.id, "oracle", run, Some(true));
                b.family = item.family.as_str().into();
                rs.push(a);
                rs.push(b);
            }
        }
        let md = render(&rs, &exam, "x.jsonl");
        // By hand: 10 of 40 items pass under none (every fourth), every item
        // under oracle. The sd of ten ones and thirty zeros is
        // √(0.25 × 0.75 × 40/39) = 0.43853, the se 0.069338, and t(39) is
        // 2.0227, so the half-width is 14.0 points: none 25% [11, 39], and
        // the headroom +75 [+61, +89], 30 gained, 10 tied.
        assert!(md.contains("| **all** | 40 | 25% [11, 39] | 100% [100, 100] | +75 [+61, +89] | 30/0/10 | <0.001 |"), "{md}");
        for f in Family::V1 {
            assert!(md.contains(&format!("| {} | 4 |", f.as_str())), "{f:?}");
        }
        for f in Family::HARD {
            assert!(!md.contains(&format!("| {} |", f.as_str())), "{f:?}");
        }
        assert!(
            md.contains("| fact-1 | fact | tuning | 3/3 | 3/3 | +0 |"),
            "{md}"
        );
    }
}
