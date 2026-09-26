//! `theseusd`: the Theseus server.
//!
//! Startup order: read the service-account token, load config (from 1Password
//! or a file), resolve every secret or refuse to start, open the store, then
//! serve the protocol on stdio (`--stdio`) or a Unix socket (default).

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use theseus_core::config::DEFAULT_CONFIG_REF;
use theseus_core::secrets::{OpReader, Secrets};
use theseus_core::store::Store;
use theseus_core::{Config, Core};
use tokio::net::UnixListener;

mod web;

const AFTER_HELP: &str = "\
Running it:
  With no COMMAND, theseusd serves: it loads config, resolves every secret from 1Password
  (refusing to start if any is missing), opens the store, then listens on the Unix socket
  and the web UI (http://127.0.0.1:7433/). It stays in the foreground; Ctrl-C stops it.

  export OP_SERVICE_ACCOUNT_TOKEN=...   the only secret allowed outside 1Password
  theseusd check                        prove the vault wiring, then exit
  theseusd config                       show the config actually loaded, and from where
  theseusd                              serve (foreground); add & to background it
  THESEUS_LOG=debug theseusd            more detail (tracing filter syntax)
  theseusd --socket /tmp/dbg.sock       a scratch instance beside a running one

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

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Print a template config (TOML) and exit. A starting point, not the loaded config.
    ExampleConfig,
    /// Load config, resolve every secret, report, and exit without serving.
    Check,
    /// Print the loaded config (TOML, secret references only, never values) and its source.
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
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
        print!("{}", toml::to_string_pretty(&Config::example())?);
        return Ok(());
    }

    let op = OpReader::from_env(cli.op_token_file.as_deref())?;
    let cfg = Config::load(&cli.config, &op).await?;
    if let Some(Cmd::Config) = cli.cmd {
        println!("# source: {}", cli.config);
        print!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(());
    }
    tracing::info!(source = %cli.config, model = %cfg.model.model, "config loaded");
    let secrets = Secrets::resolve_all(&cfg.secrets, &op).await?;
    tracing::info!(count = secrets.names().len(), names = ?secrets.names(), "all secrets resolved");

    github_token_report(&cfg, &secrets).await;

    if let Some(Cmd::Check) = cli.cmd {
        println!(
            "ok: config loaded from {}; {} secret(s) resolved: {}",
            cli.config,
            secrets.names().len(),
            secrets.names().join(", ")
        );
        return Ok(());
    }

    let state_dir = cfg.state_dir();
    std::fs::create_dir_all(&state_dir)
        .with_context(|| format!("creating state dir {}", state_dir.display()))?;
    // The embedded store is single-process. A spawned stdio server must not fight a
    // running daemon for the same file, so stdio mode uses its own store.
    let store_name = if cli.stdio {
        "theseus-stdio.redb"
    } else {
        "theseus.redb"
    };
    let store = Store::open(&state_dir.join(store_name))?;
    let socket_path = cli.socket.clone().unwrap_or_else(|| cfg.socket_path());
    let core = Core::new(cfg, secrets, store)?;
    tracing::info!(
        ledger_rows = core.store.ledger_len().unwrap_or(0),
        "store open"
    );

    if cli.stdio {
        tracing::info!("serving protocol on stdio");
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        core.clone()
            .serve_connection(stdin, stdout, "stdio".into())
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

    serve_socket(core, socket_path).await
}

async fn serve_socket(core: Arc<Core>, path: PathBuf) -> Result<()> {
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
    tracing::info!(socket = %path.display(), "serving protocol; localhost only, file permissions are the auth");

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
                    if let Err(e) = core.serve_connection(r, w, client.clone()).await {
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

/// Log when the GitHub token expires; warn loudly when close. Never fatal.
async fn github_token_report(cfg: &Config, secrets: &Secrets) {
    let Some(tok) = secrets.get(&cfg.github.token_secret) else {
        return;
    };
    match theseus_core::github::token_status(tok).await {
        Ok(st) => {
            let login = st.login.clone().unwrap_or_else(|| "?".into());
            match st.days_left {
                Some(d) if d <= cfg.github.warn_days => tracing::warn!(
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
        }
        Err(e) => tracing::warn!(error = %e, "could not check GitHub token; continuing"),
    }
}
