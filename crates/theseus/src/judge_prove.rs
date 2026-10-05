//! `theseus judge prove` (M5 L3, roadmap row 50; design §2.9, "The prove"):
//! the exit report for `loop.v1`'s canary, from the daemon's ledger. The
//! report's Markdown goes to stdout exactly as `theseus-judge prove` writes
//! it over the same records (`--records` writes them), so the two compare
//! byte for byte; what the daemon read (the window, the records by arm, the
//! tasks left out and why) goes to stderr. A read: anyone may run it.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use theseus_client::Conn;
use theseus_protocol::judge_runs::{JudgeProveParams, JudgeProveResult};
use theseus_protocol::method;

use crate::cmd::output;

#[derive(Args, Debug)]
pub struct ProveArgs {
    /// The first local day of tasks that ended (`2026-10-01`). None: since loop.v1's latest move
    /// to canary, else every task.
    #[arg(long)]
    since: Option<String>,
    /// The last local day of tasks that ended, whole (`2026-10-04`). None: up to now.
    #[arg(long)]
    until: Option<String>,
    /// Labeled tasks each arm needs before a rate is stated (the generator's default: 30).
    #[arg(long, value_name = "N")]
    min_tasks: Option<u32>,
    /// Labeled items each precision, recall, or rate needs (the generator's default).
    #[arg(long, value_name = "N")]
    min_labeled: Option<u32>,
    /// Write the records, as JSON lines `theseus-judge prove` reads, to this file (`-`: stdout,
    /// instead of the report).
    #[arg(long, value_name = "PATH")]
    records: Option<PathBuf>,
}

/// What the daemon read, in lines: the window, the records by arm, the tasks
/// left out by reason, and the notes.
pub fn read_lines(r: &JudgeProveResult) -> Vec<String> {
    let counts = |m: &std::collections::BTreeMap<String, u32>| {
        m.iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = vec![
        format!("{}: {}", r.pack, r.window),
        format!(
            "{} finished tasks read; records by arm: {}",
            r.tasks,
            counts(&r.arms)
        ),
    ];
    if !r.left_out.is_empty() {
        out.push(format!("left out: {}", counts(&r.left_out)));
    }
    for q in &r.classification {
        let share = |k: u32, rate: Option<f64>| match rate {
            Some(v) => format!("{k} ({:.1}%)", v * 100.0),
            None => k.to_string(),
        };
        out.push(format!(
            "{} {} against its baseline: {} labeled, {} compared; Jev right {}, baseline right {}: {}{}",
            q.pack,
            q.question,
            q.labeled,
            q.compared,
            share(q.jev_right, q.jev_rate),
            share(q.baseline_right, q.baseline_rate),
            q.verdict,
            q.insufficient
                .as_deref()
                .map(|w| format!(" ({w})"))
                .unwrap_or_default()
        ));
    }
    out.extend(r.notes.iter().map(|n| format!("note: {n}")));
    out.push(format!("built in {} ms", r.elapsed_ms));
    out
}

pub async fn prove(conn: &mut Conn, json: bool, a: ProveArgs) -> Result<()> {
    let p = JudgeProveParams {
        since: a.since,
        until: a.until,
        min_tasks: a.min_tasks,
        min_labeled: a.min_labeled,
        records: a.records.is_some(),
    };
    let v = conn
        .request(method::JUDGE_PROVE, serde_json::to_value(&p)?)
        .await?;
    let to = a.records;
    output(json, v, |r: JudgeProveResult| {
        for line in read_lines(&r) {
            eprintln!("{line}");
        }
        let records = r.records.unwrap_or_default();
        match to.as_deref() {
            Some(path) if path.as_os_str() == "-" => {
                print!("{records}");
                return Ok(());
            }
            Some(path) => std::fs::write(path, &records)
                .with_context(|| format!("writing {}", path.display()))?,
            None => {}
        }
        print!("{}", r.markdown);
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use theseus_protocol::judge_runs::ClassifyQuality;

    /// What the daemon read is said in plain lines: the window, both arms
    /// even when empty, each reason a task was left out, and the notes.
    #[test]
    fn what_was_read_is_said() {
        let r = JudgeProveResult {
            pack: "loop.v1".into(),
            window: "every task that ended: loop.v1 has not moved to canary".into(),
            tasks: 4,
            arms: [("canary".into(), 1), ("control".into(), 0)].into(),
            left_out: [("never_judged".into(), 3)].into(),
            notes: vec!["nudges are 0".into()],
            classification: vec![ClassifyQuality {
                pack: "classify.v1".into(),
                question: "should_promote".into(),
                labeled: 5,
                compared: 4,
                jev_right: 3,
                baseline_right: 2,
                verdict: "insufficient".into(),
                insufficient: Some("compared 4 of 30".into()),
                ..ClassifyQuality::default()
            }],
            elapsed_ms: 7,
            ..JudgeProveResult::default()
        };
        assert_eq!(
            read_lines(&r),
            vec![
                "loop.v1: every task that ended: loop.v1 has not moved to canary",
                "4 finished tasks read; records by arm: canary 1, control 0",
                "left out: never_judged 3",
                "classify.v1 should_promote against its baseline: 5 labeled, 4 compared; Jev right 3, \
                 baseline right 2: insufficient (compared 4 of 30)",
                "note: nudges are 0",
                "built in 7 ms",
            ]
        );
    }
}
