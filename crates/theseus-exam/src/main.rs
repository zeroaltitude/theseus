//! `theseus-exam`: M6's memory exam and the headroom test (step 34a).
//!
//! ```text
//! theseus-exam list                                   the items, by family
//! theseus-exam write-store --store DIR --manifest F   every item's past, into a scratch store
//! theseus-exam note --store DIR --manifest F --item ID what the oracle arm sends for an item
//! theseus-exam run --base-config F --store DIR --manifest F --work DIR --out F --limit-usd N
//!                                                     items × arms × runs, one scratch daemon per arm
//! theseus-exam report --runs F [--out F]              the report over the arms, as Markdown
//! theseus-exam probe --tender S --manifest F [--arm A] the gold's recall, asked of a running index tender
//! ```
//!
//! The exam is exam-v2, built in; `--exam FILE` reads another file instead.
//! `run` starts and stops its own daemons (`arms.rs`): the `theseusd` beside
//! this binary, or `--theseusd`.
//! `probe` needs a tender: a scratch daemon over the store `write-store`
//! wrote starts one, and its socket is `<state dir>/index/sock`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use theseus_exam::arms::{self, ArmsPlan};
use theseus_exam::drive::{self, Arm};
use theseus_exam::fixture::{self, Manifest};
use theseus_exam::item::{Exam, Family};
use theseus_exam::{report, tender};

#[derive(Parser)]
#[command(
    name = "theseus-exam",
    about = "M6's memory exam and the headroom test (34a, theseus-zaz.11)"
)]
struct Cli {
    /// An exam file's path (default: the built-in exam).
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
        /// The exam's store, as `write-store` wrote it.
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        item: String,
    },
    /// Run items × arms × runs, one scratch daemon per arm of the real
    /// pipeline (each on its own copy of the store, in `[memory] mode =
    /// "live"`), appending a JSON line per cell to --out. It starts and
    /// stops every daemon itself.
    Run {
        /// The scratch config every daemon's is written from: its profiles,
        /// providers and secrets. `[memory]` mode and arm are set, and
        /// Discord, the web UI and the MCP server turned off.
        #[arg(long)]
        base_config: PathBuf,
        /// The exam's store, as `write-store` wrote it (never served: each
        /// daemon gets a copy).
        #[arg(long)]
        store: PathBuf,
        #[arg(long)]
        manifest: PathBuf,
        /// A directory of the run's own: each arm's snapshot, each run's
        /// daemons and their logs.
        #[arg(long)]
        work: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// Comma-separated: none, bm25, baseline, oracle.
        #[arg(long, default_value = "none,bm25,baseline,oracle")]
        arms: String,
        #[arg(long, default_value_t = 3)]
        runs: u32,
        /// Comma-separated item ids (default: every item of --half).
        #[arg(long, default_value = "")]
        items: String,
        /// `in` (the held-in half), `out` (the held-out half), or `all`.
        #[arg(long, default_value = "all")]
        half: String,
        /// Stop starting cells once the output file's spend, plus a reserve per
        /// cell in flight, would pass this.
        #[arg(long)]
        limit_usd: f64,
        #[arg(long, default_value_t = 4)]
        workers: usize,
        /// The profile every cell runs on: the base config's live one.
        #[arg(long, default_value = "glm")]
        profile: String,
        #[arg(long, default_value_t = 34)]
        seed: u64,
        /// A cell's time, start to end.
        #[arg(long, default_value_t = 300)]
        timeout_secs: u64,
        /// How long a tender may take to read (and embed) the store.
        #[arg(long, default_value_t = 1800)]
        settle_secs: u64,
        /// The daemon (default: the `theseusd` beside this binary, else on
        /// PATH).
        #[arg(long)]
        theseusd: Option<PathBuf>,
    },
    /// The report from a run's records: every arm's pass rate, the paired
    /// differences, cost per pass, the halves, and the decision per feature.
    Report {
        #[arg(long)]
        runs: PathBuf,
        /// Score the stored replies again with this exam's checks first.
        #[arg(long)]
        rescore: bool,
        /// Write the report, frozen, to this file (refused if it exists),
        /// instead of printing it.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// The exam's tasks asked of a running index tender (BM25 and entities,
    /// and vectors when its model is there): per family, how much of the
    /// gold ranks in the top k, per arm of sources and fusion weights. The
    /// tender is a scratch daemon's, over the store `write-store` wrote, on
    /// `<state dir>/index/sock`. No model runs in the exam.
    Probe {
        /// Also list each item's gold ranks.
        #[arg(long)]
        items: bool,
        /// The index tender's socket (`<state dir>/index/sock`).
        #[arg(long)]
        tender: PathBuf,
        /// The store's manifest, which maps the tender's node ids to the
        /// exam's keys.
        #[arg(long)]
        manifest: PathBuf,
        /// An arm, as sources with optional fusion weights
        /// (`bm25,entity,vector:2`). Repeat it for more; by default
        /// `bm25,entity`, `bm25,entity,vector`, and `vector`.
        #[arg(long = "arm")]
        arms: Vec<String>,
        /// `in` (the default: the held-in half, all tuning may
        /// see), `out` (the held-out half, which judges once), or `all`.
        #[arg(long)]
        half: Option<String>,
        /// Hits asked per query (at most 100).
        #[arg(long)]
        k: Option<usize>,
        /// Every item's gold ranks under every arm, as JSON.
        #[arg(long)]
        json: Option<PathBuf>,
        /// How long to wait for the tender to read and embed
        /// the store.
        #[arg(long, default_value_t = 900)]
        settle_secs: u64,
    },
}

