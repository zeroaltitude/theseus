//! `theseus judge replay` and `audit` (M5 25d; design §2.9): the owner's
//! runs over the learning ledger. Each spends money, so the daemon judges
//! each as an answer (the owner, from a private place) and the client
//! refuses each inside a job (`client::refuse_in_a_job`).

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use theseus_client::{render, Conn};
use theseus_protocol::judge_runs::{
    JudgeAuditParams, JudgeAuditResult, JudgeReplayParams, JudgeReplayResult,
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
