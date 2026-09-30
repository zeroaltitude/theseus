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
//! the vault serves from the note's last-known-good copy and reads the vault
//! behind the socket, beside the secrets (theseus-2fo). Until the vault
//! confirms the copy it answers only what reads, and nothing acts; a changed
//! note restarts the daemon onto the vault's version. A start with no copy
//! reads the vault first, and keeps the copy after serving.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use theseus_core::approval::{Client, Surface};
use theseus_core::config::DEFAULT_CONFIG_REF;
use theseus_core::config_copy::{self, Compared};
use theseus_core::config_gate::{self, ConfigGate};
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
        return out(Config::EXAMPLE_TOML);
    }
    if let Some(Cmd::ExampleBindings) = cli.cmd {
        return out(theseus_discord::EXAMPLE_BINDINGS);
    }
    if let Some(Cmd::JobWrapper { args }) = cli.cmd {
        // No config, no secrets: the wrapper only runs a command and spools.
        let wa = theseus_kernel::job::parse_wrapper_args(args)?;
        return theseus_kernel::job::run_wrapper(&wa);
    }
    keep_name();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    match rt.block_on(daemon(cli, origin))? {
        Exit::Done => Ok(()),
        Exit::Exec(var, value) => {
            // The clean shutdown path has run. The runtime's tasks go, and
            // with them the store, which closes; then the same image.
            rt.shutdown_timeout(Duration::from_millis(500));
            exec_self(var, &value)
        }
    }
}

