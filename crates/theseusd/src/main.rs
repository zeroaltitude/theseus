//! `theseusd`: the Theseus server.
//!
//! Startup order (serve first, theseus-qa0): read the service-account token,
//! load config, start resolving the secrets in the background, open the
//! store, run the kernel's startup, then serve the protocol on stdio
//! (`--stdio`) or a Unix socket (default). Nothing on that path waits for a
//! secret: each consumer waits for its own, and one that fails to resolve
//! never runs without it.
//!
//! The config is a file, or a note in the vault. A start whose config is in
//! the vault serves from the note's last-known-good copy and acts on it, when
//! the copy's digest is the one the daemon recorded in the store as it wrote
//! it (theseus-zmgb), and reads the vault once behind the socket, beside the
//! secrets (theseus-2fo); a changed note restarts the daemon onto the vault's
//! version. A start with no copy, or one edited since, reads the vault first,
//! and keeps the copy after serving.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use theseus_core::approval::{Client, Surface};
use theseus_core::config::{DEFAULT_CONFIG, NO_CONFIG};
use theseus_core::config_copy::{self, Compared};
use theseus_core::config_gate::{self, ConfigGate};
use theseus_core::secrets::{OpReader, Secret, SecretBoard, Waited};
use theseus_core::startup::{stop_phase, StartupLog};
use theseus_core::store::Store;
use theseus_core::{Config, Core};
use tokio::net::UnixListener;

mod install;
mod web;

const AFTER_HELP: &str = "\
Running it:
  With no COMMAND, theseusd serves: it loads config, opens the store, and listens on the Unix
  socket and the web UI (http://127.0.0.1:7433/) while the secrets resolve from 1Password in
  the background. `theseus health` says whether they are resolving, ready, or failed; a turn,
  the Discord binding, and the GitHub check each wait for their own, and never run without it.
  It stays in the foreground; Ctrl-C stops it.

  export OP_SERVICE_ACCOUNT_TOKEN=...   the only secret allowed outside 1Password
  theseusd check                        prove the vault wiring, then exit
  theseusd config                       show the config actually loaded, and from where
  theseusd restore --from <wal dir>     rebuild the store from a WAL copy (daemon stopped)
  theseusd restore --from s3://<bucket>/durability/<deployment>/   ...or from what it shipped
  theseusd example-bindings             the Discord bindings file format
  theseusd                              serve (foreground); add & to background it
  THESEUS_LOG=debug theseusd            more detail (tracing filter syntax)
  theseusd --socket /tmp/dbg.sock --state-dir /tmp/dbg   a scratch instance beside a running one

Config source (--config / THESEUS_CONFIG): a local TOML file (default: ~/.theseus/theseus.toml),
or the op:// reference of a 1Password note that holds it. `theseusd example-config` prints a
template; it is not what the server runs with.";

#[derive(Parser, Debug)]
#[command(
    name = "theseusd",
    version,
    about = "Theseus server: config and secrets from 1Password, the turn kernel, the protocol on a Unix socket or stdio, and the localhost web UI.",
    after_help = AFTER_HELP
)]
struct Cli {
    /// Config source: a file path, or an op:// reference.
    #[arg(long, env = "THESEUS_CONFIG", default_value = DEFAULT_CONFIG)]
    config: String,

    /// File holding the 1Password service-account token (used when OP_SERVICE_ACCOUNT_TOKEN is unset).
    // Global, so `theseusd install --user --op-token-file F` works as well as
    // `theseusd --op-token-file F install --user` (theseus-w1nf): the install plan's hint
    // names the first.
    #[arg(long, env = "THESEUS_OP_TOKEN_FILE", global = true)]
    op_token_file: Option<String>,

    /// Speak the protocol on stdin/stdout instead of a socket (spawned by a client).
    #[arg(long)]
    stdio: bool,

    /// Unix socket path; overrides [server].socket in config.
    #[arg(long, env = "THESEUS_SOCKET")]
    socket: Option<PathBuf>,