/// The `theseusd` beside this binary, else `theseusd` on PATH.
fn beside_me() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| Some(e.parent()?.join("theseusd")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| PathBuf::from("theseusd"))
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
        Cmd::Note {
            store,
            manifest,
            item,
        } => {
            let m = Manifest::read(&manifest)?;
            let Some(i) = exam.item(&item) else {
                bail!("no item {item}");
            };
            let notes = drive::oracle_notes(&store, &m, &[i])?;
            println!("{}", drive::input_for(&notes, i, Arm::Oracle)?);
        }
        Cmd::Run {
            base_config,
            store,
            manifest,
            work,
            out,
            arms,
            runs,
            items,
            half,
            limit_usd,
            workers,
            profile,
            seed,
            timeout_secs,
            settle_secs,
            theseusd,
        } => {
            let m = Manifest::read(&manifest)?;
            let half = tender::Half::parse(&half)?;
            let mut ids: Vec<String> = items
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect();
            if ids.is_empty() {
                ids = exam
                    .file
                    .items
                    .iter()
                    .filter(|i| half.admits(i.held_out))
                    .map(|i| i.id.clone())
                    .collect();
            }
            let base = std::fs::read_to_string(&base_config)?;
            let plan = ArmsPlan {
                theseusd: theseusd.unwrap_or_else(beside_me),
                base_config: base.parse()?,
                store,
                work,
                arms: arms
                    .split(',')
                    .map(str::trim)
                    .map(Arm::parse)
                    .collect::<Result<_>>()?,
                runs,
                items: ids,
                limit_usd,
                workers,
                profile,
                seed,
                timeout: Duration::from_secs(timeout_secs),
                out,
                settle: Duration::from_secs(settle_secs),
                env: Vec::new(),
            };
            let s = arms::run(&plan, &exam, &m, &mut |line| eprintln!("{line}"))?;
            println!("{}", serde_json::to_string(&s)?);
        }
        Cmd::Report { runs, rescore, out } => {
            let mut records = drive::read_records(&runs)?;
            if rescore {
                report::rescore(&mut records, &exam);
            }
            let md = report::render(&records, &exam, &runs.display().to_string());
            match out {
                Some(path) => {
                    report::freeze(&path, &md)?;
                    println!("{}", path.display());
                }
                None => print!("{md}"),
            }
        }
        Cmd::Probe {
            items,
            tender: socket,
            manifest,
            arms,
            half,
            k,
            json,
            settle_secs,
        } => {
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
    }
    Ok(())
}
