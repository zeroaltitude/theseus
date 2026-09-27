//! `theseus`: the CLI. A thin client that speaks the protocol to `theseusd`
//! over its Unix socket, or spawns `theseusd --stdio` and talks over pipes.
//! Built for shells: prompt from an argument or stdin, streamed reply on
//! stdout, diagnostics on stderr, `--json` for machines, meaningful exit codes.
//!
//! Exit codes: 0 ok · 1 server/provider error · 2 usage · 3 cannot connect.

use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;
use theseus_protocol::{
    method, notify, ActionConfirmParams, ActionConfirmResult, CatalogListResult, ConfirmRequest,
    HealthResult, HooksListResult, HooksRegisterParams, Id, LedgerTailParams, LedgerTailResult,
    Message, NodeInfo, ProfileListResult, ProfileUseParams, Request, SessionHistoryParams,
    SessionHistoryResult, SessionInfo, SessionListResult, SessionOpenParams,
    SessionRecompileParams, SessionRef, ToolListResult, TurnSubmitParams, TurnSubmitResult,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

const AFTER_HELP: &str = "\
Quick start:
  export OP_SERVICE_ACCOUNT_TOKEN=...        the one secret allowed outside 1Password
  theseusd &                                 start the server (or run it in the foreground)
  theseus health                             is it up, which profile is live, token totals
  theseus ask \"Say hello.\"                  one turn, streamed
  echo \"Summarize: ...\" | theseus ask --json  pipelines
  theseus profile use glm                    switch the live profile (persists)
  theseus ask -P sonnet \"...\"               one turn under another profile
  theseus ledger -n 20 -k provider.call      what every call cost and how long it took
  theseus ask -s <session> \"...\"            continue a session (its whole history is the context)
  theseus history [session]                  a session's transcript: messages, tool calls, results
  theseus watch [session]                    follow a session live (turns started anywhere)
  theseus confirm [id] [--deny]              answer a tool call waiting for you (no id: list them)
  theseus tools                              the toollets, their policy, and calls so far
  theseus catalog                            models, context windows, and prices
  theseus --spawn ask \"...\"                 no daemon: spawn theseusd on stdio for one turn
  theseus shutdown

Web UI:      http://127.0.0.1:7433/  (while theseusd runs)
Exit codes:  0 ok · 1 server or provider error · 2 usage · 3 cannot connect
More:        theseus <command> --help";

#[derive(Parser, Debug)]
#[command(
    name = "theseus",
    version,
    about = "Theseus CLI: a thin client for the Theseus server (theseusd), built for shells and pipelines.",
    long_about = "Theseus CLI.\n\nA thin client that speaks the Theseus protocol (JSON-RPC over newline-delimited JSON) \
to a running theseusd over its Unix socket, or spawns one on stdio with --spawn. Prompts come from an \
argument or stdin; replies stream to stdout; diagnostics go to stderr; --json gives one JSON object for machines.",
    after_help = AFTER_HELP,
    args_conflicts_with_subcommands = false
)]
struct Cli {
    /// Unix socket of a running theseusd.
    #[arg(
        long,
        env = "THESEUS_SOCKET",
        default_value = "~/.theseus/theseus.sock",
        global = true
    )]
    socket: String,

    /// Spawn `theseusd --stdio` (optionally a path to the binary) instead of connecting to the socket.
    #[arg(long, global = true, num_args = 0..=1, default_missing_value = "theseusd")]
    spawn: Option<String>,

    /// Machine-readable output: the final result as one JSON object on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Do not stream the reply; print it once at the end.
    #[arg(long, global = true)]
    no_stream: bool,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Send one prompt as one turn and print the reply. Reads stdin when PROMPT is omitted or "-".
    Ask {
        prompt: Option<String>,
        /// Continue an existing session instead of opening a new one.
        #[arg(long, short)]
        session: Option<String>,
        /// Profile for this turn (default: the live profile).
        #[arg(long = "profile", short = 'P')]
        profile: Option<String>,
        /// Raw provider override for this turn (a configured provider name, e.g. anthropic, zai).
        #[arg(long, short)]
        provider: Option<String>,
        /// Model id for this turn (e.g. claude-sonnet-5, glm-5.3-flash).
        #[arg(long, short)]
        model: Option<String>,
        /// After the reply, print the turn's timing tree (turn > loops > hooks/provider) to stderr.
        #[arg(long)]
        trace: bool,
        /// Show the model's thinking summaries on stderr as they stream.
        #[arg(long)]
        thinking: bool,
    },
    /// A session's transcript: messages, tool calls with their gate decisions, results, and
    /// anything waiting for your confirmation. SESSION defaults to the most recently active.
    History {
        session: Option<String>,
        /// Only the newest N nodes.
        #[arg(short, long)]
        n: Option<usize>,
        /// Print tool results and long messages in full (default: clipped).
        #[arg(long)]
        full: bool,
    },
    /// Follow a session live: streamed text, tool calls, confirmations, context decisions,
    /// whoever started the turn (web UI, CLI, the harness). SESSION defaults to the most recent.
    Watch {
        session: Option<String>,
        /// Show thinking summaries too.
        #[arg(long)]
        thinking: bool,
    },
    /// Answer a tool call waiting for your confirmation, then follow the turn it resumes.
    /// Without an id, list everything waiting.
    Confirm {
        correlation_id: Option<String>,
        /// Decline instead of approve (the model is told, and carries on without it).
        #[arg(long)]
        deny: bool,
        /// A note for the ledger and, on a decline, for the model.
        #[arg(long)]
        note: Option<String>,
        /// Return as soon as the answer is recorded instead of following the resumed turn.
        #[arg(long)]
        no_wait: bool,
    },
    /// The toollets: class, backend, what policy does with each, and calls so far.
    Tools {
        /// Also print each tool's description and input schema.
        #[arg(long, short)]
        verbose: bool,
    },
    /// The model catalog: context windows, output limits, prices per million tokens.
    Catalog,
    /// Server health: version, live profile, providers, sessions, turns, provider errors, token totals.
    Health,
    /// Sessions: list them with per-session token totals, or open one to continue across turns.
    Sessions {
        #[command(subcommand)]
        cmd: Option<SessionsCmd>,
    },
    /// Hook events and registered handlers; `hooks watch <event>` observes one live.
    Hooks {
        #[command(subcommand)]
        cmd: HooksCmd,
    },
    /// Executions (one per session): state, turns, outstanding actions, budget; `executions cancel <id>`.
    Executions {
        #[command(subcommand)]
        cmd: Option<ExecutionsCmd>,
    },
    /// Model profiles: list, or switch the live one (`theseus profile use glm`).
    Profile {
        #[command(subcommand)]
        cmd: Option<ProfileCmd>,
    },
    /// Recent ledger rows (every turn, loop, provider call, hook site, error).
    Ledger {
        /// How many rows.
        #[arg(short, long, default_value_t = 20)]
        n: usize,
        /// Only rows of this kind, e.g. turn.ended, provider.call, provider.error, hook.site.
        #[arg(short, long)]
        kind: Option<String>,
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Send a raw JSON-RPC request (e.g. `rpc health`, `rpc turn.submit '{"input":"hi"}'`); notifications echo to stderr.
    Rpc {
        method: String,
        params: Option<String>,
    },
    /// Ask the server to stop cleanly (removes its socket).
    Shutdown,
}

