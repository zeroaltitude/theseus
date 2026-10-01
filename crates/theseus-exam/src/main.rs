//! `theseus-exam`: M6's memory exam and the headroom test (step 34a).
//!
//! ```text
//! theseus-exam list                                   the items, by family
//! theseus-exam write-store --store DIR --manifest F   every item's past, into a scratch store
//! theseus-exam note --manifest F --item ID            what the oracle arm sends for an item
//! theseus-exam run --socket S --manifest F --out F    items × arms × runs, through a scratch daemon
//! theseus-exam report --runs F                        the headroom report, as Markdown
//! ```

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use theseus_exam::drive::{self, Arm, Plan};
use theseus_exam::fixture::{self, Manifest};
use theseus_exam::item::{Exam, Family};
use theseus_exam::report;

#[derive(Parser)]
#[command(
    name = "theseus-exam",
    about = "M6's memory exam and the headroom test (34a)"
)]
struct Cli {
    /// The exam file (default: the committed exam-v1, built in).
    #[arg(long, global = true)]
    exam: Option<PathBuf>,
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
}

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
    }
    Ok(())
}
