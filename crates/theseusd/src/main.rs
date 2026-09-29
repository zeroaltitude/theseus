//! `theseusd`: the Theseus server.
//!
//! Startup order (serve first, theseus-qa0): read the service-account token,
//! load config (from 1Password or a file), start resolving the secrets in the
//! background, open the store, run the kernel's startup, then serve the
//! protocol on stdio (`--stdio`) or a Unix socket (default). Nothing on that
//! path waits for a secret: each consumer waits for its own, and one that
//! fails to resolve never runs without it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use theseus_core::approval::{Client, Surface};
use theseus_core::config::DEFAULT_CONFIG_REF;
use theseus_core::secrets::{OpReader, Secret, SecretBoard, Waited};
use theseus_core::startup::StartupLog;
use theseus_core::store::Store;
use theseus_core::{Config, Core};
use tokio::net::UnixListener;

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
  theseusd example-bindings             the Discord bindings file format
  theseusd                              serve (foreground); add & to background it
  THESEUS_LOG=debug theseusd            more detail (tracing filter syntax)
  theseusd --socket /tmp/dbg.sock --state-dir /tmp/dbg   a scratch instance beside a running one

Config source (--config / THESEUS_CONFIG): an op:// reference (default: the 1Password item
theseus-config) or a local TOML file. `theseusd example-config` prints a template; it is
not what the server runs with.";

#[derive(Parser, Debug)]
#[command(
    name = "theseusd",
    version,
    about = "Theseus server: config and secrets from 1Password, the turn kernel, the protocol on a Unix socket or stdio, and the localhost web UI.",
    after_help = AFTER_HELP
)]
struct Cli {
    /// Config source: an op:// reference or a file path.
    #[arg(long, env = "THESEUS_CONFIG", default_value = DEFAULT_CONFIG_REF)]
    config: String,

    /// File holding the 1Password service-account token (used when OP_SERVICE_ACCOUNT_TOKEN is unset).
    #[arg(long, env = "THESEUS_OP_TOKEN_FILE")]
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
    Config,
    /// Print an annotated bindings file (the Discord guild, channel, and who may drive it) and exit.
    ExampleBindings,
    /// Rebuild the store from a local WAL directory (or another store's directory) and exit.
    /// The source is only read; a store already in place is moved aside with --force, never deleted.
    Restore {
        /// A WAL directory (holding *.seg files) or a store directory (holding wal/).
        #[arg(long)]
        from: PathBuf,
        /// Move the existing store aside instead of refusing.
        #[arg(long)]
        force: bool,
    },
    /// Internal: the detached job wrapper (spawned by the kernel, never by hand).
    #[command(hide = true, disable_help_flag = true)]
    JobWrapper {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // The start of every startup phase's clock (theseus-qa0).
    let origin = Instant::now();
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
        print!("{}", Config::EXAMPLE_TOML);
        return Ok(());
    }
    if let Some(Cmd::ExampleBindings) = cli.cmd {
        print!("{}", theseus_discord::EXAMPLE_BINDINGS);
        return Ok(());
    }
    if let Some(Cmd::JobWrapper { args }) = cli.cmd {
        // No config, no secrets: the wrapper only runs a command and spools.
        let wa = theseus_kernel::job::parse_wrapper_args(args)?;
        return theseus_kernel::job::run_wrapper(&wa);
    }

    let op = OpReader::from_env(cli.op_token_file.as_deref())?;
    let startup = Arc::new(StartupLog::new(origin));
    let t = Instant::now();
    let mut cfg = Config::load(&cli.config, &op).await?;
    // The config note is read through `op` too: on the start path, because
    // everything after it needs it (the report's open question).
    let source = if cli.config.starts_with("op://") {
        "vault"
    } else {
        "file"
    };
    startup.record("config", false, t, json!({"source": source}));
    // The floor keeps the token file, whether the flag or the environment named it.
    cfg.op_token_file = op.token_file().map(std::path::Path::to_path_buf);
    if let Some(Cmd::Config) = cli.cmd {
        println!("# source: {}", cli.config);
        print!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(());
    }
    if let Some(Cmd::Restore { from, force }) = &cli.cmd {
        return restore(&cli, &cfg, from, *force).await;
    }
    tracing::info!(source = %cli.config, model = %cfg.model.model, "config loaded");

    // Serve first (FAST, §2): the secrets resolve in the background while
    // the store opens and the socket binds.
    let secrets = SecretBoard::new(cfg.secrets.keys().cloned(), origin);
    tokio::spawn(theseus_core::secrets::resolve_into(
        secrets.clone(),
        cfg.secrets.clone(),
        Arc::new(op),
    ));

    if let Some(Cmd::Check) = cli.cmd {
        return check(&cli.config, &cfg, &secrets).await;
    }

    let state_dir = cli.state_dir.clone().unwrap_or_else(|| cfg.state_dir());
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    // The store is single-process. A spawned stdio server must not fight a
    // running daemon for the same directory, so stdio mode uses its own.
    let store_name = if cli.stdio { "store-stdio" } else { "store" };
    let t = Instant::now();
    let store = Store::open(&state_dir.join(store_name))?;
    let st = store.stats()?;
    startup.record(
        "store",
        false,
        t,
        json!({"last_position": st.last_position, "wal_bytes": st.wal_bytes, "segments": st.wal_segments, "replayed_into_index": st.replayed_into_index}),
    );
    tracing::info!(
        last_position = st.last_position,
        wal_bytes = st.wal_bytes,
        segments = st.wal_segments,
        frames = st.frames_appended,
        syncs = st.syncs,
        "store open"
    );
    let socket_path = cli.socket.clone().unwrap_or_else(|| cfg.socket_path());
    let bindings_path = cfg.discord.bindings_path(&state_dir);
    let t = Instant::now();
    let core = Core::new(cfg, secrets, store, startup.clone())?;
    startup.record("core", false, t, serde_json::Value::Null);