#[derive(Subcommand, Debug)]
enum ExecutionsCmd {
    /// List every execution with state, turns, outstanding actions, and budget.
    List,
    /// Cancel an execution: deterministic control path, terminates its jobs.
    Cancel { execution_id: String },
}

#[derive(Subcommand, Debug)]
enum ProfileCmd {
    /// List profiles; `*` marks the live one and where the choice came from (default).
    List,
    /// Make NAME the live profile (persists across restarts).
    Use { name: String },
}

#[derive(Subcommand, Debug)]
enum SessionsCmd {
    /// List sessions with turns and tokens in/out (default).
    List,
    /// Open a session and print its id; pass it to `ask -s` to keep turns together.
    Open {
        /// Human label shown in listings.
        #[arg(long)]
        label: Option<String>,
    },
    /// Ask for a recompile on the session's next turn: `fresh` keeps only the current exchange,
    /// `transcript` keeps everything with thinking stripped.
    Recompile {
        session: String,
        #[arg(long, default_value = "fresh", value_parser = ["fresh", "transcript"])]
        strategy: String,
    },
}

#[derive(Subcommand, Debug)]
enum HooksCmd {
    /// Every hook event with its kind (gate/transform/claim/observe) and handler count (default).
    List,
    /// Register as an observer of EVENT and print each hook.event as it arrives (Ctrl-C to stop).
    Watch {
        event: String,
        #[arg(long, default_value = "cli-watch")]
        handler_id: String,
    },
}

/// A JSON-RPC error response, kept structured so callers can read `data`.
#[derive(Debug)]
struct CallError {
    code: i64,
    message: String,
    data: Value,
}

impl std::fmt::Display for CallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let class = self.data.get("class").and_then(Value::as_str);
        let transient = self.data.get("transient").and_then(Value::as_bool);
        match (class, transient) {
            (Some(c), Some(t)) => write!(
                f,
                "{} [class={c}, transient={t}, code {}]",
                self.message, self.code
            ),
            _ => write!(f, "{} (code {})", self.message, self.code),
        }
    }
}

impl std::error::Error for CallError {}

struct Conn {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    _child: Option<tokio::process::Child>,
    next_id: u64,
}

impl Conn {
    async fn socket(path: &str) -> Result<Self> {
        let path = PathBuf::from(shellexpand::tilde(path).into_owned());
        let stream = tokio::net::UnixStream::connect(&path)
            .await
            .with_context(|| {
                format!(
                    "connecting to theseusd at {} (is it running? try --spawn)",
                    path.display()
                )
            })?;
        let (r, w) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(Box::new(r)),
            writer: Box::new(w),
            _child: None,
            next_id: 1,
        })
    }

    async fn spawn(bin: &str) -> Result<Self> {
        let mut child = tokio::process::Command::new(bin)
            .arg("--stdio")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawning {bin}"))?;
        let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
        Ok(Self {
            reader: BufReader::new(Box::new(stdout)),
            writer: Box::new(stdin),
            _child: Some(child),
            next_id: 1,
        })
    }

    async fn send(&mut self, method: &str, params: Value) -> Result<Id> {
        let id = Id::Num(self.next_id);
        self.next_id += 1;
        let mut line = serde_json::to_string(&Request::new(id.clone(), method, params))?;
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await?;
        self.writer.flush().await?;
        Ok(id)
    }

    async fn next(&mut self) -> Result<Option<Message>> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line).await?;
            if n == 0 {
                return Ok(None);
            }
            if line.trim().is_empty() {
                continue;
            }
            return Ok(Some(
                serde_json::from_str(line.trim()).context("bad line from server")?,
            ));
        }
    }

    /// Send a request and wait for its response, handing notifications to `on_notify`.
    async fn call(
        &mut self,
        method: &str,
        params: Value,
        mut on_notify: impl FnMut(&str, &Value),
    ) -> Result<Value> {
        let id = self.send(method, params).await?;
        while let Some(msg) = self.next().await? {
            match msg {
                Message::Notification(n) => on_notify(&n.method, &n.params),
                Message::Response(r) if r.id == id => {
                    if let Some(e) = r.error {
                        return Err(CallError {
                            code: e.code,
                            message: e.message,
                            data: e.data,
                        }
                        .into());
                    }
                    return Ok(r.result.unwrap_or(Value::Null));
                }
                _ => {}
            }
        }
        Err(anyhow!("connection closed before response"))
    }
}