    /// State directory (store, spool); overrides [server].state_dir. Use with --socket
    /// to run a scratch instance beside a live daemon (the store is single-process).
    #[arg(long, env = "THESEUS_STATE_DIR")]
    state_dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Print the annotated config template (every parameter, set or commented with its default) and exit.
    ExampleConfig,
    /// Load config, resolve every secret, report, and exit without serving.
    Check,
    /// Print the loaded config (TOML, secret references only, never values) and its source.
    Config {
        /// Print the config as a note of only what differs from the defaults: the secrets'
        /// references, every value that differs, and nothing else. It loads to the same config
        /// (theseus-vwar).
        #[arg(long)]
        sparse: bool,
    },
    /// Print an annotated bindings file (the Discord guild, channel, and who may drive it) and exit.
    ExampleBindings,
    /// Rebuild the store from a local WAL directory (or another store's directory) and exit.
    /// The source is only read; a store already in place is moved aside with --force, never deleted.
    Restore {
        /// A WAL directory (holding *.seg files), a store directory (holding wal/), or
        /// s3://<bucket>/durability/<deployment>/: what the durability tender shipped there.
        #[arg(long)]
        from: PathBuf,
        /// Move the existing store aside instead of refusing.
        #[arg(long)]
        force: bool,
        /// Repair the store in place instead: take each frame of its WAL that does not check
        /// whole from the copy at --from, keep every other byte, and keep the store it repairs
        /// aside (theseus-15g).
        #[arg(long, conflicts_with = "force")]
        repair: bool,
    },
    /// Install the daemon as a systemd service: --user (yours), or --separate (as its own
    /// user; needs root). Prints a plan; --apply performs it, --check compares, --remove undoes.
    Install(install::InstallArgs),
    /// Internal: the detached job wrapper (spawned by the kernel, never by hand).
    #[command(hide = true, disable_help_flag = true)]
    JobWrapper {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Internal: a hand, the job wrapper inside AWS (AWS design §3.3): run in the hand image, as
    /// Lambda's bootstrap or a Fargate task's command, never by hand.
    #[command(hide = true)]
    Hand,
}

/// In the environment of a start that must read the vault before serving,
/// whatever copy it finds (theseus-2fo): why, in words.
const VAULT_FIRST_ENV: &str = "THESEUS_CONFIG_VAULT_FIRST";

/// How a serving daemon ends.
enum Exit {
    Done,
    /// Become the same binary image with the same arguments, and this
    /// variable set: a restart onto the vault's changed note, or a start
    /// that must read the vault first.
    Exec(&'static str, String),
}

fn main() -> Result<()> {
    // An L1 job's init (17b): pid 1 of the job, started by its wrapper with
    // its spec on fd 3. First, before anything else: it stays one thread,
    // its stderr is the job's, and it keeps the umask its wrapper gave it.
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == theseus_sandbox::INIT_ROLE)
    {
        theseus_sandbox::init_main();
    }
    // The start of every startup phase's clock (theseus-qa0).
    let origin = Instant::now();
    // The commit this binary was built from, a constant (theseus-9o5n):
    // health and `server.started` name it.
    theseus_core::set_commit(env!("THESEUS_COMMIT"));
    // Before anything is created (theseus-wz2): the store, the spool and raw
    // job output, the config copy, and the socket are the operator's alone.
    // The operator's own umask is kept for a job's command and a tool's new
    // files in the workspace.
    theseus_kernel::umask::private();
    let cli = Cli::parse();
    // Logs go to stderr always; stdout may be the protocol stream.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("THESEUS_LOG")
                .unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    if let Some(Cmd::ExampleConfig) = cli.cmd {
        return out(Config::EXAMPLE_TOML);
    }
    if let Some(Cmd::ExampleBindings) = cli.cmd {
        return out(theseus_discord::EXAMPLE_BINDINGS);
    }
    if let Some(Cmd::JobWrapper { args }) = cli.cmd {
        // No config, no `op`, no runtime: the wrapper runs a command,
        // spools its result, and waits for what the command left running.
        // A broker's grant is in its environment, the job's, and a thread
        // copies the output to the spool with each granted value withheld
        // (theseus-l0d).
        let wa = theseus_kernel::job::parse_wrapper_args(args)?;
        return theseus_kernel::job::run_wrapper_process(&wa);
    }
    if let Some(Cmd::Hand) = cli.cmd {
        // No config, no store, no secrets: its spec is its whole input.
        std::process::exit(theseus_core::aws::hands::hand::main());
    }
    if let Some(Cmd::Install(args)) = &cli.cmd {
        // No config, no secrets, no runtime: never on the start path.
        let globals = install::Globals {
            config: cli.config.clone(),
            op_token_file: cli.op_token_file.clone(),
            state_dir: cli.state_dir.clone(),
            socket: cli.socket.clone(),
        };
        std::process::exit(install::run(args, &globals)?);
    }
    keep_name();
    if cli.cmd.is_none() {
        adopt_children();
    }
    let stdio = cli.stdio;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match rt.block_on(daemon(cli, origin))? {
        Exit::Done => {
            // The runtime's tasks go, and with them the store, which closes
            // (redb's close, logged by the index): each timed (theseus-26r).
            if stdio {
                // A `--stdio` daemon reads stdin on a blocking thread, which
                // only the client's end of the pipe ends: a stop by a signal
                // while the client holds it open would wait on it for ever
                // (theseus-p7q). The tasks go, and the store closes, within
                // this bound; the read is left behind, and the process ends.
                rt.shutdown_timeout(Duration::from_millis(500));
            } else {
                drop(rt);
            }
            stop_phase("runtime dropped");
            Ok(())
        }
        Exit::Exec(var, value) => {
            // The clean shutdown path has run. The runtime's tasks go, and
            // with them the store, which closes; then the same image.
            rt.shutdown_timeout(Duration::from_millis(500));
            stop_phase("runtime shut down");
            exec_self(var, &value)
        }
    }
}

/// How this start got its config.
enum Start {
    /// A file's text.
    File(String),
    /// The vault's note, read before serving: kept as the copy once serving.
    Vault(String),
    /// The last-known-good copy's text. The vault is read behind the
    /// socket: this read began at `began`, beside the secrets' `op inject`.
    Copy {
        text: String,
        first: tokio::task::JoinHandle<Result<String, String>>,
        began: Instant,
    },
}

#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
#[expect(clippy::too_many_lines, reason = "shape budget: split it")]
async fn daemon(cli: Cli, origin: Instant) -> Result<Exit> {
    if cli.cmd.is_none() {
        // The socket daemon and `--stdio` both spawn job wrappers.
        tokio::spawn(reap_children());
    }
    let op = Arc::new(match OpReader::from_env(cli.op_token_file.as_deref()) {
        Ok(op) => op,
        // No vault (a container, CI, or anyone without 1Password;
        // theseus-n88g.1): a file config whose secrets are `env:` or `file:`
        // entries needs none, and any `op://` entry fails with this reason,
        // one consumer at a time, as `theseusd check` and health say.
        Err(e) if !cli.config.starts_with("op://") => {
            tracing::info!(why = %format!("{e:#}"), "no 1Password access: only env: and file: secrets resolve");
            OpReader::absent(format!("{e:#}"))
        }
        Err(e) => {
            return Err(
                e.context("refusing to start without 1Password access: the config is in the vault")
            )
        }
    });
    let startup = Arc::new(StartupLog::new(origin));
    let in_vault = cli.config.starts_with("op://");
    // The copy is found before any config is read: in `--state-dir`, else ~/.theseus.
    let copy_path = in_vault.then(|| config_copy::path(cli.state_dir.as_deref()));
    let t = Instant::now();
    // A start that serves (the socket or `--stdio`) serves from the copy,
    // once the store says it is the daemon's (below); `check`, `config`, and
    // `restore` read the vault itself, as before.
    let vault_first = std::env::var(VAULT_FIRST_ENV).ok();
    let mut why_vault = vault_first.clone();
    let copy = match (&copy_path, &cli.cmd, &vault_first) {
        (Some(p), None, None) => match config_copy::read(p, &cli.config) {
            Ok(c) => c,
            Err(why) => {
                tracing::warn!(copy = %p.display(), why = %why, "the config copy cannot serve this start; reading the vault first");
                why_vault = Some(format!("the copy was not used: {why}"));
                None
            }
        },
        _ => None,
    };
    let (mut cfg, start) = match copy {
        Some(c) => {
            for w in &c.warnings {
                tracing::warn!("{w}");
            }
            // The vault's word on the copy, read now, beside the secrets.
            let (reader, reference) = (op.clone(), cli.config.clone());
            let began = Instant::now();
            let first = tokio::spawn(async move {
                config_gate::ReadNote::read_note(reader.as_ref(), &reference).await
            });
            (
                c.config,
                Start::Copy {
                    text: c.text,
                    first,
                    began,
                },
            )
        }
        None => {
            // The default names no one's vault (theseus-8d1b): its file
            // missing, the start says where a config comes from.
            let default_missing = cli.config == DEFAULT_CONFIG
                && !theseus_core::config::expand(DEFAULT_CONFIG).exists();
            if default_missing {
                anyhow::bail!(NO_CONFIG);
            }
            let (cfg, text) = Config::load_text(&cli.config, &op).await?;
            (
                cfg,
                if in_vault {
                    Start::Vault(text)
                } else {
                    Start::File(text)
                },
            )
        }
    };
    let from = match &start {
        Start::File(_) => "file",
        Start::Vault(_) => "vault",
        Start::Copy { .. } => "copy",
    };
    startup.record(
        "config",
        false,
        t,
        json!({"source": if in_vault { "vault" } else { "file" }, "from": from}),
    );
    // The floor keeps the token file, whether the flag or the environment
    // named it, and the config's copy.
    cfg.op_token_file = op.token_file().map(std::path::Path::to_path_buf);
    cfg.config_copy = copy_path.clone();
    if let Some(Cmd::Config { sparse }) = &cli.cmd {
        if *sparse {
            // A note of only what differs (theseus-vwar), cut from the text
            // this config was read from: `config` reads its source itself,
            // never the copy (above).
            let (Start::File(text) | Start::Vault(text)) = &start else {
                anyhow::bail!("config --sparse reads the config's source, never its copy");
            };
            out(&theseus_core::config::sparse_note(text)?)?;
            return Ok(Exit::Done);
        }
        let mut text = format!("# source: {}\n", cli.config);
        if let (Some(p), Start::Vault(note)) = (&copy_path, &start) {
            text.push_str(&format!("# copy: {}\n", copy_line(p, &cli.config, note)));
        }
        text.push_str(&toml::to_string_pretty(&cfg)?);
        out(&text)?;
        return Ok(Exit::Done);
    }
    if let Some(Cmd::Restore {
        from,
        force,
        repair,
    }) = &cli.cmd
    {
        let fetch: Arc<dyn theseus_core::secrets::Fetch> = op.clone();
        restore(&cli, &cfg, from, *force, *repair, (fetch, origin)).await?;
        return Ok(Exit::Done);
    }
    tracing::info!(source = %cli.config, from, model = %cfg.model.model, "config loaded");

    // Serve first (FAST, §2): the secrets resolve in the background while
    // the store opens and the socket binds.
    let secrets = SecretBoard::for_config(&cfg.secrets, origin);
    tokio::spawn(theseus_core::secrets::resolve_into(
        secrets.clone(),
        cfg.secrets.clone(),
        op.clone(),
    ));

    let state_dir = cli.state_dir.clone().unwrap_or_else(|| cfg.state_dir());
    if let Some(Cmd::Check) = cli.cmd {
        check(&cli.config, &cfg, &secrets, &state_dir).await?;
        return Ok(Exit::Done);
    }

    private_dir(&state_dir)?;
    // A panic aborts the release build (`panic = "abort"`): its hook first
    // writes what panicked beside the store, which the next start reports
    // (Review 2's consideration 1). Each mode of a state dir has its own file.
    let mode = if cli.stdio { "stdio" } else { "socket" };
    theseus_core::crash::install(&state_dir, mode);
    // The store is single-process. A spawned stdio server must not fight a
    // running daemon for the same directory, so stdio mode uses its own.
    let store_name = if cli.stdio { "store-stdio" } else { "store" };
    let store_dir = state_dir.join(store_name);
    private_dir(&store_dir)?;
    private_dir(&theseus_core::rpc::spool_dir(&store_dir))?;
    let t = Instant::now();
    let store = Store::open(&store_dir)?;
    let st = store.stats()?;
    let mut detail = json!({"last_position": st.last_position, "wal_bytes": st.wal_bytes, "segments": st.wal_segments, "replayed_into_index": st.replayed_into_index, "history_bytes": st.history_bytes, "index_repaired": st.index_repaired, "lock_wait_ms": st.lock_wait_us as f64 / 1000.0});
    // An index that was not a database, moved aside and built again from
    // the WAL (theseus-0b8): the store's open logged it, and the ledger's
    // `store.index_replaced` row follows once serving.
    if let Some(m) = &st.index_moved_aside {
        detail["index_moved_aside"] = json!(m.path);
    }
    // A torn tail the open cut: where, whether a whole frame followed it,
    // and how far the log was known synced (theseus-gt12, theseus-7nfj).
    if let Some(cut) = &st.cut {
        detail["cut"] = json!(cut);
    }
    // The index's terms are not whole (an older build wrote last): they are
    // built after serving, and until then the kernel reads every record
    // (theseus-lv2).
    if st.terms_pending {
        detail["terms_pending"] = json!(true);
    }
    startup.record("store", false, t, detail);
    tracing::info!(
        last_position = st.last_position,
        wal_bytes = st.wal_bytes,
        segments = st.wal_segments,
        frames = st.frames_appended,
        syncs = st.syncs,
        "store open"
    );
    // A copy acts at once only when it is the one the daemon wrote: its
    // digest is in the store (theseus-zmgb). One edited since, or one an
    // older build wrote, makes this start one that reads the vault first.
    if let Start::Copy { text, .. } = &start {
        if let Err(why) = config_copy::written_by_daemon(&store, text) {
            tracing::warn!(why = %why, "the config copy cannot serve this start; restarting to read the vault first");
            return Ok(Exit::Exec(
                VAULT_FIRST_ENV,
                format!("the copy was not used: {why}"),
            ));
        }
    }
    // Where the config came from, the note to keep as the copy once serving
    // (a read before serving), and the vault's read of the copy (a start
    // from it).
    let (gate, keep, check) = match start {
        Start::File(_) => (ConfigGate::file(&cli.config), None, None),
        Start::Vault(text) => {
            let before = why_vault
                .clone()
                .unwrap_or_else(|| "there was no copy yet".to_string());
            let copy = copy_path
                .clone()
                .context("a vault config has a copy path")?;
            (
                ConfigGate::vault(&cli.config, copy, origin, Instant::now(), before),
                Some(text),
                None,
            )
        }
        Start::Copy { text, first, began } => {
            let copy = copy_path
                .clone()
                .context("a vault config has a copy path")?;
            (
                ConfigGate::from_copy(&cli.config, copy, text, origin),
                None,
                Some((first, began)),
            )
        }
    };
    let socket_path = cli.socket.clone().unwrap_or_else(|| cfg.socket_path());
    let bindings_path = cfg.discord.bindings_path(&state_dir);
    // Records `providers`, `kernel`, and `core`, one after another.
    let core = Core::new(cfg, secrets, store, startup.clone(), gate)?;
    let _ = CORE.set(Arc::downgrade(&core));
    // No L1 job's view shows this daemon's socket (M4 17b).
    core.tools.sandbox.hide(socket_path.clone());
    if let Some((first, began)) = check {
        tokio::spawn(config_gate::check(core.clone(), op, Some(first), began));
    }

    // The harness loop and the continuation driver start once the socket
    // answers (`after_serving`); neither waits for Discord, whose posts are
    // in the outbox (theseus-q4v).
    tokio::spawn(core.clone().watch_secrets());

    if cli.stdio {
        tracing::info!("serving protocol on stdio");
        // SIGINT and SIGTERM stop a `--stdio` daemon as they do the socket
        // one (theseus-p7q): a supervisor's stop, or an MCP client's kill,
        // used to end it outright, with no stopping row and no checkpoint,
        // and the next open of `store-stdio` replayed the tail and repaired
        // the index. Registered once, before anything is served.
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigint = signal(SignalKind::interrupt())?;
        let mut sigterm = signal(SignalKind::terminate())?;
        tokio::spawn(after_serving(core.clone(), keep, None, state_dir, mode));
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        // The one client is whoever holds the pipes: the parent that spawned
        // this daemon.
        let conn = core
            .clone()
            .serve_connection(stdin, stdout, Client::new("stdio", Surface::Cli));
        let served = tokio::select! {
            r = conn => r,
            _ = core.restart_asked() => Ok(()),
            _ = sigint.recv() => {
                tracing::info!(signal = "SIGINT", "stopping on a signal");
                core.stopping_on("SIGINT");
                Ok(())
            }
            _ = sigterm.recv() => {
                tracing::info!(signal = "SIGTERM", "stopping on a signal");
                core.stopping_on("SIGTERM");
                Ok(())
            }
        };
        // The same end as the socket daemon's: the posts in flight settle
        // (none, with no channel bound), and the index is checkpointed.
        core.finish_stop().await;
        flush_telemetry(&core).await;
        served?;
        return Ok(exit(&core));
    }

    // Only the socket daemon binds Discord, never `--stdio`: one gateway
    // connection per state dir, whose bindings file names its places.
    let after_bind = after_serving(core.clone(), keep, Some(bindings_path), state_dir, mode);
    let served = serve_socket(core.clone(), socket_path, after_bind).await;
    stop_phase("serving loop ended");
    // The index tender gets SIGTERM and is never waited for (§9), unless this
    // is a restart in place, whose next image takes it over (roadmap row 51).
    core.stop_index_tender();
    // The posts already sent settle within the stop's grace, and the index is
    // checkpointed after them (theseus-pfv).
    core.finish_stop().await;
    flush_telemetry(&core).await;
    stop_phase("telemetry flushed");
    served?;
    Ok(exit(&core))
}

/// How long a stopping daemon waits for telemetry's last batch.
const TELEMETRY_FLUSH: Duration = Duration::from_secs(1);

/// Telemetry's last batch as the daemon ends (§3.22): the traces still
/// waiting, and the metrics since the last interval. Bounded, so a receiver
/// that does not answer never holds a stop; with no endpoint, no wait.
async fn flush_telemetry(core: &Core) {
    let t = core.telemetry();
    if t.enabled() && !t.flush(TELEMETRY_FLUSH).await {
        tracing::warn!(
            "telemetry: its last batch was not sent within {} s; stopping without it",
            TELEMETRY_FLUSH.as_secs()
        );
    }
}

/// Print a subcommand's output. A reader that went away (`theseusd config |
/// head`) ends it quietly, as a closed pipe ends `cat`, instead of panicking
/// (theseus-gi7). The signal stays ignored: the daemon must never die of a
/// client that disconnects mid-write.
fn out(text: &str) -> Result<()> {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        r => Ok(r?),
    }
}

