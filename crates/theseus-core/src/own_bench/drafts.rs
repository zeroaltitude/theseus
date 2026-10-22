//! The report's draft columns (theseus-a1wvq.12): what a China-origin draft
//! and its Claude review cost together, and why a class that drafts did not.
//! Read only, over the `task.draft` rows and the `draft_skip` word of
//! `route.decided`; a key a row lacks reads its default, never an error.

use std::collections::BTreeMap;

use theseus_protocol::own_bench::{DraftCell, GateCount};

use super::report::Turn;

/// A `task.draft` row, as the report reads it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Draft {
    pub draft_model: String,
    /// `none` when no review ran.
    pub review_model: String,
    pub draft_usd: f64,
    pub review_usd: f64,
    pub lines_changed: u64,
    pub applied: bool,
    pub draft_outcome: String,
}

impl Draft {
    pub fn of_row(d: &serde_json::Value) -> Draft {
        let s = |k: &str| d[k].as_str().map(str::to_string);
        Draft {
            draft_model: s("draft_model").unwrap_or_default(),
            review_model: s("review_model").unwrap_or_else(|| "none".into()),
            draft_usd: d["draft_cost_usd"].as_f64().unwrap_or(0.0),
            review_usd: d["review_cost_usd"].as_f64().unwrap_or(0.0),
            lines_changed: d["lines_changed"].as_u64().unwrap_or(0),
            applied: d["applied"].as_bool().unwrap_or(false),
            draft_outcome: s("draft_outcome").unwrap_or_default(),
        }
    }
}

/// One cell per (draft model, review model), sorted by that pair.
pub fn drafts(rows: &[Draft]) -> Vec<DraftCell> {
    let mut by: BTreeMap<(&str, &str), DraftCell> = BTreeMap::new();
    for r in rows {
        let c = by
            .entry((&r.draft_model, &r.review_model))
            .or_insert_with(|| DraftCell {
                draft_model: r.draft_model.clone(),
                review_model: r.review_model.clone(),
                ..DraftCell::default()
            });
        c.drafts += 1;
        c.applied += u64::from(r.applied);
        c.reviewed += u64::from(r.review_model != "none");
        c.draft_usd += r.draft_usd;
        c.review_usd += r.review_usd;
        c.lines_changed += r.lines_changed;
    }
    let mut out: Vec<DraftCell> = by.into_values().collect();
    for c in &mut out {
        c.total_usd = c.draft_usd + c.review_usd;
    }
    out
}

/// Turns per `draft_skip` word, sorted; a turn with no word is not counted.
pub fn skips(turns: &[Turn]) -> Vec<GateCount> {
    let mut by: BTreeMap<&str, u64> = BTreeMap::new();
    for w in turns.iter().filter_map(|t| t.data.draft_skip.as_deref()) {
        *by.entry(w).or_default() += 1;
    }
    by.into_iter()
        .map(|(why, turns)| GateCount {
            why: why.into(),
            turns,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::own_bench::report::Data;
    use serde_json::json;

    fn row(
        draft: &str,
        review: Option<&str>,
        d: f64,
        r: f64,
        lines: Option<u64>,
        applied: bool,
    ) -> Draft {
        let mut v = json!({
            "draft_model": draft, "review_model": review,
            "draft_cost_usd": d, "review_cost_usd": review.map(|_| r),
            "applied": applied, "draft_outcome": "complete",
        });
        if let Some(n) = lines {
            v["lines_changed"] = json!(n);
        }
        Draft::of_row(&v)
    }

    #[test]
    fn the_report_counts_drafts_and_their_review_cost() {
        let rows = [
            row("dsflash", Some("sonnet"), 0.05, 0.25, Some(100), true),
            row("dsflash", Some("sonnet"), 0.0625, 0.25, Some(112), true),
            row("dsflash", None, 0.0625, 0.0, Some(0), false),
            row("dsflashmax", Some("sonnethi"), 0.5, 1.0, None, false),
        ];
        let got = drafts(&rows);
        let key: Vec<_> = got
            .iter()
            .map(|c| (&*c.draft_model, &*c.review_model))
            .collect();
        assert_eq!(
            key,
            [
                ("dsflash", "none"),
                ("dsflash", "sonnet"),
                ("dsflashmax", "sonnethi")
            ]
        );
        let n = &got[0];
        assert_eq!(
            (n.drafts, n.applied, n.reviewed, n.lines_changed),
            (1, 0, 0, 0)
        );
        assert_eq!(
            (n.draft_usd, n.review_usd, n.total_usd),
            (0.0625, 0.0, 0.0625)
        );
        let s = &got[1];
        assert_eq!(
            (s.drafts, s.applied, s.reviewed, s.lines_changed),
            (2, 2, 2, 212)
        );
        assert!((s.draft_usd - 0.1125).abs() < 1e-9);
        assert!((s.review_usd - 0.5).abs() < 1e-9);
        assert!((s.total_usd - 0.6125).abs() < 1e-9);
        let m = &got[2];
        assert_eq!((m.drafts, m.applied, m.reviewed), (1, 0, 1));
        assert_eq!(m.lines_changed, 0, "a row missing lines_changed reads 0");
        assert_eq!((m.draft_usd, m.review_usd, m.total_usd), (0.5, 1.0, 1.5));
    }

    #[test]
    fn the_report_counts_draft_skips_per_word() {
        let t = |w: Option<&str>| Turn {
            model: "m".into(),
            effort: String::new(),
            cost_usd: 0.0,
            checked: false,
            data: Data {
                draft_skip: w.map(Into::into),
                ..Data::default()
            },
        };
        let turns = [
            t(Some("not_git")),
            t(Some("dir_not_open")),
            t(None),
            t(Some("dir_not_open")),
            t(None),
        ];
        let got: Vec<_> = skips(&turns)
            .into_iter()
            .map(|g| (g.why, g.turns))
            .collect();
        assert_eq!(
            got,
            [("dir_not_open".to_string(), 2), ("not_git".to_string(), 1)]
        );
        assert!(skips(&turns[2..3]).is_empty());
    }
}