fn read_stdin_prompt() -> Result<String> {
    use std::io::Read;
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s)?;
    let s = s.trim().to_string();
    if s.is_empty() {
        eprintln!("theseus: no prompt given (pass PROMPT or pipe it on stdin)");
        std::process::exit(2);
    }
    Ok(s)
}

#[tokio::main]
async fn main() {
    // Rust ignores SIGPIPE, so a closed pipe (`theseus history | head`) would
    // panic in println!. Restore the default: exit quietly like other tools.
    #[cfg(unix)]
    // SAFETY: called once at startup before any other thread writes to stdout.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    let code = match run(cli).await {
        Ok(()) => 0,
        Err(e) => {
            let msg = format!("{e:#}");
            eprintln!("theseus: {msg}");
            if msg.contains("connecting to theseusd") || msg.contains("spawning") {
                3
            } else {
                1
            }
        }
    };
    std::process::exit(code);
}

async fn run(cli: Cli) -> Result<()> {
    let mut conn = match &cli.spawn {
        Some(bin) => Conn::spawn(bin).await?,
        None => Conn::socket(&cli.socket).await?,
    };
    let json = cli.json;
    let stream = !cli.no_stream && !json;

    match cli.cmd {
        Cmd::Ask {
            prompt,
            session,
            profile,
            provider,
            model,
            trace,
            thinking,
        } => {
            let prompt = match prompt.as_deref() {
                None | Some("-") => read_stdin_prompt()?,
                Some(p) => p.to_string(),
            };
            let mut printer = Printer::new(stream, thinking && !json, false, json);
            let call = conn
                .call(
                    method::TURN_SUBMIT,
                    serde_json::to_value(TurnSubmitParams {
                        session_id: session,
                        input: prompt,
                        profile,
                        provider,
                        model,
                        author: None,
                    })?,
                    |m, p| printer.on(m, p),
                )
                .await;
            printer.settle();
            let result = match call {
                Ok(v) => v,
                Err(err) => {
                    if trace {
                        if let Some(ce) = err.downcast_ref::<CallError>() {
                            if let Ok(t) = serde_json::from_value::<theseus_protocol::Span>(
                                ce.data.get("trace").cloned().unwrap_or(Value::Null),
                            ) {
                                eprintln!(
                                    "--- trace up to the failure ({} total)",
                                    fmt_us(t.duration_us())
                                );
                                print_span(&t, 0, t.duration_us().max(1));
                            }
                        }
                    }
                    return Err(err);
                }
            };
            let r: TurnSubmitResult = serde_json::from_value(result.clone())?;
            if json {
                println!("{}", serde_json::to_string(&result)?);
            } else if stream {
                eprintln!("{}", status_line(&r));
            } else {
                println!("{}", r.output);
                eprintln!("{}", status_line(&r));
            }
            if let (Some(corr), false) = (&r.awaiting_confirm, json) {
                eprintln!(
                    "[parked: waiting for your answer on {corr} · `theseus confirm {corr}` or `--deny`; the turn resumes on its own]"
                );
            }
            if trace && !json {
                if let Some(t) = &r.trace {
                    eprintln!("--- trace ({} total)", fmt_us(t.duration_us()));
                    print_span(t, 0, t.duration_us().max(1));
                }
            }
        }
        Cmd::History { session, n, full } => {
            let sid = resolve_session(&mut conn, session).await?;
            let v = conn
                .call(
                    method::SESSION_HISTORY,
                    serde_json::to_value(SessionHistoryParams { session_id: sid, n })?,
                    |_, _| {},
                )
                .await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                let h: SessionHistoryResult = serde_json::from_value(v)?;
                println!("{}", session_header(&h.session));
                for node in &h.nodes {
                    print_node(node, full);
                }
                for c in &h.pending_confirms {
                    print_confirm(c);
                }
            }
        }
        Cmd::Watch { session, thinking } => {
            let sid = resolve_session(&mut conn, session).await?;
            conn.call(
                method::SESSION_WATCH,
                serde_json::to_value(SessionRef {
                    session_id: sid.clone(),
                })?,
                |_, _| {},
            )
            .await?;
            eprintln!("watching {sid} (Ctrl-C to stop)");
            let mut printer = Printer::new(true, thinking, true, json);
            while let Some(msg) = conn.next().await? {
                if let Message::Notification(n) = msg {
                    if json {
                        println!("{}", serde_json::to_string(&n)?);
                    } else {
                        printer.on(&n.method, &n.params);
                    }
                }
            }
        }
        Cmd::Confirm {
            correlation_id,
            deny,
            note,
            no_wait,
        } => {
            let Some(corr) = correlation_id else {
                // Everything waiting, across sessions.
                let l: SessionListResult = serde_json::from_value(
                    conn.call(method::SESSION_LIST, Value::Null, |_, _| {})
                        .await?,
                )?;
                let mut waiting = Vec::new();
                for s in l
                    .sessions
                    .iter()
                    .filter(|s| s.execution_state.as_deref() == Some("waiting"))
                {
                    let h: SessionHistoryResult = serde_json::from_value(
                        conn.call(
                            method::SESSION_HISTORY,
                            serde_json::to_value(SessionHistoryParams {
                                session_id: s.session_id.clone(),
                                n: Some(1),
                            })?,
                            |_, _| {},
                        )
                        .await?,
                    )?;
                    waiting.extend(h.pending_confirms);
                }
                if json {
                    println!("{}", serde_json::to_string(&waiting)?);
                } else if waiting.is_empty() {
                    println!("nothing is waiting for confirmation");
                } else {
                    for c in &waiting {
                        print_confirm(c);
                    }
                }
                return Ok(());
            };
            let mut printer = Printer::new(true, false, false, json);
            let v = conn
                .call(
                    method::ACTION_CONFIRM,
                    serde_json::to_value(ActionConfirmParams {
                        correlation_id: corr.clone(),
                        approve: !deny,
                        note,
                        watch: !no_wait,
                        author: None,
                    })?,
                    |m, p| printer.on(m, p),
                )
                .await?;
            let r: ActionConfirmResult = serde_json::from_value(v.clone())?;
            if no_wait {
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!(
                        "{} {} · session {}",
                        if r.approved { "approved" } else { "declined" },
                        r.correlation_id,
                        r.session_id
                    );
                }
                return Ok(());
            }
            eprintln!(
                "{} {} · following the resumed turn in {}",
                if r.approved { "approved" } else { "declined" },
                r.correlation_id,
                r.session_id
            );
            // The server subscribed us before waking the execution: read until
            // the continuation turn ends (or fails, or parks on another confirm).
            let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(900);
            loop {
                let msg = match tokio::time::timeout_at(deadline, conn.next()).await {
                    Ok(m) => m?,
                    Err(_) => {
                        printer.settle();
                        eprintln!(
                            "[still running after 15 min · follow it with `theseus watch {}`]",
                            r.session_id
                        );
                        break;
                    }
                };
                let Some(Message::Notification(n)) = msg else {
                    if msg.is_none() {
                        break;
                    }
                    continue;
                };
                printer.on(&n.method, &n.params);
                if n.method == notify::TURN_ENDED {
                    printer.settle();
                    if let Ok(t) = serde_json::from_value::<TurnSubmitResult>(n.params.clone()) {
                        if json {
                            println!("{}", serde_json::to_string(&t)?);
                        } else {
                            eprintln!("{}", status_line(&t));
                            if let Some(c) = &t.awaiting_confirm {
                                eprintln!("[parked again: `theseus confirm {c}` or `--deny`]");
                            }
                        }
                    }
                    break;
                }
                if n.method == notify::TURN_FAILED {
                    printer.settle();
                    anyhow::bail!(
                        "the resumed turn failed: {}",
                        n.params.get("error").and_then(Value::as_str).unwrap_or("?")
                    );
                }
            }
        }
        Cmd::Tools { verbose } => {
            let v = conn.call(method::TOOL_LIST, Value::Null, |_, _| {}).await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                let l: ToolListResult = serde_json::from_value(v)?;
                println!(
                    "{:<12} {:<12} {:<6} {:<7} {:<8} {:>6}",
                    "tool", "wire name", "class", "backend", "policy", "calls"
                );
                for t in &l.tools {
                    println!(
                        "{:<12} {:<12} {:<6} {:<7} {:<8} {:>6}",
                        t.name, t.wire_name, t.class, t.backend, t.policy, t.calls
                    );
                    if verbose {
                        println!("    {}", t.description);
                        println!("    input: {}", serde_json::to_string(&t.input_schema)?);
                    }
                }
                eprintln!(
                    "[roots: {} · {} call(s) · shell-fallback ratio {:.0}% (proc.run / all calls)]",
                    l.roots.join(", "),
                    l.calls_total,
                    l.shell_fallback_ratio * 100.0
                );
            }
        }
        Cmd::Catalog => {
            let v = conn
                .call(method::CATALOG_LIST, Value::Null, |_, _| {})
                .await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                let l: CatalogListResult = serde_json::from_value(v)?;
                println!(
                    "{:<24} {:<10} {:>9} {:>8} {:>7} {:>7} {:>7} {:>7}  {:<9} profiles",
                    "model",
                    "provider",
                    "window",
                    "max out",
                    "$in",
                    "$out",
                    "$c.rd",
                    "$c.wr",
                    "thinking"
                );
                for m in &l.models {
                    let e = &m.entry;
                    let num = |k: &str| e.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                    println!(
                        "{:<24} {:<10} {:>9} {:>8} {:>7} {:>7} {:>7} {:>7}  {:<9} {}",
                        m.model,
                        e.get("provider").and_then(Value::as_str).unwrap_or("?"),
                        fmt_tokens(num("context_window") as u64),
                        fmt_tokens(num("max_output_tokens") as u64),
                        fmt_price(num("input_per_mtok")),
                        fmt_price(num("output_per_mtok")),
                        fmt_price(num("cache_read_per_mtok")),
                        fmt_price(num("cache_write_per_mtok")),
                        e.get("thinking").and_then(Value::as_str).unwrap_or("?"),
                        m.profiles.join(",")
                    );
                }
                eprintln!(
                    "[catalog {} · prices are USD per million tokens]",
                    l.version
                );
            }
        }
        Cmd::Health => {
            let v = conn.call(method::HEALTH, Value::Null, |_, _| {}).await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                let h: HealthResult = serde_json::from_value(v)?;
                println!(
                    "{} {} · protocol {} · up {}s · live profile {} ({}/{}) · providers [{}] · sessions {} · turns {} · provider errors {} · ledger rows {}",
                    h.name, h.version, h.protocol, h.uptime_secs, h.profile, h.provider, h.model, h.providers.join(", "), h.sessions, h.turns, h.provider_errors, h.ledger_rows
                );
                println!(
                    "telemetry: {}",
                    match &h.telemetry.otlp_endpoint {
                        Some(e) => format!("OTLP/HTTP → {e}"),
                        None => "off (no [telemetry].otlp_endpoint)".to_string(),
                    }
                );
                let k = &h.kernel;
                let fmt_counts = |m: &std::collections::BTreeMap<String, u64>| {
                    if m.is_empty() {
                        "none".to_string()
                    } else {
                        m.iter()
                            .map(|(s, n)| format!("{n} {s}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                };
                println!(
                    "kernel: {} · turns held {}/{} · executions [{}] · actions [{}] · quarantined completions {}",
                    if k.accepting { "accepting" } else { "starting" },
                    k.turns_held,
                    k.admission_ceiling,
                    fmt_counts(&k.executions_by_state),
                    fmt_counts(&k.actions_by_state),
                    k.quarantined_completions
                );
                println!(
                    "tokens total: in {} out {} cache-read {} cache-write {} · secrets [{}]",
                    h.usage_total.input_tokens,
                    h.usage_total.output_tokens,
                    h.usage_total.cache_read_input_tokens,
                    h.usage_total.cache_creation_input_tokens,
                    h.secrets_resolved.join(", ")
                );
                for b in &h.bindings {
                    let places: Vec<String> = b
                        .places
                        .iter()
                        .map(|p| {
                            format!(
                                "{} → {}",
                                p.label,
                                p.session_id.as_deref().unwrap_or("no session")
                            )
                        })
                        .collect();
                    println!(
                        "{}: {}{} · {} in · {} sent · {} edits · {} presses · {} ignored · {} errors{}",
                        b.kind,
                        b.state,
                        b.detail
                            .as_deref()
                            .map(|d| format!(" ({d})"))
                            .unwrap_or_default(),
                        b.messages_in,
                        b.messages_out,
                        b.edits,
                        b.interactions,
                        b.ignored,
                        b.errors,
                        if places.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", places.join(", "))
                        }
                    );
                }
            }
        }
        Cmd::Sessions { cmd } => match cmd.unwrap_or(SessionsCmd::List) {
            SessionsCmd::List => {
                let v = conn
                    .call(method::SESSION_LIST, Value::Null, |_, _| {})
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    let l: SessionListResult = serde_json::from_value(v)?;
                    for s in l.sessions {
                        println!(
                            "{}\t{}\tturns={}\ttools={}\t${:.4}\tin={}\tout={}\t{}\t{}\t{}",
                            s.session_id,
                            fmt_time(s.last_active_ms.max(s.created_at_unix_ms)),
                            s.turns,
                            s.tool_calls,
                            s.cost_usd,
                            s.usage.input_tokens,
                            s.usage.output_tokens,
                            s.execution_state.clone().unwrap_or_else(|| "-".into()),
                            s.model.clone().unwrap_or_else(|| "-".into()),
                            s.label.clone().or(s.title.clone()).unwrap_or_default()
                        );
                    }
                }
            }
            SessionsCmd::Open { label } => {
                let v = conn
                    .call(
                        method::SESSION_OPEN,
                        serde_json::to_value(SessionOpenParams { kind: None, label })?,
                        |_, _| {},
                    )
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!(
                        "{}",
                        v.get("session_id").and_then(Value::as_str).unwrap_or("")
                    );
                }
            }
            SessionsCmd::Recompile { session, strategy } => {
                let v = conn
                    .call(
                        method::SESSION_RECOMPILE,
                        serde_json::to_value(SessionRecompileParams {
                            session_id: session.clone(),
                            strategy: strategy.clone(),
                        })?,
                        |_, _| {},
                    )
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!("{session}: the next turn recompiles ({strategy})");
                }
            }
        },
        Cmd::Hooks { cmd } => match cmd {
            HooksCmd::List => {
                let v = conn
                    .call(method::HOOKS_LIST, Value::Null, |_, _| {})
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    let l: HooksListResult = serde_json::from_value(v)?;
                    for e in l.events {
                        println!("{:<24}{:<12}{}", e.event, e.kind, e.handlers);
                    }
                    if !l.handlers.is_empty() {
                        println!("--- handlers");
                        for h in l.handlers {
                            println!("{:<24}{:<20}{}", h.event, h.handler_id, h.client);
                        }
                    }
                }
            }
            HooksCmd::Watch { event, handler_id } => {
                let v = conn
                    .call(
                        method::HOOKS_REGISTER,
                        serde_json::to_value(HooksRegisterParams {
                            event: event.clone(),
                            handler_id,
                        })?,
                        |_, _| {},
                    )
                    .await?;
                eprintln!(
                    "watching {} ({})",
                    event,
                    v.get("kind").and_then(Value::as_str).unwrap_or("?")
                );
                while let Some(msg) = conn.next().await? {
                    if let Message::Notification(n) = msg {
                        if n.method == notify::HOOK_EVENT {
                            println!("{}", serde_json::to_string(&n.params)?);
                        }
                    }
                }
            }
        },
        Cmd::Executions { cmd } => match cmd.unwrap_or(ExecutionsCmd::List) {
            ExecutionsCmd::List => {
                let v = conn
                    .call(method::EXECUTION_LIST, Value::Null, |_, _| {})
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    let l: theseus_protocol::ExecutionListResult = serde_json::from_value(v)?;
                    if l.executions.is_empty() {
                        println!("no executions");
                    }
                    for e in l.executions {
                        println!(
                            "{}\t{}\t{}\tturns={}\tinterrupted={}\toutstanding={}\tqueued={}\tbudget spent {}/{} (reserved {}, held {})\tsession={}{}",
                            e.execution_id,
                            e.kind,
                            e.state,
                            e.turns,
                            e.interrupted,
                            e.outstanding,
                            e.queued_results,
                            e.budget.spent,
                            e.budget.limit,
                            e.budget.reserved,
                            e.budget.held_unknown,
                            e.session_id,
                            e.ended_reason.map(|r| format!("\t{r}")).unwrap_or_default()
                        );
                    }
                }
            }
            ExecutionsCmd::Cancel { execution_id } => {
                let v = conn
                    .call(
                        method::EXECUTION_CANCEL,
                        serde_json::to_value(theseus_protocol::ExecutionCancelParams {
                            execution_id,
                            author: None,
                        })?,
                        |_, _| {},
                    )
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    let r: theseus_protocol::ExecutionCancelResult = serde_json::from_value(v)?;
                    println!(
                        "{} {} · {} action(s) asked to stop{}",
                        r.execution.execution_id,
                        r.execution.state,
                        r.cancelled_actions.len(),
                        r.execution
                            .ended_reason
                            .map(|x| format!(" · {x}"))
                            .unwrap_or_default()
                    );
                }
            }
        },
        Cmd::Profile { cmd } => match cmd.unwrap_or(ProfileCmd::List) {
            ProfileCmd::List => {
                let v = conn
                    .call(method::PROFILE_LIST, Value::Null, |_, _| {})
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    let l: ProfileListResult = serde_json::from_value(v)?;
                    for p in l.profiles {
                        println!(
                            "{} {:<12} {:<10} {:<28} max_output_tokens={}{}",
                            if p.live { "*" } else { " " },
                            p.name,
                            p.provider,
                            p.model,
                            p.max_output_tokens,
                            if p.has_system { "  +system" } else { "" }
                        );
                    }
                    eprintln!("[live: {} (from {})]", l.live, l.live_source);
                }
            }
            ProfileCmd::Use { name } => {
                let v = conn
                    .call(
                        method::PROFILE_USE,
                        serde_json::to_value(ProfileUseParams { name })?,
                        |_, _| {},
                    )
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!(
                        "live profile: {} (was {})",
                        v.get("live").and_then(Value::as_str).unwrap_or("?"),
                        v.get("previous").and_then(Value::as_str).unwrap_or("?")
                    );
                }
            }
        },
        Cmd::Ledger { n, kind, session } => {
            let v = conn
                .call(
                    method::LEDGER_TAIL,
                    serde_json::to_value(LedgerTailParams {
                        n: Some(n),
                        kind,
                        session_id: session,
                    })?,
                    |_, _| {},
                )
                .await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                let t: LedgerTailResult = serde_json::from_value(v)?;
                for r in t.rows {
                    let data = serde_json::to_string(&r.data)?;
                    let data: String = data.chars().take(160).collect();
                    println!(
                        "{:>6}  {}  {:<16} {:<36} {}",
                        r.position,
                        fmt_time(r.at_unix_ms),
                        r.kind,
                        r.turn_id.unwrap_or_default(),
                        data
                    );
                }
                eprintln!("[{} rows total]", t.total);
            }
        }
        Cmd::Rpc { method, params } => {
            let params: Value = match params {
                Some(p) => serde_json::from_str(&p).context("PARAMS must be JSON")?,
                None => Value::Null,
            };
            let v = conn
                .call(&method, params, |m, p| {
                    if !json {
                        eprintln!("<- {m} {p}");
                    }
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Cmd::Shutdown => {
            let v = conn.call(method::SHUTDOWN, Value::Null, |_, _| {}).await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            }
        }
    }
    Ok(())
}