/// How the daemon ends: a restart onto the vault's changed note execs the
/// same image, marked so it never restarts again.
fn exit(core: &Core) -> Exit {
    match core.restart_requested() {
        Some(r) => Exit::Exec(
            config_gate::RESTARTED_ENV,
            serde_json::to_string(&r).unwrap_or_default(),
        ),
        None => Exit::Done,
    }
}

/// Become the same binary image with the same arguments, and `var` set
/// (theseus-2fo): the pid, the terminal, and any supervisor stay the same.
/// `/proc/self/exe` is this image even after an install renamed a newer
/// binary over its path. Returns only if the exec failed.
/// `dir`, the operator's alone (theseus-wz2): made 0700, with any parent it
/// lacks, or tightened when it is open to anyone else, as an older build
/// left the state dir (0775), the store, and the spool (0755). Only the group
/// and other bits go. A tightening that fails is a warning: the store still
/// serves, and the operator sees why.
fn private_dir(dir: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let mode = std::fs::metadata(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .permissions()
        .mode()
        & 0o7777;
    if mode & 0o077 != 0 {
        let tight = mode & !0o077;
        match std::fs::set_permissions(dir, std::fs::Permissions::from_mode(tight)) {
            Ok(()) => tracing::info!(
                dir = %dir.display(),
                from = %format!("{mode:04o}"),
                to = %format!("{tight:04o}"),
                "tightened: Theseus's state is the operator's alone"
            ),
            Err(e) => tracing::warn!(
                dir = %dir.display(),
                mode = %format!("{mode:04o}"),
                error = %e,
                "could not tighten: other users may read Theseus's state"
            ),
        }
    }
    Ok(())
}

fn exec_self(var: &str, value: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let mut args = std::env::args_os();
    let arg0 = args.next().unwrap_or_else(|| "theseusd".into());
    tracing::info!(var, "exec /proc/self/exe with the same arguments");
    let mut cmd = std::process::Command::new("/proc/self/exe");
    cmd.arg0(arg0).args(args).env(var, value);
    // The exec keeps this process's umask, 077 by now: the operator's rides
    // in the environment (theseus-wz2).
    if let Some(u) = theseus_kernel::umask::operator() {
        cmd.env(theseus_kernel::umask::ENV, theseus_kernel::umask::format(u));
    }
    let err = cmd.exec();
    Err(anyhow::Error::new(err).context("restarting: exec of /proc/self/exe failed"))
}

/// A serving daemon adopts a job's orphans (theseus-z4b): as a child
/// subreaper, it is where a job's descendant goes when the job kills its own
/// wrapper, instead of init, and it reaps them. The socket daemon and
/// `--stdio` alike (theseus-6uo). Set before any thread starts. After an exec restart, which keeps the pid
/// and so every child, the flag is set again and the children are learned
/// again: a live child whose command line is a wrapper's is that job's
/// wrapper, and any other is an orphan.
fn adopt_children() {
    use theseus_kernel::children;
    if let Err(why) = children::adopt() {
        tracing::warn!(error = %why, "not a child subreaper: a job that kills its own wrapper leaves orphans to init");
    }
    let found = children::relearn();
    if found != children::Relearned::default() {
        tracing::info!(
            wrappers = found.wrappers,
            tenders = found.tenders,
            orphans = found.orphans,
            zombies = found.zombies,
            "children kept across the restart"
        );
    }
}

/// How often the reaper sweeps without a SIGCHLD.
const SWEEP_EVERY: Duration = Duration::from_secs(10);

/// The core, once built: the reaper hands it each wrapper that a signal
/// ended, to learn whether the wrapper had reported (theseus-6uo). Weak: a
/// static that owned the core kept it, and the store, past the runtime, so
/// the index was never closed and every start repaired it (theseus-8ni).
static CORE: std::sync::OnceLock<std::sync::Weak<Core>> = std::sync::OnceLock::new();

/// Reap what this daemon holds (theseus-z4b): each job wrapper, and each
/// orphan it adopted, once it exits. Woken by SIGCHLD, which every child's
/// exit sends to its parent, and every `SWEEP_EVERY` besides. A sweep waits
/// only for pids it names, never for tokio's `op` processes, which tokio
/// waits for (`children`). tokio gives each listener of a signal its own
/// wake, so this one takes nothing from tokio's. A wrapper that a signal
/// ended goes to the core, which says whether it was lost (theseus-6uo); one
/// reaped before the core is built waits for it.
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn reap_children() {
    use std::os::unix::process::ExitStatusExt;
    let mut signalled: Vec<(u32, String, i32)> = Vec::new();
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigchld = match signal(SignalKind::child()) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!(error = %e, "no SIGCHLD listener: children are reaped every {} s", SWEEP_EVERY.as_secs());
            None
        }
    };
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + SWEEP_EVERY, SWEEP_EVERY);
    loop {
        match tokio::task::spawn_blocking(theseus_kernel::children::sweep).await {
            Ok(swept) => {
                for (pid, job, status) in &swept.wrappers {
                    tracing::debug!(pid, job = %job, status = %status, "reaped a job wrapper");
                    if let Some(sig) = status.signal() {
                        signalled.push((*pid, job.clone(), sig));
                    }
                }
                for (pid, status) in &swept.orphans {
                    tracing::info!(pid, status = %status, "reaped an orphan a job left");
                }
                // A tender's exit goes to its supervisor, which starts the
                // next (roadmap row 51). Before the core is built there is
                // none: the supervisor finds no tender, and starts one.
                if let Some(core) = CORE.get().and_then(std::sync::Weak::upgrade) {
                    for (pid, name, status) in swept.tenders {
                        if name == theseus_core::tender::NAME {
                            core.index.exited(pid, status);
                        }
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "the reaper's sweep failed"),
        }
        if let Some(core) = CORE.get().and_then(std::sync::Weak::upgrade) {
            for (pid, job, sig) in signalled.drain(..) {
                core.wrapper_signalled(pid, &job, sig);
            }
        }
        tokio::select! {
            _ = async {
                match &mut sigchld {
                    Some(s) => { s.recv().await; }
                    None => std::future::pending::<()>().await,
                }
            } => {}
            _ = tick.tick() => {}
        }
    }
}

/// A daemon that restarted itself is the image `/proc/self/exe`, which the
/// kernel names `exe` (theseus-2fo). Give it back its own name, before any
/// thread starts, so `ps`, `pgrep theseusd`, and `pkill theseusd` still find
/// it. A job wrapper, exec'd the same way, keeps `exe`.
fn keep_name() {
    if !std::fs::read_to_string("/proc/self/comm").is_ok_and(|c| c.trim_end() == "exe") {
        return;
    }
    let arg0 = std::env::args_os().next();
    let Some(name) = arg0
        .as_deref()
        .map(std::path::Path::new)
        .and_then(|p| p.file_name())
    else {
        return;
    };
    // The kernel keeps 15 bytes of a name.
    let mut name = name.to_string_lossy().into_owned();
    while name.len() > 15 {
        name.pop();
    }
    if let Err(e) = std::fs::write("/proc/self/comm", &name) {
        tracing::warn!(error = %e, "could not restore the process name after the restart");
    }
}

/// What `theseusd config` says of the copy: whether it matches the vault's note.
fn copy_line(path: &std::path::Path, reference: &str, vault: &str) -> String {
    let at = path.display();
    match config_copy::read(path, reference) {
        Ok(None) => {
            format!("none at {at} yet: the next start reads the vault first, then keeps one")
        }
        Err(why) => format!("{at} is not used ({why}): the next start reads the vault first"),
        Ok(Some(c)) => match config_copy::compare(&c.text, vault) {
            Compared::Same => format!("{at} matches the vault's note"),
            Compared::Comments => format!(
                "{at} differs from the vault's note only in comments or formatting: the next \
                 start confirms it and rewrites it"
            ),
            Compared::Changed(tables) => format!(
                "{at} differs from the vault's note in {}: a start from it restarts onto the \
                 vault's version",
                tables.join(", ")
            ),
        },
    }
}

/// A first start keeps the note it read as the last-known-good copy, once
/// serving (theseus-2fo), with its digest in the store (theseus-zmgb): never
/// a note with a credential in a URL.
fn keep_copy(core: &Core, text: &str) {
    let Some(path) = core.cfg.config_copy.clone() else {
        return;
    };
    let kept = match core.cfg.credential_in_url() {
        Some(field) => {
            tracing::warn!(field = %field, "no config copy is kept: this field carries a credential in its URL, so every start reads the vault first");
            format!("no copy is kept, since {field} carries a credential in its URL")
        }
        None => match config_copy::keep(&core.store, &path, &core.config_gate.reference(), text) {
            Ok(()) => {
                tracing::info!(copy = %path.display(), "config copy kept for the next start");
                "the copy is kept for the next start".to_string()
            }
            Err(e) => {
                tracing::warn!(copy = %path.display(), error = %format!("{e:#}"), "the config copy could not be written");
                format!("the copy could not be written ({e:#})")
            }
        },
    };
    core.config_gate.append_detail(&kept);
}

/// `theseusd check`: every secret resolves, or the check fails naming each
/// one that did not and why. It waits for the first round, as serving does not.
/// The secrets from outside the vault are named with their sources
/// (theseus-n88g.1). Then L1's self-test, on demand (theseus-gyin): `/bin/true`
/// in L1 over the view a job of a daemon on `state` gets, and its verdict. L1
/// is not required, so a failed self-test is said, and fails nothing.
async fn check(source: &str, cfg: &Config, secrets: &Arc<SecretBoard>, state: &Path) -> Result<()> {
    secrets.settle_all().await;
    let st = secrets.status();
    if !st.failed.is_empty() {
        anyhow::bail!(
            "{} secret(s) failed to resolve:\n  {}",
            st.failed.len(),
            st.failed
                .iter()
                .map(|f| format!("{}: {}", f.name, f.error))
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
    github_token_report(secrets.get(&cfg.github.token_secret), cfg.github.warn_days).await;
    // What a job may be handed, and the keys that stay the harness's own
    // (theseus-gh7): read from the config alone.
    let broker = theseus_core::broker::Broker::new(&cfg.broker, secrets.clone(), None);
    let kept = theseus_core::broker::harness_only(cfg, &broker);
    let l1 = theseus_core::toolrun::sandbox_for(cfg, state)
        .self_test()
        .await;
    // The secrets from outside the vault, by name and source; the vault
    // stays the recommended source (theseus-n88g.1).
    let outside = st
        .outside_vault_words()
        .map(|w| format!("{w}; the vault is the recommended source\n"))
        .unwrap_or_default();
    out(&format!(
        "ok: config loaded from {source}; {} secret(s) resolved in {} ms ({}): {}\n{outside}{}\nL1: \
         the self-test {}\n",
        st.ready.len(),
        st.settled_ms.unwrap_or(0),
        st.method.as_deref().filter(|m| !m.is_empty()).unwrap_or("nothing to fetch"),
        st.ready.join(", "),
        kept.line(),
        theseus_protocol::sandbox::launch_words(&l1)
    ))
}

/// What runs once the socket answers: the kernel's startup report and the
/// start path's phases in the ledger (one frame), the kernel's counts in the
/// log, the index tender (the socket daemon's, row 51), and the GitHub token
/// check once its secret resolves. The network and every fsync but the
/// kernel's one stay off the start path (§9).
///
/// Then the actors: the harness loop, the driver, telemetry, the web UI,
/// Discord (`bindings`: the socket daemon only), and the GitHub check. A
/// copy the daemon wrote acts at once (theseus-zmgb). `keep` is a note read
/// before serving, kept as the copy now.
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn after_serving(
    core: Arc<Core>,
    keep: Option<String>,
    bindings: Option<PathBuf>,
    state_dir: PathBuf,
    mode: &'static str,
) {
    let serving = core.startup_log.us(Instant::now());
    tracing::info!(serving_ms = serving / 1000, "serving");
    core.announce_serving(serving);
    // The crash file the last run left, if it panicked: said and kept.
    core.report_crash(&state_dir, mode);
    // A test's planted panic (a debug build's `THESEUS_TEST_PANIC`).
    theseus_core::crash::planted("after_serving");
    // What the store's open left unchecked, the WAL's history, is checked
    // now, in the background (theseus-8ni).
    core.check_store_history();
    // The index's terms, when an older build wrote last: built now, in the
    // background, never before serving (theseus-lv2).
    core.build_store_terms();
    // Raw job output no result will absorb goes, now and every hour
    // (theseus-2ij).
    core.sweep_spool_after_serving();
    if let Some(text) = keep {
        keep_copy(&core, &text);
    }
    let k = core.kernel_status();
    tracing::info!(
        admission_ceiling = k.admission_ceiling,
        executions = ?k.executions_by_state,
        actions = ?k.actions_by_state,
        quarantined = k.quarantined_completions,
        "kernel"
    );
    // The outbox's index, read once the socket answers, so that no answer and
    // no turn waits for it (theseus-q4v).
    let outbox = core.outbox.clone();
    tokio::task::spawn_blocking(move || outbox.warm());
    // The ontology's snapshot, by one META prefix scan (theseus-8kk.1): a
    // compile reads it from memory.
    core.warm_ontology();
    // The index tender (roadmap row 51; M6 §2.2), started once the socket
    // answers, never before, and by the socket daemon alone (`bindings` is
    // its): a `--stdio` daemon serves `store-stdio` for one client.
    if bindings.is_some() {
        tokio::spawn(core.index.clone().run());
        // The durability tender (AWS step 15), for the account that ships
        // the store: 2 s after serving, once its account's check passes.
        core.tend_durability_after_serving();
    }
    // The MCP servers `[mcp.servers]` attaches (M7 36b), each started now,
    // never before serving: their stored lists were offered from the start.
    core.mcp.start();
    // The harness loop (heartbeat reconciler, wrapper notify socket) and the
    // driver (continuation turns: job results, confirms, restarts). Both
    // write to the disk at once, and a write beside the socket's `bind` can
    // wait on the same journal commit: theseus-qa0 measured `bind` at 11 ms
    // instead of 1 when it did.
    tokio::spawn(theseus_core::harness::run(core.clone()));
    tokio::spawn(theseus_core::harness::drive(core.clone()));
    tokio::spawn(core.clone().install_telemetry());
    // Each bound AWS account's check (row 29, C1): its key from the board,
    // then `sts:GetCallerIdentity`, which must name the account (AWS design
    // §3.10). Its calls fail closed until it passes.
    if let Some(aws) = core.tools.aws.clone() {
        let log = core.startup_log.clone();
        tokio::spawn(async move {
            let phase = log.begin("aws.check", true, Instant::now());
            log.end(phase, aws.check_all().await);
        });
        // The budget's reconcile and reads, and GuardDuty's usage (C2), each
        // once its account's check has passed.
        core.tend_aws_after_serving();
        // The completion queue's poller (step 40): idle until a hands group
        // is open, then a long poll while one is.
        core.poll_hands_after_serving();
    }
    if let Some(path) = bindings {
        core.post_restart_notice();
        if core.cfg.web.enabled {
            let (bind, port) = (core.cfg.web.bind.clone(), core.cfg.web.port);
            let web_core = core.clone();
            tokio::spawn(async move {
                if let Err(e) = web::serve(web_core, &bind, port).await {
                    tracing::error!(error = %e, "web UI failed; protocol socket unaffected");
                }
            });
        }
        tokio::spawn(theseus_discord::run(
            core.clone(),
            core.cfg.discord.clone(),
            path,
        ));
    }
    let name = core.cfg.github.token_secret.clone();
    let t0 = Instant::now();
    let phase = core.startup_log.begin("github.check", true, t0);
    let detail = match core.secrets.settle(&name).await {
        Waited::Ready(token) => {
            let waited_ms = t0.elapsed().as_millis() as u64;
            let mut d = github_token_report(Some(token), core.cfg.github.warn_days).await;
            d["secret"] = json!(name);
            d["waited_ms"] = json!(waited_ms);
            d
        }
        Waited::Failed(why) => {
            tracing::warn!(secret = %name, error = %why, "GitHub token check skipped: its secret did not resolve");
            json!({"secret": name, "outcome": "failed", "error": why})
        }
        Waited::Absent | Waited::Resolving => json!({"secret": name, "outcome": "unconfigured"}),
    };
    core.startup_log.end(phase, detail);
}

/// Serve the protocol socket; `after_bind` starts once the socket answers.
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn serve_socket(
    core: Arc<Core>,
    path: PathBuf,
    after_bind: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let t = Instant::now();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() {
        // A stale socket from a previous run; if something is listening, fail rather than steal it.
        if tokio::net::UnixStream::connect(&path).await.is_ok() {
            anyhow::bail!(
                "another theseusd is already listening on {}",
                path.display()
            );
        }
        std::fs::remove_file(&path)?;
    }
    let listener =
        UnixListener::bind(&path).with_context(|| format!("binding {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    core.startup_log
        .record("socket", false, t, serde_json::Value::Null);
    tracing::info!(socket = %path.display(), secrets = %core.secrets.status().summary(), "serving protocol; localhost only, file permissions are the auth");
    tokio::spawn(after_bind);

    let mut conn_id: u64 = 0;
    // Registered once, before the loop: `notify_waiters` wakes only the
    // waiters registered when it is called, so a stop that lands while the
    // loop is taking a connection would otherwise be missed.
    let stop = core.shutdown.notified();
    tokio::pin!(stop);
    stop.as_mut().enable();
    // SIGINT and SIGTERM alike (theseus-bv5). SIGTERM is systemd's stop,
    // `kill`'s default, and most supervisors' signal; it used to kill the
    // daemon outright, its socket left behind. Each is registered once, as
    // the stop is, so one that lands while the loop takes a connection waits.
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                conn_id += 1;
                // Who connected, for the log: the peer's pid, as the socket
                // says it (`SO_PEERCRED`).
                let pid = stream.peer_cred().ok().and_then(|c| c.pid());
                let (r, w) = stream.into_split();
                let core = core.clone();
                let client = format!("sock#{conn_id}");
                tracing::info!(client = %client, pid = ?pid, "client connected");
                tokio::spawn(async move {
                    // The socket is mode 0600: whoever connects is the CLI.
                    let cli = Client::new(client.clone(), Surface::Cli);
                    if let Err(e) = core.serve_connection(r, w, cli).await {
                        tracing::warn!(client = %client, error = %e, "connection ended with error");
                    } else {
                        tracing::info!(client = %client, "client disconnected");
                    }
                });
            }
            _ = &mut stop => {
                tracing::info!("shutdown requested over protocol");
                break;
            }
            _ = core.restart_asked() => {
                tracing::info!("restarting onto the vault's changed config note");
                break;
            }
            _ = sigint.recv() => {
                tracing::info!(signal = "SIGINT", "stopping on a signal");
                core.stopping_on("SIGINT");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!(signal = "SIGTERM", "stopping on a signal");
                core.stopping_on("SIGTERM");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// `theseusd restore`: refuse while a daemon serves this store, then rebuild
/// it, or repair it from a copy (`--repair`, theseus-15g), or rebuild it
/// from what the durability tender shipped (`--from s3://…`, step 16).
async fn restore(
    cli: &Cli,
    cfg: &Config,
    from: &std::path::Path,
    force: bool,
    repair: bool,
    vault: (Arc<dyn theseus_core::secrets::Fetch>, Instant),
) -> Result<()> {
    let state_dir = cli.state_dir.clone().unwrap_or_else(|| cfg.state_dir());
    let socket = cli.socket.clone().unwrap_or_else(|| cfg.socket_path());
    if tokio::net::UnixStream::connect(&socket).await.is_ok() {
        anyhow::bail!(
            "a theseusd is serving on {}; stop it first (`theseus shutdown`): the store is single-process",
            socket.display()
        );
    }
    private_dir(&state_dir)?;
    if let Some(url) = from.to_str().filter(|f| f.starts_with("s3://")) {
        if repair {
            anyhow::bail!("--repair takes a local copy of the store, not an s3:// URL");
        }
        return restore_s3(cfg, url, &state_dir, &socket, force, vault).await;
    }
    if repair {
        let r = theseus_core::restore::repair(from, &state_dir)?;
        let mut text = format!("repaired {} frame(s) from {}:\n", r.patched.len(), r.from);
        for p in &r.patched {
            text.push_str(&format!(
                "  segment {} at offset {}: {} bytes, positions {} to {}\n",
                p.segment, p.offset, p.bytes, p.first, p.last
            ));
        }
        text.push_str(&format!(
            "{} session(s), last position {}\ninto {}\nthe store it repaired is kept at {}\n\
             start theseusd to serve it; the ledger's last row is store.restored\n",
            r.sessions, r.last_position, r.into, r.moved_aside
        ));
        return out(&text);
    }
    let r = theseus_core::restore::restore(from, &state_dir, force)?;
    let mut text = restore_lines(&r);
    text.push_str("start theseusd to serve it; the ledger's last row is store.restored\n");
    out(&text)
}

/// `theseusd restore --from s3://<bucket>/durability/<deployment>/` (step
/// 16): the account's key from the vault, then the fetch and the local
/// restore, in the account's restore session.
async fn restore_s3(
    cfg: &Config,
    url: &str,
    state_dir: &std::path::Path,
    socket: &std::path::Path,
    force: bool,
    (fetch, origin): (Arc<dyn theseus_core::secrets::Fetch>, Instant),
) -> Result<()> {
    let secrets = SecretBoard::for_config(&cfg.secrets, origin);
    tokio::spawn(theseus_core::secrets::resolve_into(
        secrets.clone(),
        cfg.secrets.clone(),
        fetch,
    ));
    let Some(aws) = theseus_core::aws::Aws::from_config(&cfg.aws, secrets) else {
        anyhow::bail!("the config binds no AWS account, so there is no bucket to restore from");
    };
    let r = theseus_core::aws::durable::restore::from_s3(
        &aws,
        url,
        state_dir,
        socket,
        force,
        &Default::default(),
    )
    .await?;
    let mut text = theseus_core::aws::durable::restore::lines(&r);
    text.push_str(&restore_lines(&r.restore));
    text.push_str(&theseus_core::aws::durable::restore::after_lines(&r));
    text.push_str("start theseusd to serve it; the ledger's last row is store.restored\n");
    out(&text)
}

/// A local restore's lines: what it restored, what it cut, and where.
fn restore_lines(r: &theseus_core::restore::RestoreReport) -> String {
    let mut text = format!(
        "restored {} segment(s): {} frames, {} records, last position {}\n",
        r.segments, r.frames, r.records, r.last_position
    );
    if r.truncated_bytes > 0 {
        text.push_str(&format!(
            "cut a torn final frame of {} bytes (a write the source never finished)\n",
            r.truncated_bytes
        ));
    }
    // A whole frame after the cut, which no mark made a refusal of
    // (theseus-7nfj): the source's last batch, torn before its sync.
    if let Some((cut, whole)) = r.cut.and_then(|c| c.whole_after.map(|w| (c, w))) {
        text.push_str(&format!(
            "  from position {} at offset {} of segment {}; a whole frame followed it (offset {}, \
             position {}), and no frame's mark said position {} was synced (known synced to {}): \
             a batch torn before its sync, or rot in the source's last batch, which no frame can \
             prove\n",
            cut.position,
            cut.offset,
            cut.segment,
            whole.offset,
            whole.first,
            cut.position,
            cut.synced_to
        ));
    }
    text.push_str(&format!(
        "{} session(s), {} node(s), {} ledger row(s)\nfrom {}\ninto {}\n",
        r.sessions, r.nodes, r.ledger_rows, r.from, r.into
    ));
    if let Some(a) = &r.moved_aside {
        text.push_str(&format!("the store that was there is kept at {a}\n"));
    }
    // Where its time went (theseus-byu).
    let phases: Vec<String> = r
        .phases_ms
        .iter()
        .map(|(name, ms)| format!("{name} {ms:.1}"))
        .collect();
    text.push_str(&format!("phases, ms: {}\n", phases.join(" · ")));
    text
}

/// Log when the GitHub token expires; warn loudly when close. Never fatal.
/// Returns what it found, for the startup phase.
#[expect(clippy::cognitive_complexity, reason = "shape budget: split it")]
async fn github_token_report(token: Option<Secret>, warn_days: i64) -> serde_json::Value {
    let Some(tok) = token else {
        return json!({"outcome": "unconfigured"});
    };
    let t0 = Instant::now();
    let found = theseus_core::github::token_status(&tok).await;
    let check_ms = t0.elapsed().as_millis() as u64;
    match found {
        Ok(st) => {
            let login = st.login.clone().unwrap_or_else(|| "?".into());
            let detail = json!({"outcome": "ok", "login": login, "days_left": st.days_left, "check_ms": check_ms});
            match st.days_left {
                Some(d) if d <= warn_days => tracing::warn!(
                    login = %login,
                    expires_at = %st.expires_at.clone().unwrap_or_default(),
                    days_left = d,
                    "GitHub token expires soon; rotate it in 1Password"
                ),
                Some(d) => tracing::info!(
                    login = %login,
                    expires_at = %st.expires_at.clone().unwrap_or_default(),
                    days_left = d,
                    fine_grained = st.fine_grained,
                    "GitHub token ok"
                ),
                None => tracing::info!(login = %login, "GitHub token ok (no expiry reported)"),
            }
            detail
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not check GitHub token; continuing");
            json!({"outcome": "error", "error": format!("{e:#}"), "check_ms": check_ms})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flag and the environment name the token file through one field,
    /// which `OpReader` records and the floor keeps (theseus-8az).
    #[test]
    fn the_token_file_comes_from_the_flag_or_the_environment() {
        let flag = Cli::try_parse_from(["theseusd", "--op-token-file", "/x/flag"]).unwrap();
        assert_eq!(flag.op_token_file.as_deref(), Some("/x/flag"));
        std::env::set_var("THESEUS_OP_TOKEN_FILE", "/x/env");
        let env = Cli::try_parse_from(["theseusd"]).unwrap();
        std::env::remove_var("THESEUS_OP_TOKEN_FILE");
        assert_eq!(env.op_token_file.as_deref(), Some("/x/env"));
    }

    /// `install --user`'s plan names the flag for a re-run: it must parse after the
    /// subcommand as well as before it (theseus-w1nf), and mean the same either way.
    #[test]
    fn the_token_file_flag_works_before_or_after_the_subcommand() {
        for argv in [
            ["theseusd", "--op-token-file", "/x/tok", "install", "--user"],
            ["theseusd", "install", "--user", "--op-token-file", "/x/tok"],
            ["theseusd", "install", "--op-token-file", "/x/tok", "--user"],
        ] {
            let cli = Cli::try_parse_from(argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
            assert_eq!(cli.op_token_file.as_deref(), Some("/x/tok"), "{argv:?}");
            assert!(
                matches!(&cli.cmd, Some(Cmd::Install(a)) if a.user),
                "{argv:?}: {:?}",
                cli.cmd
            );
        }
    }

    /// A job's command may carry words that look like the daemon's options. The wrapper takes
    /// everything after its own words as the command's, so a global `--op-token-file` there is
    /// the command's argument, not the flag (theseus-w1nf).
    #[test]
    fn a_job_commands_words_are_not_taken_for_the_global_flag() {
        let cli = Cli::try_parse_from([
            "theseusd",
            "job-wrapper",
            "--correlation-id",
            "c1",
            "--",
            "echo",
            "--op-token-file",
            "/x/tok",
        ])
        .unwrap();
        assert_eq!(cli.op_token_file, None);
        let Some(Cmd::JobWrapper { args }) = cli.cmd else {
            panic!("not a job wrapper: {:?}", cli.cmd);
        };
        for word in ["echo", "--op-token-file", "/x/tok"] {
            assert!(
                args.iter().any(|a| a == word),
                "{word:?} lost from {args:?}"
            );
        }
    }

    /// The core knows a serving daemon by its command line (theseus-6uo), so
    /// it must know which of the daemon's options take a value: exactly
    /// these, with no positional argument but the subcommands.
    #[test]
    fn a_serving_daemons_value_options_are_the_ones_the_core_skips() {
        use clap::CommandFactory;
        let cmd = Cli::command();
        let mut takes: Vec<String> = cmd
            .get_arguments()
            .filter(|a| a.get_action().takes_values())
            .filter_map(|a| a.get_long().map(|l| format!("--{l}")))
            .collect();
        takes.sort();
        let mut known: Vec<String> = theseus_kernel::job::DAEMON_VALUE_FLAGS
            .iter()
            .map(|s| s.to_string())
            .collect();
        known.sort();
        assert_eq!(takes, known);
        assert_eq!(cmd.get_positionals().count(), 0);
    }
}