/// How this start got its config.
enum Start {
    File,
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

async fn daemon(cli: Cli, origin: Instant) -> Result<Exit> {
    let op = Arc::new(OpReader::from_env(cli.op_token_file.as_deref())?);
    let startup = Arc::new(StartupLog::new(origin));
    let in_vault = cli.config.starts_with("op://");
    // The copy is found before any config is read: in `--state-dir`, else ~/.theseus.
    let copy_path = in_vault.then(|| config_copy::path(cli.state_dir.as_deref()));
    let t = Instant::now();
    // A start that serves (the socket or `--stdio`) serves from the copy;
    // `check`, `config`, and `restore` read the vault itself, as before.
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
            let (cfg, text) = Config::load_text(&cli.config, &op).await?;
            (
                cfg,
                if in_vault {
                    Start::Vault(text)
                } else {
                    Start::File
                },
            )
        }
    };
    let from = match &start {
        Start::File => "file",
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
    if let Some(Cmd::Config) = cli.cmd {
        let mut text = format!("# source: {}\n", cli.config);
        if let (Some(p), Start::Vault(note)) = (&copy_path, &start) {
            text.push_str(&format!("# copy: {}\n", copy_line(p, &cli.config, note)));
        }
        text.push_str(&toml::to_string_pretty(&cfg)?);
        out(&text)?;
        return Ok(Exit::Done);
    }
    if let Some(Cmd::Restore { from, force }) = &cli.cmd {
        restore(&cli, &cfg, from, *force).await?;
        return Ok(Exit::Done);
    }
    tracing::info!(source = %cli.config, from, model = %cfg.model.model, "config loaded");
    // Whether the config may act, the note to keep as the copy once serving
    // (a read before serving), and the vault's read of the copy (a start
    // from it).
    let (gate, keep, confirm) = match start {
        Start::File => (ConfigGate::file(&cli.config), None, None),
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

    // Serve first (FAST, §2): the secrets resolve in the background while
    // the store opens and the socket binds.
    let secrets = SecretBoard::new(cfg.secrets.keys().cloned(), origin);
    tokio::spawn(theseus_core::secrets::resolve_into(
        secrets.clone(),
        cfg.secrets.clone(),
        op.clone(),
    ));

    if let Some(Cmd::Check) = cli.cmd {
        check(&cli.config, &cfg, &secrets).await?;
        return Ok(Exit::Done);
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
    // Records `providers`, `kernel`, and `core`, one after another.
    let core = match Core::new(cfg, secrets, store, startup.clone(), gate) {
        Ok(core) => core,
        // A store with unit budgets is migrated only under the vault's own
        // config: this start becomes one that reads the vault first.
        Err(e) if unconfirmed_config(&e) => {
            tracing::warn!(error = %format!("{e:#}"), "restarting to read the vault's config note before serving");
            return Ok(Exit::Exec(
                VAULT_FIRST_ENV,
                "the store still holds unit budgets, which are migrated only under a config \
                 the vault has confirmed"
                    .into(),
            ));
        }
        Err(e) => return Err(e),
    };
    if let Some((first, began)) = confirm {
        tokio::spawn(config_gate::confirm(core.clone(), op, Some(first), began));
    }

    // The harness loop and the continuation driver start once the socket
    // answers and the config may act (`after_serving`).
    if !cli.stdio {
        // Discord binds then; continuations wait until it watches its sessions.
        core.bindings.expect();
    }
    tokio::spawn(core.clone().watch_secrets());

    if cli.stdio {
        tracing::info!("serving protocol on stdio");
        tokio::spawn(after_serving(core.clone(), keep, None));
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();
        let conn = core
            .clone()
            .serve_connection(stdin, stdout, Client::new("stdio", Surface::Cli));
        let served = tokio::select! {
            r = conn => r,
            _ = core.restart_asked() => Ok(()),
        };
        flush_telemetry(&core).await;
        served?;
        return Ok(exit(&core));
    }

    // Only the socket daemon binds Discord: one bot token, one gateway connection.
    let after_bind = after_serving(core.clone(), keep, Some(bindings_path));
    let served = serve_socket(core.clone(), socket_path, after_bind).await;
    flush_telemetry(&core).await;
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

/// Whether the kernel refused a store with unit budgets under a config the
/// vault has not confirmed (theseus-2fo).
fn unconfirmed_config(e: &anyhow::Error) -> bool {
    matches!(
        e.downcast_ref::<theseus_kernel::KernelError>(),
        Some(theseus_kernel::KernelError::UnconfirmedConfig { .. })
    )
}

/// Become the same binary image with the same arguments, and `var` set
/// (theseus-2fo): the pid, the terminal, and any supervisor stay the same.
/// `/proc/self/exe` is this image even after an install renamed a newer
/// binary over its path. Returns only if the exec failed.
fn exec_self(var: &str, value: &str) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let mut args = std::env::args_os();
    let arg0 = args.next().unwrap_or_else(|| "theseusd".into());
    tracing::info!(var, "exec /proc/self/exe with the same arguments");
    let err = std::process::Command::new("/proc/self/exe")
        .arg0(arg0)
        .args(args)
        .env(var, value)
        .exec();
    Err(anyhow::Error::new(err).context("restarting: exec of /proc/self/exe failed"))
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
/// serving (theseus-2fo): never a note with a credential in a URL.
fn keep_copy(core: &Core, text: &str) {
    let Some(path) = core.cfg.config_copy.clone() else {
        return;
    };
    let kept = match core.cfg.credential_in_url() {
        Some(field) => {
            tracing::warn!(field = %field, "no config copy is kept: this field carries a credential in its URL, so every start reads the vault first");
            format!("no copy is kept, since {field} carries a credential in its URL")
        }
        None => match config_copy::write(&path, &core.config_gate.reference(), text) {
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
    out(&format!(
        "ok: config loaded from {source}; {} secret(s) resolved in {} ms ({}): {}\n",
        st.ready.len(),
        st.settled_ms.unwrap_or(0),
        st.method.as_deref().unwrap_or("nothing to fetch"),
        st.ready.join(", ")
    ))
}

/// What runs once the socket answers: the kernel's startup report and the
/// start path's phases in the ledger (one frame), the kernel's counts in the
/// log, and the GitHub token check once its secret resolves. The network and
/// every fsync but the kernel's one stay off the start path (§9).
///
/// Then, once the config may act (at once for a file or a vault read before
/// serving; when the vault confirms the copy otherwise, theseus-2fo), the
/// actors: the harness loop, the driver, telemetry, the web UI, Discord
/// (`bindings`: the socket daemon only), and the GitHub check. Nothing acts,
/// sends a secret, or talks to the network on a copy's word. `keep` is a
/// note read before serving, kept as the copy now.
async fn after_serving(core: Arc<Core>, keep: Option<String>, bindings: Option<PathBuf>) {
    let serving = core.startup_log.us(Instant::now());
    tracing::info!(serving_ms = serving / 1000, "serving");
    core.announce_serving(serving);
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
    if !core.config_gate.opened().await {
        return;
    }
    // The harness loop (heartbeat reconciler, wrapper notify socket) and the
    // driver (continuation turns: job results, confirms, restarts). Both
    // write to the disk at once, and a write beside the socket's `bind` can
    // wait on the same journal commit: theseus-qa0 measured `bind` at 11 ms
    // instead of 1 when it did.
    tokio::spawn(theseus_core::harness::run(core.clone()));
    tokio::spawn(theseus_core::harness::drive(core.clone()));
    tokio::spawn(core.clone().install_telemetry());
    if let Some(path) = bindings {
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
            _ = core.restart_asked() => {
                tracing::info!("restarting onto the vault's changed config note");
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
    text.push_str(&format!(
        "{} session(s), {} node(s), {} ledger row(s)\nfrom {}\ninto {}\n",
        r.sessions, r.nodes, r.ledger_rows, r.from, r.into
    ));
    if let Some(a) = &r.moved_aside {
        text.push_str(&format!("the store that was there is kept at {a}\n"));
    }
    text.push_str("start theseusd to serve it; the ledger's last row is store.restored\n");
    out(&text)
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