/// hh:mm:ss.mmm in local time, without pulling in a date crate.
fn fmt_time(unix_ms: u64) -> String {
    let secs = unix_ms / 1000;
    let ms = unix_ms % 1000;
    let s = secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        s / 3600,
        (s / 60) % 60,
        s % 60,
        ms
    )
}

fn fmt_us(us: u64) -> String {
    if us >= 1_000_000 {
        format!("{:.2} s", us as f64 / 1e6)
    } else if us >= 1000 {
        format!("{:.1} ms", us as f64 / 1e3)
    } else {
        format!("{us} µs")
    }
}

/// Indented tree with a 24-column bar: where in the turn each span sat.
fn print_span(s: &theseus_protocol::Span, depth: usize, total_us: u64) {
    let width = 24usize;
    let a = ((s.start_us as f64 / total_us as f64) * width as f64).floor() as usize;
    let b =
        ((s.end_us.unwrap_or(s.start_us) as f64 / total_us as f64) * width as f64).ceil() as usize;
    let (a, b) = (a.min(width), b.clamp(a.min(width), width));
    let mut bar = String::new();
    for i in 0..width {
        bar.push(if i >= a && (i < b || (i == a && a == b)) {
            '█'
        } else {
            '·'
        });
    }
    let dur = if s.end_us == Some(s.start_us) {
        format!("@{}", fmt_us(s.start_us))
    } else {
        fmt_us(s.duration_us())
    };
    let attrs = match &s.attrs {
        serde_json::Value::Null => String::new(),
        v => {
            let t = serde_json::to_string(v).unwrap_or_default();
            let t: String = t.chars().take(90).collect();
            format!("  {t}")
        }
    };
    eprintln!(
        "{bar} {:>9}  {}{} [{}]{}",
        dur,
        "  ".repeat(depth),
        s.name,
        s.kind,
        attrs
    );
    for c in &s.children {
        print_span(c, depth + 1, total_us);
    }
}

