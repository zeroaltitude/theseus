//! `theseus-index`: run the index tender, or ask one. Installed beside
//! `theseusd`, which runs it after serving as `serve --parent <its pid>`
//! (roadmap row 51), restarts it when it exits, and forwards `index.status`
//! and `index.query` to its socket for every other client. The other
//! subcommands ask a tender's socket directly, for checks by hand.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use theseus_index::client::Client;
use theseus_index::proto::{
    method, EmbedParams, EmbedResult, ForgetParams, ForgetResult, IndexStatus, NeighboursParams,
    NeighboursResult, QueryParams, QueryResult, RebuildResult, Task, WarmResult,
};
use theseus_index::vectors::VectorConfig;
use theseus_index::{Config, OpenError};

#[derive(Parser)]
#[command(
    name = "theseus-index",
    about = "The index tender (M6 steps 29b and 29c), and a client for its socket"
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
        /// Where the embedding models live (`[index] weights_dir`; default
        /// `~/.cache/theseus/models`). Nothing is fetched: without the
        /// weights, BM25 and entities answer alone.
        #[arg(long)]
        weights_dir: Option<PathBuf>,
        /// No vectors at all.
        #[arg(long)]
        no_vectors: bool,
        /// Unload the model after this many minutes unused
        /// (`[index] idle_unload_mins`).
        #[arg(long, default_value_t = 10.0)]
        idle_unload_mins: f64,
        /// candle's threads (`[index] threads`): `RAYON_NUM_THREADS` and
        /// `CANDLE_NUM_THREADS`, set for this process before anything starts.
        #[arg(long, default_value_t = 1)]
        threads: usize,
        /// The fusion's default weights, over the built-in ones
        /// (`bm25=1,entity=1,vector=2`; `[index] fusion` at the wire-in).
        #[arg(long)]
        weights: Option<String>,
        /// Exit when this process does: the daemon that started this tender
        /// passes its own pid (roadmap row 51), so no tender outlives it.
        #[arg(long)]
        parent: Option<u32>,
    },
    /// Hits for a text.
    Query {
        #[arg(long)]
        socket: PathBuf,
        #[arg(short, long, default_value_t = 10)]
        k: usize,
        /// Only nodes written before this position.
        #[arg(long)]
        as_of: Option<u64>,
        /// Sources to fuse: bm25, entity, vector (default: all the tender has).
        #[arg(long, value_delimiter = ',')]
        sources: Vec<String>,
        /// Wait this long for the model to load, rather than answer without
        /// vectors.
        #[arg(long, default_value_t = 0)]
        wait_ms: u64,
        /// Fusion weights for this query (`vector=2`); the tender's own for
        /// any source not named.
        #[arg(long)]
        weights: Option<String>,
        /// The whole answer, as JSON.
        #[arg(long)]
        json: bool,
        text: Vec<String>,
    },
    /// The nodes nearest a node, by the 768-d vector.
    Neighbours {
        #[arg(long)]
        socket: PathBuf,
        #[arg(short, long, default_value_t = 10)]
        k: usize,
        node_id: String,
    },
    /// Vectors for texts, as JSON.
    Embed {
        #[arg(long)]
        socket: PathBuf,
        /// search_document (default), search_query, clustering, classification.
        #[arg(long, default_value = "search_document")]
        task: String,
        /// 768 or 256.
        #[arg(long)]
        dims: Option<usize>,
        texts: Vec<String>,
    },
    /// Start loading the model, and answer at once.
    Warm {
        #[arg(long)]
        socket: PathBuf,
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
    /// Nodes, or the chunks that hold texts, leave the index now, and their
    /// vectors leave every vector file.
    Forget {
        #[arg(long)]
        socket: PathBuf,
        /// A node that leaves whole (repeat for more).
        #[arg(long = "node")]
        nodes: Vec<String>,
        /// A chunk text, exactly as a hit gives it (repeat for more).
        #[arg(long = "text")]
        texts: Vec<String>,
    },
}

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// The idle I/O class (roadmap row 51): the tender's commits, about 140 ms of
/// fsyncs each on this disk, wait for the disk to be otherwise idle, so the
/// core's WAL syncs never queue behind them, where the I/O scheduler honours
/// classes. Set before any thread starts, and every thread inherits it.
fn io_idle() {
    const IOPRIO_WHO_PROCESS: libc::c_int = 1;
    const IOPRIO_CLASS_IDLE: libc::c_int = 3;
    const IOPRIO_CLASS_SHIFT: libc::c_int = 13;
    // SAFETY: plain integers; it sets this thread's own I/O priority.
    let set = unsafe {
        libc::syscall(
            libc::SYS_ioprio_set,
            IOPRIO_WHO_PROCESS,
            0,
            IOPRIO_CLASS_IDLE << IOPRIO_CLASS_SHIFT,
        )
    };
    if set != 0 {
        tracing::warn!(
            error = %std::io::Error::last_os_error(),
            "index: could not take the idle I/O class"
        );
    }
}