    // The harness loop: heartbeat reconciler and the wrapper notify socket;
    // the driver takes continuation turns (job results, confirms, restarts).
    tokio::spawn(theseus_core::harness::run(core.clone()));
    if !cli.stdio {
        // Discord binds below; continuations wait until it watches its sessions.
        core.bindings.expect();
    }
    tokio::spawn(theseus_core::harness::drive(core.clone()));
    tokio::spawn(core.clone().watch_secrets());
    tokio::spawn(core.clone().install_telemetry());

    if cli.stdio {
        tracing::info!("serving protocol on stdio");
        tokio::spawn(after_serving(core.clone()));
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        core.clone()
            .serve_connection(stdin, stdout, Client::new("stdio", Surface::Cli))
            .await?;
        return Ok(());
    }

    if core.cfg.web.enabled {
        let (bind, port) = (core.cfg.web.bind.clone(), core.cfg.web.port);
        let web_core = core.clone();
        tokio::spawn(async move {
            if let Err(e) = web::serve(web_core, &bind, port).await {
                tracing::error!(error = %e, "web UI failed; protocol socket unaffected");
            }
        });
    }

    // Only the socket daemon binds Discord: one bot token, one gateway connection.
    tokio::spawn(theseus_discord::run(
        core.clone(),
        core.cfg.discord.clone(),
        bindings_path,
    ));

    let after_bind = after_serving(core.clone());
    serve_socket(core, socket_path, after_bind).await
}

/// `theseusd check`: every secret resolves, or the check fails naming each
/// one that did not and why. It waits for the first round, as serving does not.
async fn check(source: &str, cfg: &Config, secrets: &Arc<SecretBoard>) -> Result<()> {
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
    println!(
        "ok: config loaded from {source}; {} secret(s) resolved in {} ms ({}): {}",
        st.ready.len(),
        st.settled_ms.unwrap_or(0),
        st.method.as_deref().unwrap_or("nothing to fetch"),
        st.ready.join(", ")
    );
    Ok(())
}

/// What runs once the socket answers: the start path's phases in the
/// ledger, the kernel's counts in the log, and the GitHub token check once
/// its secret resolves. The network stays off the start path (§9).
async fn after_serving(core: Arc<Core>) {
    let serving = core.startup_log.us(Instant::now());
    let phases: Vec<_> = core
        .startup_log
        .snapshot()
        .into_iter()
        .filter(|p| !p.background)
        .collect();
    tracing::info!(serving_ms = serving / 1000, "serving");
    core.binding_ledger(
        "server.serving",
        None,
        json!({"serving_us": serving, "phases": phases}),
    );
    let k = core.kernel_status();
    tracing::info!(
        admission_ceiling = k.admission_ceiling,
        executions = ?k.executions_by_state,
        actions = ?k.actions_by_state,
        quarantined = k.quarantined_completions,
        "kernel"
    );
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
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                conn_id += 1;
                let (r, w) = stream.into_split();
                let core = core.clone();
                let client = format!("sock#{conn_id}");
                tracing::info!(client = %client, "client connected");
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
            _ = core.shutdown.notified() => {
                tracing::info!("shutdown requested over protocol");
                break;
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("SIGINT");
                break;
            }
        }
    }
    let _ = std::fs::remove_file(&path);
    Ok(())
}

/// `theseusd restore`: refuse while a daemon serves this store, then rebuild it.
async fn restore(cli: &Cli, cfg: &Config, from: &std::path::Path, force: bool) -> Result<()> {
    let state_dir = cli.state_dir.clone().unwrap_or_else(|| cfg.state_dir());
    let socket = cli.socket.clone().unwrap_or_else(|| cfg.socket_path());
    if tokio::net::UnixStream::connect(&socket).await.is_ok() {
        anyhow::bail!(
            "a theseusd is serving on {}; stop it first (`theseus shutdown`): the store is single-process",
            socket.display()
        );
    }
    std::fs::create_dir_all(&state_dir)?;
    let r = theseus_core::restore::restore(from, &state_dir, force)?;
    println!(
        "restored {} segment(s): {} frames, {} records, last position {}",
        r.segments, r.frames, r.records, r.last_position
    );
    if r.truncated_bytes > 0 {
        println!(
            "cut a torn final frame of {} bytes (a write the source never finished)",
            r.truncated_bytes
        );
    }
    println!(
        "{} session(s), {} node(s), {} ledger row(s)",
        r.sessions, r.nodes, r.ledger_rows
    );
    println!(
        "from {}
into {}",
        r.from, r.into
    );
    if let Some(a) = &r.moved_aside {
        println!("the store that was there is kept at {a}");
    }
    println!("start theseusd to serve it; the ledger's last row is store.restored");
    Ok(())
}

/// Log when the GitHub token expires; warn loudly when close. Never fatal.
/// Returns what it found, for the startup phase.
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
}