fn status_line(r: &TurnSubmitResult) -> String {
    let cache = if r.usage.cache_read_input_tokens + r.usage.cache_creation_input_tokens > 0 {
        format!(
            " cache r{} w{}",
            r.usage.cache_read_input_tokens, r.usage.cache_creation_input_tokens
        )
    } else {
        String::new()
    };
    format!(
        "[{} → {}/{} · {} loop(s){}{} · {} · tokens in {} out {}{}{} · {} ms{} · session {}]",
        r.profile,
        r.provider,
        r.model,
        r.loops,
        if r.tool_calls > 0 {
            format!(" · {} tool call(s)", r.tool_calls)
        } else {
            String::new()
        },
        if r.continuation {
            " · continuation"
        } else {
            ""
        },
        r.stop_reason,
        r.usage.input_tokens,
        r.usage.output_tokens,
        cache,
        r.cost_usd
            .map(|c| format!(" · ${c:.4}"))
            .unwrap_or_default(),
        r.elapsed_ms,
        r.first_token_ms
            .map(|t| format!(" (first token {t} ms)"))
            .unwrap_or_default(),
        r.session_id
    )
}

/// The session a command means: the one named, or the most recently active.
async fn resolve_session(conn: &mut Conn, session: Option<String>) -> Result<String> {
    if let Some(s) = session {
        return Ok(s);
    }
    let l: SessionListResult = serde_json::from_value(
        conn.call(method::SESSION_LIST, Value::Null, |_, _| {})
            .await?,
    )?;
    l.sessions
        .first()
        .map(|s| s.session_id.clone())
        .ok_or_else(|| anyhow!("there are no sessions yet"))
}

