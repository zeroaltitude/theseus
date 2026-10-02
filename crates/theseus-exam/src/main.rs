//! `theseus-exam`: M6's memory exam and the headroom test (step 34a).
//!
//! ```text
//! theseus-exam list                                   the items, by family
//! theseus-exam write-store --store DIR --manifest F   every item's past, into a scratch store
//! theseus-exam note --manifest F --item ID            what the oracle arm sends for an item
//! theseus-exam run --socket S --manifest F --out F    items × arms × runs, through a scratch daemon
//! theseus-exam report --runs F                        the headroom report, as Markdown
//! theseus-exam probe [--against v1]                   BM25's recall of the gold, per family (no model)
//! theseus-exam probe --tender S --manifest F [--arm A] the same, asked of a running index tender
//! ```
//!
//! `--exam` picks the exam: `v2` (the default, built in), `v1` (built in), or
//! a file.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use theseus_exam::drive::{self, Arm, Plan};
use theseus_exam::fixture::{self, Manifest};
use theseus_exam::item::{Exam, Family};
use theseus_exam::probe::{self, Rank, Tokenizer};
use theseus_exam::{report, tender};

#[derive(Parser)]
#[command(
    name = "theseus-exam",
    about = "M6's memory exam and the headroom test (34a), and exam-v2 (theseus-zaz.11)"
)]
struct Cli {
    /// The exam: `v2` (default) or `v1`, built in, or an exam file's path.
    #[arg(long, global = true)]
    exam: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// The items, by family, with their halves.
    List,
    /// Write every item's past into the store at --store (a scratch store's
    /// directory, e.g. <state dir>/store), and its manifest.
    WriteStore {
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
    },
    /// Print what the oracle arm sends for an item.
    Note {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        item: String,
    },
    /// Run items × arms × runs through a scratch daemon's socket, appending a
    /// JSON line per cell to --out.
    Run {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Comma-separated: none, oracle.
        #[arg(long, default_value = "none,oracle")]
        arms: String,
        #[arg(long, default_value_t = 3)]
        runs: u32,
        /// Comma-separated item ids (default: every item).
        #[arg(long, default_value = "")]
        items: String,
        /// Stop starting cells once the output file's spend, plus a reserve per
        /// cell in flight, would pass this.
        #[arg(long)]
        limit_usd: f64,
        #[arg(long, default_value_t = 4)]
        workers: usize,
        #[arg(long, default_value = "glm")]
        profile: String,
        #[arg(long, default_value_t = 34)]
        seed: u64,
        /// A cell's time, start to end.
        #[arg(long, default_value_t = 300)]
        timeout_secs: u64,
    },
    /// The headroom report from a run's records.
    Report {
        #[arg(long)]
        runs: PathBuf,
        /// Score the stored replies again with this exam's checks first.
        #[arg(long)]
        rescore: bool,
    },
    /// BM25 over every past node of the exam's store, queried with each
    /// task: per family, how much of the gold ranks in the top k. No store,
    /// no daemon, no model. With --tender, the tasks are asked of a running
    /// index tender instead, per arm of sources and fusion weights.
    Probe {
        /// `v1` (the 34a script's) or `simple` (split on non-alphanumerics).
        #[arg(long, default_value = "v1")]
        tokenizer: String,
        /// Weight each score by 2^(−age / this many days).
        #[arg(long)]
        half_life_days: Option<f64>,
        /// Also list each item's gold ranks.
        #[arg(long)]
        items: bool,
        /// Compare with this exam (`v1`, `v2`, or a file), base first.
        #[arg(long)]
        against: Option<String>,
        /// Ask the index tender on this socket (`theseus-index serve` over
        /// the store `write-store` wrote), rather than BM25 in process.
        #[arg(long)]
        tender: Option<PathBuf>,
        /// With --tender: the store's manifest, which maps the tender's node
        /// ids to the exam's keys.
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// With --tender: an arm, as sources with optional fusion weights
        /// (`bm25,entity,vector:2`). Repeat it for more; by default
        /// `bm25,entity`, `bm25,entity,vector`, and `vector`.
        #[arg(long = "arm")]
        arms: Vec<String>,
        /// With --tender: `in` (the default: the held-in half, all tuning may
        /// see), `out` (the held-out half, which judges once), or `all`.
        #[arg(long)]
        half: Option<String>,
        /// With --tender: hits asked per query (at most 100).
        #[arg(long)]
        k: Option<usize>,
        /// With --tender: every item's gold ranks under every arm, as JSON.
        #[arg(long)]
        json: Option<PathBuf>,
        /// With --tender: how long to wait for the tender to read and embed
        /// the store.
        #[arg(long, default_value_t = 900)]
        settle_secs: u64,
    },
}

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn main() -> Result<()> {
    let cli = Cli::parse();
    let exam = Exam::load(cli.exam.as_deref())?;
    match cli.cmd {
        Cmd::List => {
            println!(
                "{} ({}), {} items",
                exam.file.version,
                exam.digest,
                exam.file.items.len()
            );
            for f in Family::ALL {
                let of: Vec<String> = exam
                    .file
                    .items
                    .iter()
                    .filter(|i| i.family == f)
                    .map(|i| format!("{}{}", i.id, if i.held_out { "*" } else { "" }))
                    .collect();
                println!("  {:<14} {}", f.as_str(), of.join(" "));
            }
            println!("  (* held out)");
        }
        Cmd::WriteStore { store, manifest } => {
            if store.join("wal").exists() {
                bail!(
                    "{} already holds a store: write into an empty directory",
                    store.display()
                );
            }
            let m = fixture::write(&exam, &store)?;
            m.write(&manifest)?;
            println!(
                "{} sessions, {} keyed nodes, last position {} -> {}; manifest {}",
                m.sessions.len(),
                m.nodes.len(),
                m.last_position,
                store.display(),
                manifest.display()
            );
        }
        Cmd::Note { manifest, item } => {
            let m = Manifest::read(&manifest)?;
            let Some(i) = exam.item(&item) else {
                bail!("no item {item}");
            };
            println!("{}", drive::input_for(&m, i, Arm::Oracle)?);
        }
        Cmd::Run {
            socket,
            manifest,
            out,
            arms,
            runs,
            items,
            limit_usd,
            workers,
            profile,
            seed,
            timeout_secs,
        } => {
            let m = Manifest::read(&manifest)?;
            let plan = Plan {
                socket,
                profile,
                arms: arms
                    .split(',')
                    .map(str::trim)
                    .map(Arm::parse)
                    .collect::<Result<_>>()?,
                runs,
                items: items
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect(),
                limit_usd,
                workers,
                seed,
                timeout: Duration::from_secs(timeout_secs),
                out,
            };
            println!("preflight: {}", drive::preflight(&plan, &exam, &m)?);
            let s = drive::run(&plan, &exam, &m)?;
            println!("{}", serde_json::to_string(&s)?);
        }
        Cmd::Report { runs, rescore } => {
            let mut records = drive::read_records(&runs)?;
            if rescore {
                report::rescore(&mut records, &exam);
            }
            print!(
                "{}",
                report::render(&records, &exam, &runs.display().to_string())
            );
        }
        Cmd::Probe {
            half_life_days,
            items,
            against,
            tender: Some(socket),
            manifest,
            arms,
            half,
            k,
            json,
            settle_secs,
            ..
        } => {
            if half_life_days.is_some() || against.is_some() {
                bail!("--half-life-days and --against are the in-process probe's, not --tender's");
            }
            let Some(manifest) = manifest else {
                bail!("--tender needs --manifest: the store's, from write-store");
            };
            let m = Manifest::read(&manifest)?;
            let arms: Vec<String> = if arms.is_empty() {
                tender::DEFAULT_ARMS.map(String::from).to_vec()
            } else {
                arms
            };
            let plan = tender::Plan {
                socket,
                arms: arms
                    .iter()
                    .map(|a| tender::Arm::parse(a))
                    .collect::<Result<_>>()?,
                k: k.unwrap_or(100).clamp(1, 100),
                half: tender::Half::parse(half.as_deref().unwrap_or("in"))?,
                wait_ms: 60_000,
                settle: Duration::from_secs(settle_secs),
            };
            let run = tender::run(&exam, &m, &plan, &mut |line| eprintln!("{line}"))?;
            if let Some(path) = json {
                std::fs::write(
                    &path,
                    serde_json::to_vec_pretty(&tender::to_json(&exam, &m, &run))?,
                )?;
            }
            print!("{}", tender::render(&exam, &run, items));
        }
        Cmd::Probe {
            tokenizer,
            half_life_days,
            items,
            against,
            tender: None,
            manifest,
            arms,
            half,
            k,
            json,
            ..
        } => {
            if manifest.is_some()
                || !arms.is_empty()
                || half.is_some()
                || k.is_some()
                || json.is_some()
            {
                bail!("--manifest, --arm, --half, --k, and --json go with --tender");
            }
            let tok = Tokenizer::parse(&tokenizer)?;
            let rank = match half_life_days {
                Some(h) if h > 0.0 => Rank::Recency { half_life_days: h },
                Some(h) => bail!("a half-life of {h} days"),
                None => Rank::Bm25,
            };
            let (c, ps) = probe::probe(&exam, tok, rank)?;
            if let Some(spec) = against {
                let base = Exam::load(Some(&spec))?;
                let (_, bs) = probe::probe(&base, tok, rank)?;
                println!(
                    "{}",
                    probe::compare((&base.file.version, &bs), (&exam.file.version, &ps))
                );
            }
            print!("{}", probe::render(&exam, &c, &ps, tok, rank, items));
        }
    }
    Ok(())
}
