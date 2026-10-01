//! `theseus-index`: run the index tender, or ask one. Until the wire-in
//! (roadmap row 51) gives `theseusd` a `tender index` role, this binary is
//! how the tender runs and how it is checked.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use theseus_index::client::Client;
use theseus_index::proto::{method, IndexStatus, QueryParams, QueryResult, RebuildResult};
use theseus_index::{Config, OpenError};

#[derive(Parser)]
#[command(
    name = "theseus-index",
    about = "The index tender (M6 step 29b), and a client for its socket"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Follow a store's WAL into an index, and answer on the index's socket.
    Serve {
        /// The store's directory (`<state>/store`).
        #[arg(long)]
        store: PathBuf,
        /// The index's directory (`<state>/index`).
        #[arg(long)]
        index: PathBuf,
        /// Run at this nice level (the design's 10).
        #[arg(long, default_value_t = 10)]
        nice: i32,
        /// The longest a caught-up tender sleeps without an event.
        #[arg(long, default_value_t = 60)]
        backstop_secs: u64,
    },
    /// BM25 and entity hits for a text.
    Query {
        #[arg(long)]
        socket: PathBuf,
        #[arg(short, long, default_value_t = 10)]
        k: usize,
        /// Only nodes written before this position.
        #[arg(long)]
        as_of: Option<u64>,
        /// Sources to fuse: bm25, entity (default both).
        #[arg(long, value_delimiter = ',')]
        sources: Vec<String>,
        /// The whole answer, as JSON.
        #[arg(long)]
        json: bool,
        text: Vec<String>,
    },
    /// The tender's status (health's `index` block), as JSON.
    Status {
        #[arg(long)]
        socket: PathBuf,
    },
    /// Drop the index and build it again from the WAL.
    Rebuild {
        #[arg(long)]
        socket: PathBuf,
    },
}

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("theseus-index: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: Cmd) -> anyhow::Result<ExitCode> {
    match cmd {
        Cmd::Serve {
            store,
            index,
            nice,
            backstop_secs,
        } => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info".into()),
                )
                .init();
            // SAFETY: no pointers; it sets this process's own priority.
            if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) } != 0 {
                tracing::warn!(nice, "index: could not lower its priority");
            }
            let mut cfg = Config::new(&store, &index);
            cfg.backstop = Duration::from_secs(backstop_secs.max(1));
            match theseus_index::serve(cfg) {
                Ok(()) => Ok(ExitCode::SUCCESS),
                Err(OpenError::Held(dir)) => {
                    eprintln!("theseus-index: another tender holds {}", dir.display());
                    Ok(ExitCode::from(3))
                }
                Err(OpenError::Other(e)) => Err(e),
            }
        }
        Cmd::Query {
            socket,
            k,
            as_of,
            sources,
            json,
            text,
        } => {
            let mut p = QueryParams::new(&text.join(" "));
            p.k = k;
            p.as_of = as_of;
            p.sources = sources;
            let r: QueryResult = Client::connect(&socket, CALL_TIMEOUT)?.call(method::QUERY, &p)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(ExitCode::SUCCESS);
            }
            println!(
                "{} hits; indexed through position {}, {} bytes behind; {:.1} ms (bm25 {:.1}, entity {:.1})",
                r.hits.len(),
                r.indexed_through,
                r.lag.bytes,
                r.timings.total_ms,
                r.timings.bm25_ms,
                r.timings.entity_ms
            );
            for (i, h) in r.hits.iter().enumerate() {
                let sources: Vec<String> = h
                    .sources
                    .iter()
                    .map(|(s, r)| format!("{s} #{} {:.2}", r.rank, r.score))
                    .collect();
                let preview: String = h
                    .text
                    .chars()
                    .take(100)
                    .collect::<String>()
                    .replace('\n', " ");
                println!(
                    "{:>2}. {}#{} @{} {} {} [{}] {:.4}{}\n    {}",
                    i + 1,
                    h.node_id,
                    h.chunk,
                    h.position,
                    h.kind,
                    h.session_id,
                    sources.join(", "),
                    h.fused,
                    if h.entities_matched.is_empty() {
                        String::new()
                    } else {
                        format!(" entities {}", h.entities_matched.join(" "))
                    },
                    preview
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Status { socket } => {
            let s: IndexStatus =
                Client::connect(&socket, CALL_TIMEOUT)?.call(method::STATUS, ())?;
            println!("{}", serde_json::to_string_pretty(&s)?);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Rebuild { socket } => {
            let r: RebuildResult =
                Client::connect(&socket, CALL_TIMEOUT)?.call(method::REBUILD, ())?;
            println!("rebuild accepted: {}", r.accepted);
            Ok(ExitCode::SUCCESS)
        }
    }
}