fn session_header(s: &SessionInfo) -> String {
    format!(
        "── {} · {} · {} turn(s) · {} tool call(s) · ${:.4} · {}{}",
        s.session_id,
        s.title
            .as_deref()
            .or(s.label.as_deref())
            .map(|t| format!("\"{t}\""))
            .unwrap_or_else(|| "(untitled)".into()),
        s.turns,
        s.tool_calls,
        s.cost_usd,
        s.model.as_deref().unwrap_or("-"),
        s.execution_state
            .as_deref()
            .map(|e| format!(" · {e}"))
            .unwrap_or_default()
    )
}

fn clip(s: &str, max: usize) -> String {
    let one = s.replace('\n', " ⏎ ");
    if one.chars().count() <= max {
        one
    } else {
        format!("{}…", one.chars().take(max).collect::<String>())
    }
}

fn indent(s: &str, pad: &str) -> String {
    s.lines()
        .map(|l| format!("{pad}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn print_node(n: &NodeInfo, full: bool) {
    let t = fmt_time(n.at_unix_ms);
    let d = &n.detail;
    let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("");
    match n.kind.as_str() {
        "user_message" => {
            let who = match n.author.as_deref() {
                Some(a) => format!("operator ({a})"),
                None => "operator".to_string(),
            };
            if full {
                println!("[{t}] {who}:\n{}", indent(&n.text, "    "));
            } else {
                println!("[{t}] {who}: {}", clip(&n.text, 300));
            }
        }
        "assistant_message" => {
            let cost = d
                .get("cost_usd")
                .and_then(Value::as_f64)
                .map(|c| format!(" · ${c:.4}"))
                .unwrap_or_default();
            let calls = d
                .get("tool_calls")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            let body = if n.text.is_empty() {
                String::new()
            } else if full {
                format!("\n{}", indent(&n.text, "    "))
            } else {
                format!(" {}", clip(&n.text, 300))
            };
            println!(
                "[{t}] {}:{body}{}",
                s("model"),
                if calls > 0 && n.text.is_empty() {
                    format!(" ({calls} tool call(s))")
                } else {
                    String::new()
                }
            );
            if !n.thinking.is_empty() && full {
                println!("      (thinking) {}", clip(&n.thinking, 600));
            }
            println!(
                "      ↳ {} · in {} out {}{cost}",
                s("stop_reason"),
                d.pointer("/usage/input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                d.pointer("/usage/output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            );
        }
        "tool_call" => {
            let input = d.get("input").map(|v| v.to_string()).unwrap_or_default();
            // The policy's verdict with its reason; a call that failed
            // validation never reached policy, so fall back to the gate result.
            let gate = match d.get("decision") {
                Some(Value::Object(o)) => format!(
                    "{}: {}",
                    o.get("mode").and_then(Value::as_str).unwrap_or("?"),
                    clip(o.get("reason").and_then(Value::as_str).unwrap_or(""), 90)
                ),
                _ => d
                    .pointer("/result/gate")
                    .and_then(Value::as_str)
                    .unwrap_or("-")
                    .to_string(),
            };
            println!(
                "      ⚙ {} {} [{}]",
                s("tool"),
                if full { input } else { clip(&input, 160) },
                gate
            );
        }
        "tool_result" => {
            let late = if d.get("late").and_then(Value::as_bool) == Some(true) {
                " (late)"
            } else {
                ""
            };
            let ms = format!(
                "{}{}",
                d.pointer("/meta/exit_code")
                    .and_then(Value::as_i64)
                    .map(|c| format!(" · exit {c}"))
                    .unwrap_or_default(),
                d.get("duration_ms")
                    .and_then(Value::as_u64)
                    .map(|m| format!(" · {m} ms"))
                    .unwrap_or_default()
            );
            if full {
                println!(
                    "      ← {} {}{late}{ms} · {}\n{}",
                    s("tool"),
                    s("status"),
                    fmt_bytes(n.bytes),
                    indent(&n.text, "        ")
                );
            } else {
                println!(
                    "      ← {} {}{late}{ms} · {}: {}",
                    s("tool"),
                    s("status"),
                    fmt_bytes(n.bytes),
                    clip(&n.text, 160)
                );
            }
        }
        other => println!("[{t}] {other}"),
    }
}

fn print_confirm(c: &ConfirmRequest) {
    println!(
        "  ? {} waits for you in {}: {}\n      input: {}\n      approve: theseus confirm {}\n      decline: theseus confirm --deny {}",
        c.tool,
        c.session_id,
        c.reason,
        clip(&c.input.to_string(), 200),
        c.correlation_id,
        c.correlation_id
    );
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 20 {
        format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
    } else if b >= 1024 {
        format!("{:.1} KB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 && n.is_multiple_of(1_000_000) {
        format!("{}M", n / 1_000_000)
    } else if n >= 1000 {
        format!("{}K", n / 1000)
    } else {
        n.to_string()
    }
}

fn fmt_price(p: f64) -> String {
    if p == 0.0 {
        "-".into()
    } else {
        format!("{p:.3}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

/// Renders live session events for a terminal: the model's text on stdout,
/// everything else (tools, confirmations, context decisions) on stderr.
struct Printer {
    text: bool,
    thinking: bool,
    verbose: bool,
    quiet: bool,
    stdout_mid_line: bool,
    thinking_open: bool,
}

impl Printer {
    fn new(text: bool, thinking: bool, verbose: bool, quiet: bool) -> Self {
        Self {
            text,
            thinking,
            verbose,
            quiet,
            stdout_mid_line: false,
            thinking_open: false,
        }
    }

    /// Finish a partial stdout line or thinking run before an event line.
    fn settle(&mut self) {
        use std::io::Write;
        if self.thinking_open {
            eprintln!();
            self.thinking_open = false;
        }
        if self.stdout_mid_line {
            println!();
            let _ = std::io::stdout().flush();
            self.stdout_mid_line = false;
        }
    }

    fn on(&mut self, m: &str, p: &Value) {
        use std::io::Write;
        if self.quiet {
            return;
        }
        let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let u = |k: &str| p.get(k).and_then(Value::as_u64);
        match m {
            notify::MODEL_DELTA if self.text => {
                if let Some(t) = p.get("text").and_then(Value::as_str) {
                    if self.thinking_open {
                        eprintln!();
                        self.thinking_open = false;
                    }
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(t.as_bytes());
                    let _ = out.flush();
                    if !t.is_empty() {
                        self.stdout_mid_line = !t.ends_with('\n');
                    }
                }
            }
            notify::MODEL_THINKING if self.thinking => {
                if let Some(t) = p.get("text").and_then(Value::as_str) {
                    if !self.thinking_open {
                        self.settle();
                        eprint!("  (thinking) ");
                        self.thinking_open = true;
                    }
                    eprint!("{}", t.replace('\n', "\n             "));
                }
            }
            notify::TOOL_STARTED => {
                self.settle();
                let argv = p
                    .get("argv")
                    .and_then(Value::as_array)
                    .map(|a| {
                        format!(
                            " [{}]{}",
                            a.iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(" "),
                            u("pid").map(|p| format!(" pid {p}")).unwrap_or_default()
                        )
                    })
                    .unwrap_or_default();
                eprintln!("  → {}{argv}", s("tool"));
            }
            notify::TOOL_ENDED => {
                self.settle();
                let mut extra = Vec::new();
                if let Some(c) = p.get("exit_code").and_then(Value::as_i64) {
                    extra.push(format!("exit {c}"));
                }
                if let Some(ms) = u("duration_ms") {
                    extra.push(format!("{ms} ms"));
                }
                if let Some(b) = u("bytes") {
                    extra.push(fmt_bytes(b));
                }
                if p.get("truncated").and_then(Value::as_bool) == Some(true) {
                    extra.push("truncated".into());
                }
                if p.get("late").and_then(Value::as_bool) == Some(true) {
                    extra.push("late".into());
                }
                eprintln!(
                    "  ← {} {}{}",
                    s("tool"),
                    s("status"),
                    if extra.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", extra.join(" · "))
                    }
                );
            }
            notify::CONFIRM_REQUESTED => {
                self.settle();
                if let Ok(c) = serde_json::from_value::<ConfirmRequest>(p.clone()) {
                    eprintln!(
                        "  ? {} needs your confirmation{}: {}\n      input: {}\n      approve: theseus confirm {}\n      decline: theseus confirm --deny {}",
                        c.tool,
                        if c.against_policy { " (AGAINST POLICY)" } else { "" },
                        c.reason,
                        clip(&c.input.to_string(), 200),
                        c.correlation_id,
                        c.correlation_id
                    );
                }
            }
            notify::POLICY_NOTIFIED => {
                self.settle();
                let off = p.get("kind").and_then(Value::as_str) == Some("off_policy");
                eprintln!(
                    "  {} {} {}: {}\n      the policy said: {}\n      ({})",
                    if off { "!!" } else { "!" },
                    if off {
                        "ran against policy"
                    } else {
                        "ran without approval"
                    },
                    p.get("tool").and_then(Value::as_str).unwrap_or("?"),
                    p.get("summary").and_then(Value::as_str).unwrap_or(""),
                    p.get("rule").and_then(Value::as_str).unwrap_or(""),
                    p.get("setting").and_then(Value::as_str).unwrap_or("")
                );
            }
            notify::CONFIRM_RESOLVED => {
                self.settle();
                let ok = p.get("approved").and_then(Value::as_bool).unwrap_or(false);
                if p.get("superseded").and_then(Value::as_bool) == Some(true) {
                    eprintln!(
                        "  ✗ superseded {} (a new message arrived before an answer)",
                        s("correlation_id")
                    );
                } else {
                    eprintln!(
                        "  {} {} (by {})",
                        if ok { "✓ approved" } else { "✗ declined" },
                        s("correlation_id"),
                        s("by")
                    );
                }
            }
            notify::CONTEXT_COMPILED => {
                let recompiled = s("decision") == "recompile";
                if recompiled || self.verbose {
                    self.settle();
                    eprintln!(
                        "  ⟳ context {}{} · {} message(s) · ~{} tokens{}",
                        s("decision"),
                        p.get("trigger")
                            .and_then(Value::as_str)
                            .map(|t| format!(" ({t}, {})", s("strategy")))
                            .unwrap_or_default(),
                        u("messages").unwrap_or(0),
                        u("est_tokens").unwrap_or(0),
                        p.get("repairs")
                            .and_then(Value::as_array)
                            .filter(|a| !a.is_empty())
                            .map(|a| format!(" · {} repaired", a.len()))
                            .unwrap_or_default()
                    );
                }
            }
            notify::TURN_STARTED if self.verbose => {
                self.settle();
                eprintln!(
                    "── turn {}{}",
                    s("turn_id"),
                    if p.get("continuation").and_then(Value::as_bool) == Some(true) {
                        " (continuation)"
                    } else {
                        ""
                    }
                );
            }
            notify::TURN_ENDED if self.verbose => {
                self.settle();
                if let Ok(r) = serde_json::from_value::<TurnSubmitResult>(p.clone()) {
                    eprintln!("{}", status_line(&r));
                }
            }
            notify::TURN_FAILED => {
                self.settle();
                eprintln!(
                    "  ✗ turn failed{}: {}",
                    p.get("class")
                        .and_then(Value::as_str)
                        .map(|c| format!(" ({c})"))
                        .unwrap_or_default(),
                    s("error")
                );
            }
            _ => {}
        }
    }
}
