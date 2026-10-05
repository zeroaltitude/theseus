//! `theseus judge replay`, `audit`, and `backfill` (M5 25d; design §2.9):
//! the owner's runs over the learning ledger. Each spends money, and a
//! backfill sends his history to Jev, so the daemon judges each as an answer
//! (the owner, from a private place) and the client refuses each inside a
//! job (`client::refuse_in_a_job`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use theseus_client::{render, Conn};
use theseus_protocol::judge_runs::{
    JudgeAuditParams, JudgeAuditResult, JudgeBackfillParams, JudgeBackfillResult, JudgeLearnParams,
    JudgeProposal, JudgeReplayParams, JudgeReplayResult,
};
use theseus_protocol::method;

use crate::cmd::output;

#[derive(Args, Debug)]
pub struct ReplayArgs {
    /// An embedded version the build does not wire (`loop.v2`); or `--pack-file`.
    candidate: Option<String>,
    /// A pack file to replay: its text is sent, and the daemon opens no path.
    #[arg(long, value_name = "PATH")]
    pack_file: Option<PathBuf>,
    /// The report whose holdout is the set (`rpt_2026-10-04_loop.v1`).
    #[arg(long, value_name = "ID")]
    report: Option<String>,
    /// The report's `holdout` (the default) or its `train` split.
    #[arg(long)]
    split: Option<String>,
    /// Only the labeled judgments the incumbent got wrong.
    #[arg(long)]
    errors: bool,
    /// The set by id instead (`jdg_…`, comma-separated).
    #[arg(long, value_delimiter = ',')]
    judgments: Vec<String>,
}

#[derive(Args, Debug)]
pub struct AuditArgs {
    /// The pack version (`loop.v1`), or its id for the wired version.
    pack: String,
    /// How many answered judgments without an audit label to sample.
    #[arg(long, default_value_t = 100)]
    sample: u32,
    /// The model profile that answers (`[profiles.<name>]`).
    #[arg(long)]
    profile: String,
    /// The sample's seed (none: the run's).
    #[arg(long)]
    seed: Option<u64>,
}

#[derive(Args, Debug)]
pub struct BackfillArgs {
    /// The pack version (`loop.v1`), or its id for the wired version.
    pack: String,
    /// The local day the window starts (`2026-10-01`); it ends now.
    #[arg(long)]
    since: String,
}

fn print(lines: Vec<String>) {
    for l in lines {
        println!("{l}");
    }
}

pub async fn replay(conn: &mut Conn, json: bool, a: ReplayArgs) -> Result<()> {
    let pack_text = match &a.pack_file {
        Some(path) => Some(
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
        ),
        None => None,
    };
    let p = JudgeReplayParams {
        candidate: a.candidate,
        pack_text,
        report: a.report,
        split: a.split,
        errors: a.errors,
        judgments: a.judgments,
    };
    let v = conn
        .request(method::JUDGE_REPLAY, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: JudgeReplayResult| {
        print(render::judge_replay_lines(&r));
        Ok(())
    })
}

pub async fn audit(conn: &mut Conn, json: bool, a: AuditArgs) -> Result<()> {
    let p = JudgeAuditParams {
        pack: a.pack,
        sample: a.sample,
        profile: a.profile,
        seed: a.seed,
    };
    let v = conn
        .request(method::JUDGE_AUDIT, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: JudgeAuditResult| {
        print(render::judge_audit_lines(&r));
        Ok(())
    })
}

pub async fn backfill(conn: &mut Conn, json: bool, a: BackfillArgs) -> Result<()> {
    let p = JudgeBackfillParams {
        pack: a.pack,
        since: a.since,
    };
    let v = conn
        .request(method::JUDGE_BACKFILL, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: JudgeBackfillResult| {
        print(render::judge_backfill_lines(&r));
        Ok(())
    })
}

#[derive(Args, Debug)]
pub struct LearnArgs {
    /// The pack version (`classify.v1`), or its id for its one wired version.
    pack: String,
    /// The boundary: train before it, holdout from it until now (unix ms, an RFC 3339
    /// time, a local day, or a duration ago such as `2h`). Without it, the nightly rule.
    #[arg(long, value_name = "TIME")]
    split: Option<String>,
}

/// A proposal in lines: the decision and why, the numbers, the thresholds,
/// and the diff.
pub fn proposal_lines(p: &JudgeProposal) -> Vec<String> {
    let mut out = vec![format!(
        "{} {} ({}): {} — {}",
        p.id,
        p.version.as_deref().unwrap_or(&p.parent),
        p.trigger,
        p.decision,
        p.why
    )];
    if !p.said.is_empty() {
        out.push(format!("  {}", p.said));
    }
    let split = match (p.split_start_ms, p.split_end_ms) {
        (Some(a), Some(b)) => format!("{} [{a}, {b})", p.split),
        _ => p.split.clone(),
    };
    out.push(format!(
        "  parent {}; split {split}: {} train, {} holdout labeled; {} new errors, {} read",
        p.parent,
        p.train,
        p.holdout,
        p.new_errors,
        p.errors.len()
    ));
    if let Some(r) = &p.replay {
        out.push(format!(
            "  replay {r}: {} train errors fixed, {} broken; writer ${:.4}, replay ${:.4}; {}",
            p.fixed,
            p.broken,
            p.writer_usd,
            p.replay_usd,
            if p.sufficient {
                "at the minimum"
            } else {
                "below the minimum"
            }
        ));
    }
    let n = |x: Option<f64>| x.map_or("n/a".to_string(), |v| format!("{v:.2}"));
    for q in &p.parent_holdout {
        let c = p
            .candidate_holdout
            .iter()
            .find(|c| c.question == q.question);
        out.push(format!(
            "  {}: precision {} → {}, recall {} → {} ({} labeled)",
            q.question,
            n(q.precision),
            n(c.and_then(|c| c.precision)),
            n(q.recall),
            n(c.and_then(|c| c.recall)),
            q.labeled
        ));
    }
    for t in p.thresholds.iter().filter(|t| t.act != t.act_was) {
        out.push(format!(
            "  {}: act {:.2} → {:.2} (re-fit on {} train answers)",
            t.question, t.act_was, t.act, t.train
        ));
    }
    if let Some(q) = &p.question {
        out.push(format!(
            "  your card: theseus confirm {q} --approve (or --decline)"
        ));
    }
    if !p.diff.is_empty() {
        out.push("  diff:".into());
        out.extend(p.diff.lines().map(|l| format!("    {l}")));
    }
    out
}

pub async fn learn(conn: &mut Conn, json: bool, a: LearnArgs) -> Result<()> {
    let p = JudgeLearnParams {
        pack: a.pack,
        split: a.split,
    };
    let v = conn
        .request(method::JUDGE_LEARN, serde_json::to_value(&p)?)
        .await?;
    output(json, v, |r: JudgeProposal| {
        print(proposal_lines(&r));
        Ok(())
    })
}