/// Exit when `parent` exits (roadmap row 51): the daemon that started this
/// tender. A restart onto a changed config note execs the daemon in place,
/// with the same pid, and this tender lives on for the new image to take
/// over; a crash or a kill of the daemon ends it here, so no tender outlives
/// its daemon holding the index's lock. A pidfd, so nothing polls: a thread
/// waits on it and exits the process, which is what SIGTERM does (ingest is
/// idempotent, and the cursor follows each commit).
fn exit_with(parent: u32) {
    // SAFETY: plain integers; it returns a new fd, or -1.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, parent as libc::pid_t, 0) };
    if fd < 0 {
        tracing::warn!(
            parent,
            error = %std::io::Error::last_os_error(),
            "index: cannot watch the daemon that started this tender; it will not exit with it"
        );
        return;
    }
    let fd = fd as libc::c_int;
    // The daemon may have gone before the pidfd was taken, and this tender
    // was reparented: then that pid is no longer its parent.
    if std::os::unix::process::parent_id() != parent {
        tracing::info!(parent, "index: the daemon that started this tender is gone");
        std::process::exit(0);
    }
    let watch = std::thread::Builder::new()
        .name("index-parent".into())
        .spawn(move || {
            let mut p = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one pollfd, owned by this frame; no timeout.
            while unsafe { libc::poll(&mut p, 1, -1) } < 0
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
            {}
            tracing::info!(parent, "index: the daemon that started this tender exited");
            std::process::exit(0);
        });
    if let Err(e) = watch {
        tracing::warn!(parent, error = %e, "index: cannot watch the daemon that started this tender");
    }
}

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

#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
fn run(cmd: Cmd) -> anyhow::Result<ExitCode> {
    match cmd {
        Cmd::Serve {
            store,
            index,
            nice,
            backstop_secs,
            weights_dir,
            no_vectors,
            idle_unload_mins,
            threads,
            weights,
            parent,
        } => {
            // Before any thread starts: candle reads these at every matmul,
            // and rayon's pool at its first use. Every core when unset.
            let threads = threads.max(1).to_string();
            std::env::set_var("RAYON_NUM_THREADS", &threads);
            std::env::set_var("CANDLE_NUM_THREADS", &threads);
            // tantivy logs every commit at info (five lines each): quiet
            // unless asked, and no colour codes in a log file.
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "info,tantivy=warn".into()),
                )
                .init();
            // SAFETY: no pointers; it sets this process's own priority.
            if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) } != 0 {
                tracing::warn!(nice, "index: could not lower its priority");
            }
            io_idle();
            if let Some(parent) = parent {
                exit_with(parent);
            }
            let mut cfg = Config::new(&store, &index);
            cfg.backstop = Duration::from_secs(backstop_secs.max(1));
            if let Some(w) = weights {
                cfg.weights = cfg
                    .weights
                    .parse_over(&w)
                    .map_err(|e| anyhow::anyhow!("--weights: {e}"))?;
            }
            if !no_vectors {
                let dir = weights_dir.or_else(|| {
                    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache/theseus/models"))
                });
                cfg.vectors = VectorConfig::new(dir);
                cfg.vectors.idle_unload = Duration::from_secs_f64(idle_unload_mins.max(0.0) * 60.0);
            }
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
            wait_ms,
            weights,
            json,
            text,
        } => {
            let mut p = QueryParams::new(&text.join(" "));
            p.k = k;
            p.as_of = as_of;
            p.sources = sources;
            p.wait_ms = wait_ms;
            if let Some(w) = weights {
                for part in w.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let (s, x) = part
                        .split_once('=')
                        .ok_or_else(|| anyhow::anyhow!("--weights {part:?}: source=weight"))?;
                    p.weights.insert(s.trim().to_string(), x.trim().parse()?);
                }
            }
            let timeout = CALL_TIMEOUT + Duration::from_millis(wait_ms);
            let r: QueryResult = Client::connect(&socket, timeout)?.call(method::QUERY, &p)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(ExitCode::SUCCESS);
            }
            let t = &r.timings;
            println!(
                "{} hits; indexed through position {}, {} bytes behind; {:.1} ms (bm25 {:.1}, entity {:.1}, embed {:.1}, vector {:.1})",
                r.hits.len(),
                r.indexed_through,
                r.lag.bytes,
                t.total_ms,
                t.bm25_ms,
                t.entity_ms,
                t.embed_ms,
                t.vector_ms
            );
            for (source, why) in &r.skipped {
                println!("    ({source} skipped: {why})");
            }
            for (i, h) in r.hits.iter().enumerate() {
                let sources: Vec<String> = h
                    .sources
                    .iter()
                    .map(|(s, r)| format!("{s} #{} {:.3}", r.rank, r.score))
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
        Cmd::Neighbours { socket, k, node_id } => {
            let r: NeighboursResult = Client::connect(&socket, CALL_TIMEOUT)?.call(
                method::NEIGHBOURS,
                NeighboursParams {
                    node_id,
                    k,
                    as_of: None,
                },
            )?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Embed {
            socket,
            task,
            dims,
            texts,
        } => {
            let task: Task = serde_json::from_value(serde_json::Value::String(task))?;
            let p = EmbedParams {
                texts,
                task,
                dims,
                wait_ms: 60_000,
            };
            let r: EmbedResult =
                Client::connect(&socket, Duration::from_secs(120))?.call(method::EMBED, &p)?;
            println!("{}", serde_json::to_string(&r)?);
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Warm { socket } => {
            let r: WarmResult = Client::connect(&socket, CALL_TIMEOUT)?.call(method::WARM, ())?;
            println!("model {}, mode {}", r.model, r.mode);
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
        Cmd::Forget {
            socket,
            nodes,
            texts,
        } => {
            let r: ForgetResult = Client::connect(&socket, Duration::from_secs(70))?
                .call(method::FORGET, ForgetParams { nodes, texts })?;
            println!("{}", serde_json::to_string_pretty(&r)?);
            Ok(ExitCode::SUCCESS)
        }
    }
}
