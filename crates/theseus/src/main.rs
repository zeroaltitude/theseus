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
    method, notify, ActionConfirmParams, ActionConfirmResult, CatalogListResult, ConfirmListResult,
    ConfirmRequest, HealthResult, Id, LedgerTailParams, LedgerTailResult, Message, NodeInfo,
    ProfileListResult, ProfileUseParams, Request, SessionHistoryParams, SessionHistoryResult,
    SessionInfo, SessionListResult, SessionOpenParams, SessionRecompileParams, SessionRef,
    ToolListResult, TurnSubmitParams, TurnSubmitResult,
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
  theseus ask --attach notes.txt \"...\"      send a file with the prompt, as a Discord attachment is sent
  theseus history [session]                  a session's transcript: messages, tool calls, results
  theseus watch [session]                    follow a session live (turns started anywhere)
  theseus confirm [id] [--decline]           answer a tool call or a budget question waiting for you (no id: list them)
  theseus tools                              the toollets, their policy, and calls so far
  theseus policy tighten proc.run            should have asked: proc.run asks first from now on (untighten: undo)
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
        /// After the reply, print the turn's timing tree (turn > loops > provider/tools) to stderr.
        #[arg(long)]
        trace: bool,
        /// Show the model's thinking summaries on stderr as they stream.
        #[arg(long)]
        thinking: bool,
        /// Send a file with the prompt (repeatable), as a Discord attachment is sent: a text
        /// file's text, labeled with its name, or an image (PNG, JPEG, GIF, WebP, up to 5 MiB);
        /// anything else is listed with the reason.
        #[arg(long = "attach", value_name = "FILE")]
        attach: Vec<PathBuf>,
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
    /// Answer a tool call waiting for your confirmation, or a session at its spend limit
    /// (approve resets its spend to $0), then follow the turn it resumes.
    /// Without an id, list everything waiting.
    Confirm {
        correlation_id: Option<String>,
        /// Approve, which is what an answer does unless --decline says otherwise.
        #[arg(long, conflicts_with = "decline")]
        approve: bool,
        /// Decline instead of approve (the model is told, and carries on without it; a
        /// session at its limit keeps waiting, and its next message asks again).
        #[arg(long, alias = "deny")]
        decline: bool,
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
    /// "Should have asked": make a tool ask first from now on, undo it, or list every tool's
    /// posture and what set it. A tightening is stored, never in the config, and only makes a
    /// tool stricter; an undo returns the tool to what the config says.
    Policy {
        #[command(subcommand)]
        cmd: Option<PolicyCmd>,
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
    /// Recent ledger rows (every turn, loop, provider call, state change, error).
    Ledger {
        /// How many rows.
        #[arg(short, long, default_value_t = 20)]
        n: usize,
        /// Only rows of this kind, e.g. turn.ended, provider.call, provider.error.
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
enum PolicyCmd {
    /// Every tool's posture now and what set it, with the tightenings (default).
    List,
    /// TOOL asks first from now on, on every surface (e.g. `theseus policy tighten proc.run`).
    Tighten {
        tool: String,
        /// The call whose notice prompted it (a correlation id from `theseus ledger -k
        /// tool.notified`), kept with the tightening as a labeled example.
        #[arg(long = "call", value_name = "CORRELATION_ID")]
        call: Option<String>,
    },
    /// Undo a tightening: TOOL goes back to what the config says. It loosens, so it counts only
    /// where an approval would.
    Untighten { tool: String },
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
            // `config_unconfirmed` (theseus-2fo) names its class alone.
            (Some(c), None) => write!(f, "{} [class={c}, code {}]", self.message, self.code),
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

    fn spawn(bin: &str) -> Result<Self> {
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

/// The largest file `ask --attach` sends; the daemon caps text further, at
/// `[tools].max_read_bytes`.
const MAX_ATTACH_BYTES: u64 = 16 * 1024 * 1024;

/// One `--attach` file as `turn.submit` carries it (theseus-9g2): a text
/// file's text, or the file listed with the reason it was not read. A file
/// that cannot be opened stops the command, before anything is sent.
fn attachment_for(path: &std::path::Path) -> Result<theseus_protocol::Attachment> {
    let meta = std::fs::metadata(path).with_context(|| format!("--attach {}", path.display()))?;
    if meta.is_dir() {
        return Err(anyhow!("--attach {}: is a directory", path.display()));
    }
    let mut a = theseus_protocol::Attachment {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
        size: meta.len(),
        ..Default::default()
    };
    if meta.len() > MAX_ATTACH_BYTES {
        a.not_read = Some("over the 16 MiB limit for --attach".into());
        return Ok(a);
    }
    let bytes = std::fs::read(path).with_context(|| format!("--attach {}", path.display()))?;
    let bytes = match String::from_utf8(bytes) {
        Ok(text) if !text.as_bytes().iter().take(8192).any(|b| *b == 0) => {
            a.media_type = "text/plain".into();
            a.text = Some(text);
            return Ok(a);
        }
        Ok(text) => text.into_bytes(),
        Err(e) => e.into_bytes(),
    };
    // Not text: its bytes, when an image could be this small. The daemon
    // reads the type from the bytes and keeps only an image.
    if bytes.len() as u64 <= theseus_protocol::MAX_IMAGE_BYTES {
        use base64::Engine as _;
        a.data = Some(base64::engine::general_purpose::STANDARD.encode(&bytes));
    } else {
        a.media_type = "application/octet-stream".into();
        a.not_read = Some("not a text file, and over the 5 MiB limit for images".into());
    }
    Ok(a)
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
        Some(bin) => Conn::spawn(bin)?,
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
            attach,
        } => {
            let attachments = attach
                .iter()
                .map(|p| attachment_for(p))
                .collect::<Result<Vec<_>>>()?;
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
                        attachments,
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
                                print_span(&t, 0, &Frame::turn(&t));
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
                    "[parked: waiting for your answer on {corr} · `theseus confirm {corr}` or `--decline`; the turn resumes on its own]"
                );
            }
            if trace && !json {
                if let Some(t) = &r.trace {
                    eprintln!("--- trace ({} total)", fmt_us(t.duration_us()));
                    print_span(t, 0, &Frame::turn(t));
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
            approve: _,
            decline,
            note,
            no_wait,
        } => {
            let Some(corr) = correlation_id else {
                // Everything waiting, across sessions.
                let waiting = serde_json::from_value::<ConfirmListResult>(
                    conn.call(method::CONFIRM_LIST, Value::Null, |_, _| {})
                        .await?,
                )?
                .confirms;
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
                        approve: !decline,
                        note,
                        watch: !no_wait,
                        author: None,
                        discord: None,
                    })?,
                    |m, p| printer.on(m, p),
                )
                .await?;
            let r: ActionConfirmResult = serde_json::from_value(v.clone())?;
            if !r.resumes {
                // A declined budget question: nothing resumes until a new message.
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!(
                        "declined {} · session {} keeps waiting on its budget; a new message asks again",
                        r.correlation_id, r.session_id
                    );
                }
                return Ok(());
            }
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
                    if let Ok(t) = serde_json::from_value::<TurnSubmitResult>(n.params) {
                        if json {
                            println!("{}", serde_json::to_string(&t)?);
                        } else {
                            eprintln!("{}", status_line(&t));
                            if let Some(c) = &t.awaiting_confirm {
                                eprintln!("[parked again: `theseus confirm {c}` or `--decline`]");
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
                    "tool", "wire name", "class", "backend", "posture", "calls"
                );
                for t in &l.tools {
                    // `*`: a "should have asked" press set it (theseus policy list).
                    let posture = match &t.tightened {
                        Some(_) if t.policy != t.config_posture => format!("{}*", t.policy),
                        _ => t.policy.clone(),
                    };
                    println!(
                        "{:<12} {:<12} {:<6} {:<7} {:<8} {:>6}",
                        t.name, t.wire_name, t.class, t.backend, posture, t.calls
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
        Cmd::Policy { cmd } => match cmd.unwrap_or(PolicyCmd::List) {
            PolicyCmd::List => {
                let tools = conn.call(method::TOOL_LIST, Value::Null, |_, _| {}).await?;
                let health = conn.call(method::HEALTH, Value::Null, |_, _| {}).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"tools": tools["tools"], "tightenings": health["tightenings"]})
                    );
                } else {
                    let l: ToolListResult = serde_json::from_value(tools)?;
                    let h: HealthResult = serde_json::from_value(health)?;
                    print!("{}", policy_list(&l, &h.tightenings));
                }
            }
            PolicyCmd::Tighten { tool, call } => {
                let v = conn
                    .call(
                        method::POLICY_TIGHTEN,
                        serde_json::to_value(theseus_protocol::PolicyTightenParams {
                            tool,
                            correlation_id: call,
                            author: None,
                            discord: None,
                        })?,
                        |_, _| {},
                    )
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!("{}", tightened_line(&serde_json::from_value(v)?, true));
                }
            }
            PolicyCmd::Untighten { tool } => {
                let v = conn
                    .call(
                        method::POLICY_UNTIGHTEN,
                        serde_json::to_value(theseus_protocol::PolicyUntightenParams {
                            tool,
                            author: None,
                            discord: None,
                        })?,
                        |_, _| {},
                    )
                    .await?;
                if json {
                    println!("{}", serde_json::to_string(&v)?);
                } else {
                    println!("{}", tightened_line(&serde_json::from_value(v)?, false));
                }
            }
        },
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
                    h.telemetry.summary(theseus_protocol::now_unix_ms())
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
                    "kernel: {} · turns held {}/{} · executions [{}] · actions [{}] · quarantined completions {}{}",
                    if k.accepting { "accepting" } else { "starting" },
                    k.turns_held,
                    k.admission_ceiling,
                    fmt_counts(&k.executions_by_state),
                    fmt_counts(&k.actions_by_state),
                    k.quarantined_completions,
                    lingering_note(k.lingering_wrappers)
                );
                println!(
                    "tokens total: in {} out {} cache-read {} cache-write {}",
                    h.usage_total.input_tokens,
                    h.usage_total.output_tokens,
                    h.usage_total.cache_read_input_tokens,
                    h.usage_total.cache_creation_input_tokens,
                );
                if let Some(line) = config_line(&h.config) {
                    println!("{line}");
                }
                println!("{}", secrets_line(&h.secrets, &h.secrets_resolved));
                if let Some(line) = startup_line(&h.startup) {
                    println!("{line}");
                }
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
                print_approval(&h.approval);
                if !h.tightenings.is_empty() {
                    let t: Vec<String> = h
                        .tightenings
                        .iter()
                        .map(|t| format!("{} (by {}, {})", t.tool, t.by, fmt_time(t.at_ms)))
                        .collect();
                    println!(
                        "tightened (should have asked): {} · undo: theseus policy untighten <tool>",
                        t.join(", ")
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
                            "{}\t{}\t{}\tturns={}\tinterrupted={}\toutstanding={}\tqueued={}\t{}\tsession={}{}",
                            e.execution_id,
                            e.kind,
                            e.state,
                            e.turns,
                            e.interrupted,
                            e.outstanding,
                            e.queued_results,
                            budget_line(&e.budget),
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

/// `secrets: resolving | ready | failed <names>`, with the names ready, how
/// the vault was read and how long it took, and each failure's reason
/// (theseus-qa0). A daemon older than that reports only the ready names.
fn secrets_line(s: &theseus_protocol::SecretsStatus, ready: &[String]) -> String {
    if s.state.is_empty() {
        return format!("secrets [{}]", ready.join(", "));
    }
    let mut line = format!("secrets: {}", s.summary());
    match s.state.as_str() {
        "resolving" => line.push_str(&format!(
            " · {} of {} ready so far",
            s.ready.len(),
            s.ready.len() + s.resolving.len() + s.failed.len()
        )),
        _ => {
            if let Some(ms) = s.settled_ms {
                line.push_str(&format!(" · {} ready {ms} ms after start", s.ready.len()));
            }
            if let Some(m) = &s.method {
                line.push_str(&format!(" ({m})"));
            }
        }
    }
    if !s.ready.is_empty() {
        line.push_str(&format!(" [{}]", s.ready.join(", ")));
    }
    for f in &s.failed {
        line.push_str(&format!("\n  {} did not resolve: {}", f.name, f.error));
    }
    if let Some(ms) = s.retry_in_ms.filter(|_| !s.failed.is_empty()) {
        line.push_str(&format!("\n  fetched again in {:.0} s", ms as f64 / 1000.0));
    }
    line
}

/// A Theseus job's process tried to answer an approval and was refused
/// (theseus-6qy), as `theseus watch` says it.
fn job_refusal_line(p: &Value) -> String {
    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("?");
    let what = match s("act") {
        method::POLICY_UNTIGHTEN => format!("the undo of {}'s tightening", s("tool")),
        _ => format!("an answer to {}", s("tool")),
    };
    format!("refused {what} {} through {}", s("why"), s("via"))
}

/// The kernel line's note of job wrappers that linger for descendants their
/// command left running (theseus-6qy); nothing when none does.
fn lingering_note(n: u64) -> String {
    match n {
        0 => String::new(),
        1 => " · 1 job wrapper lingers for what its command left running".into(),
        n => format!(" · {n} job wrappers linger for what their commands left running"),
    }
}

/// `config: vault (confirmed in 1034 ms)`, `config: confirming …`, or
/// `config: held: <why>` (theseus-2fo): where the config came from, and
/// whether the vault has confirmed the copy this start served from. A daemon
/// older than that says nothing.
fn config_line(c: &theseus_protocol::ConfigStatus) -> Option<String> {
    let ms = |v: Option<u64>| v.map(|ms| format!("{ms} ms")).unwrap_or_else(|| "?".into());
    let mut line = match (c.source.as_str(), c.state.as_str()) {
        ("", _) => return None,
        ("file", _) => format!("config: file {}", c.reference),
        (_, "confirmed") if c.started_from == "vault" => format!(
            "config: vault (read before serving, in {}: {})",
            ms(c.confirmed_ms),
            c.detail.as_deref().unwrap_or("there was no copy")
        ),
        (_, "confirmed") => {
            let mut l = format!("config: vault (confirmed in {})", ms(c.confirmed_ms));
            if let Some(d) = c
                .detail
                .as_deref()
                .filter(|d| d.starts_with("only comments"))
            {
                l.push_str(&format!(": {d}"));
            }
            l
        }
        (_, "confirming") => format!(
            "config: confirming · serving from the copy of {}; nothing acts until the vault \
             confirms it",
            c.reference
        ),
        (_, state) => format!(
            "config: {state}: {}",
            c.detail.as_deref().unwrap_or("(no reason given)")
        ),
    };
    if let Some(ms) = c.retry_in_ms.filter(|_| c.state == "held") {
        line.push_str(&format!(
            "\n  the vault is read again in {:.0} s",
            ms as f64 / 1000.0
        ));
    }
    if let Some(r) = &c.restarted {
        line.push_str(&format!(
            "\n  restarted at {} onto the vault's changed note; changed since the copy: {}",
            fmt_time(r.at_unix_ms),
            r.tables.join(", ")
        ));
    }
    Some(line)
}

/// `startup: serving at 14.2 ms (config 1.0 ms · store 2.1 ms · …) · after:
/// secrets 1.03 s · …`: the last start's phases (theseus-qa0), and the time
/// no phase names, when there is some.
fn startup_line(phases: &[theseus_protocol::StartupPhase]) -> Option<String> {
    let serving = phases
        .iter()
        .filter(|p| !p.background)
        .filter_map(|p| p.end_us)
        .max()?;
    let span = |p: &theseus_protocol::StartupPhase| match p.end_us {
        Some(end) => format!("{} {}", p.name, fmt_us(end.saturating_sub(p.start_us))),
        None => format!("{} running", p.name),
    };
    let mut on: Vec<String> = phases.iter().filter(|p| !p.background).map(span).collect();
    // A slow start whose time shows here has a cause no phase names.
    let named: u64 = phases
        .iter()
        .filter(|p| !p.background)
        .filter_map(|p| p.end_us.map(|e| e.saturating_sub(p.start_us)))
        .sum();
    let between = serving.saturating_sub(named);
    if between >= 500 {
        on.push(format!("{} between phases", fmt_us(between)));
    }
    let after: Vec<String> = phases.iter().filter(|p| p.background).map(span).collect();
    let mut line = format!(
        "startup: serving at {} ({})",
        fmt_us(serving),
        on.join(" · ")
    );
    if !after.is_empty() {
        line.push_str(&format!(" · after: {}", after.join(" · ")));
    }
    Some(line)
}

/// A span's bar is drawn on this stretch of the turn: its start, its length,
/// and whether it is a `tools` span's own time rather than the whole turn's.
struct Frame {
    from_us: u64,
    total_us: u64,
    grouped: bool,
}

impl Frame {
    fn turn(root: &theseus_protocol::Span) -> Self {
        Self {
            from_us: 0,
            total_us: root.duration_us().max(1),
            grouped: false,
        }
    }
}

/// Indented tree with a 24-column bar: where in the turn each span sat. The
/// calls a `tools` span ran together are drawn on its own time, with `▓`, so
/// their overlap shows however short the group is beside the turn.
fn print_span(s: &theseus_protocol::Span, depth: usize, f: &Frame) {
    let width = 24usize;
    let at = |us: u64| us.saturating_sub(f.from_us) as f64 / f.total_us as f64 * width as f64;
    let a = at(s.start_us).floor() as usize;
    let b = at(s.end_us.unwrap_or(s.start_us)).ceil() as usize;
    let (a, b) = (a.min(width), b.clamp(a.min(width), width));
    let fill = if f.grouped { '▓' } else { '█' };
    let mut bar = String::new();
    for i in 0..width {
        bar.push(if i >= a && (i < b || (i == a && a == b)) {
            fill
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
    let own = Frame {
        from_us: s.start_us,
        total_us: s.duration_us().max(1),
        grouped: true,
    };
    let f = if s.kind == "tools" { &own } else { f };
    for c in &s.children {
        print_span(c, depth + 1, f);
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

/// A result status as the operator reads it: `declined` is a call that never
/// ran (declined, or superseded by a new message). A daemon from before
/// theseus-8az says `denied`.
fn status_word(status: &str) -> &str {
    match status {
        "declined" | "denied" => "not run",
        s => s,
    }
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
            // The policy's verdict (its posture; the band for older rows) with
            // its reason; a call that failed validation never reached policy.
            let gate = match d.get("decision") {
                Some(Value::Object(o)) => format!(
                    "{}: {}",
                    o.get("posture")
                        .or_else(|| o.get("mode"))
                        .and_then(Value::as_str)
                        .unwrap_or("?"),
                    clip(o.get("reason").and_then(Value::as_str).unwrap_or(""), 90)
                ),
                _ => match d.pointer("/result/gate").and_then(Value::as_str) {
                    Some("deny") => "invalid input".into(),
                    g => g.unwrap_or("-").to_string(),
                },
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
                    status_word(s("status")),
                    fmt_bytes(n.bytes),
                    indent(&n.text, "        ")
                );
            } else {
                println!(
                    "      ← {} {}{late}{ms} · {}: {}",
                    s("tool"),
                    status_word(s("status")),
                    fmt_bytes(n.bytes),
                    clip(&n.text, 160)
                );
            }
        }
        other => println!("[{t}] {other}"),
    }
}

/// An execution's budget in dollars: spend since the last reset, the limit,
/// what is reserved and held, and what came before dollar budgets.
fn budget_line(b: &theseus_protocol::BudgetInfo) -> String {
    let mut s = format!(
        "budget ${:.4} of ${:.2} (reserved ${:.4}, held ${:.4})",
        b.spent_usd, b.limit_usd, b.reserved_usd, b.held_unknown_usd
    );
    if b.resets > 0 {
        s.push_str(&format!(
            " · {} reset{}",
            b.resets,
            if b.resets == 1 { "" } else { "s" }
        ));
    }
    if let Some(q) = &b.question {
        s.push_str(&format!(" · at its limit: theseus confirm {q}"));
    }
    if !b.units_before.is_null() {
        s.push_str(&format!(
            " · before dollars: {} of {} units",
            b.units_before["spent"], b.units_before["limit"]
        ));
    }
    s
}

/// Health's `[approval]` (theseus-sgh): who may answer, and each listed
/// channel's state, with the reason for any that is not trusted.
/// `theseus policy list`: every tool's posture now and what set it, then any
/// tightening of a tool this daemon does not register.
fn policy_list(l: &ToolListResult, tightenings: &[theseus_protocol::Tightening]) -> String {
    let mut out = format!("{:<12} {:<8} set by\n", "tool", "posture");
    for t in &l.tools {
        let set_by = match &t.tightened {
            Some(x) if t.policy != t.config_posture => format!(
                "{} at {}{} (the config says {}, {})",
                t.setting,
                fmt_time(x.at_ms),
                x.correlation_id
                    .as_deref()
                    .map(|c| format!(", from {c}"))
                    .unwrap_or_default(),
                t.config_posture,
                t.config_setting
            ),
            Some(x) => format!(
                "{} (also tightened by {} at {})",
                t.setting,
                x.by,
                fmt_time(x.at_ms)
            ),
            None => t.setting.clone(),
        };
        out.push_str(&format!("{:<12} {:<8} {set_by}\n", t.name, t.policy));
    }
    for x in tightenings
        .iter()
        .filter(|x| !l.tools.iter().any(|t| t.name == x.tool))
    {
        out.push_str(&format!(
            "{:<12} {:<8} tightened by {} at {} (not a tool this daemon has)\n",
            x.tool,
            x.posture,
            x.by,
            fmt_time(x.at_ms)
        ));
    }
    let n = tightenings.len();
    out.push_str(&match n {
        0 => "no tightenings: every posture is the config's\n".to_string(),
        _ => format!(
            "{n} tightening{} · undo one with `theseus policy untighten <tool>`\n",
            if n == 1 { "" } else { "s" }
        ),
    });
    out
}

/// One line saying what `policy tighten` (`tightened`) or `policy untighten`
/// did.
fn tightened_line(r: &theseus_protocol::TightenResult, tightened: bool) -> String {
    match (tightened, r.already, r.changed) {
        (true, true, _) => format!(
            "{} already asks first: tightened by {} at {}",
            r.tool,
            r.tightening.by,
            fmt_time(r.tightening.at_ms)
        ),
        (true, false, true) => format!(
            "{} now asks first: tightened by {} (the config says {}, {}) · undo: theseus policy untighten {}",
            r.tool, r.by, r.config_posture, r.config_setting, r.tool
        ),
        (true, false, false) => format!(
            "{} already asks ({}); tightened by {} as well, so it keeps asking if the config changes",
            r.tool, r.config_setting, r.by
        ),
        (false, _, true) => format!(
            "{} is back to what the config says: {} ({}); the tightening by {} is undone",
            r.tool, r.posture, r.setting, r.tightening.by
        ),
        (false, _, false) => format!(
            "the tightening of {} by {} is undone; the config still asks ({})",
            r.tool, r.tightening.by, r.setting
        ),
    }
}

fn print_approval(a: &theseus_protocol::ApprovalStatus) {
    if !a.configured {
        println!(
            "approval: no [approval] section, so the CLI, the web UI, and a place's listed \
             Discord users answer"
        );
        return;
    }
    let users = if a.trusted_users.is_empty() {
        "nobody on Discord".to_string()
    } else {
        a.trusted_users.join(", ")
    };
    let channels: Vec<String> = a
        .channels
        .iter()
        .map(|c| format!("{} {}", c.channel, c.state.replace('_', " ")))
        .collect();
    println!(
        "approval: trusted users {users} · channels: {}",
        if channels.is_empty() {
            "none".to_string()
        } else {
            channels.join(", ")
        }
    );
    for c in a.channels.iter().filter(|c| c.state != "trusted") {
        println!("  {} is not trusted: {}", c.channel, c.detail);
    }
}

fn print_confirm(c: &ConfirmRequest) {
    if c.budget.is_some() {
        println!(
            "  $ {} waits for you: {}\n      reset and continue: theseus confirm {}\n      keep waiting: theseus confirm --decline {}",
            c.session_id, c.reason, c.correlation_id, c.correlation_id
        );
        return;
    }
    println!(
        "  ? {} waits for you in {}: {}\n      input: {}\n      approve: theseus confirm {}\n      decline: theseus confirm --decline {}",
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
                    status_word(&s("status")),
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
                    if c.budget.is_some() {
                        eprintln!(
                            "  $ {}\n      reset and continue: theseus confirm {}\n      keep waiting: theseus confirm --decline {}",
                            c.reason, c.correlation_id, c.correlation_id
                        );
                        return;
                    }
                    eprintln!(
                        "  ? {} needs your confirmation{}: {}\n      input: {}\n      approve: theseus confirm {}\n      decline: theseus confirm --decline {}",
                        c.tool,
                        if c.floor { " (FLOOR)" } else { "" },
                        c.reason,
                        clip(&c.input.to_string(), 200),
                        c.correlation_id,
                        c.correlation_id
                    );
                }
            }
            notify::POLICY_NOTIFIED => {
                self.settle();
                let tool = p.get("tool").and_then(Value::as_str).unwrap_or("?");
                eprintln!(
                    "  ! notified: {tool}: {}\n      ({}) · should have asked: theseus policy tighten {tool}{}",
                    p.get("summary").and_then(Value::as_str).unwrap_or(""),
                    p.get("setting").and_then(Value::as_str).unwrap_or(""),
                    p.get("correlation_id")
                        .and_then(Value::as_str)
                        .map(|c| format!(" --call {c}"))
                        .unwrap_or_default()
                );
            }
            notify::POLICY_TIGHTENED | notify::POLICY_UNTIGHTENED => {
                self.settle();
                if let Ok(r) = serde_json::from_value::<theseus_protocol::TightenResult>(p.clone())
                {
                    eprintln!("  🔒 {}", tightened_line(&r, m == notify::POLICY_TIGHTENED));
                }
            }
            notify::APPROVAL_REFUSED => {
                self.settle();
                eprintln!("  🚨 {}", job_refusal_line(p));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, policy: &str, config: &str, setting: &str) -> theseus_protocol::ToolInfo {
        theseus_protocol::ToolInfo {
            name: name.into(),
            wire_name: name.replace('.', "_"),
            family: String::new(),
            description: String::new(),
            class: "run".into(),
            backend: "job".into(),
            policy: policy.into(),
            setting: setting.into(),
            config_posture: config.into(),
            config_setting: "enforcement = notify".into(),
            tightened: None,
            input_schema: Value::Null,
            calls: 0,
        }
    }

    /// `theseus watch` says what a job's process tried, and why it was
    /// refused (theseus-6qy).
    #[test]
    fn a_jobs_refused_answer_is_one_line() {
        let why = "from a Theseus job's process (job act_j, pid 42, theseus)";
        assert_eq!(
            job_refusal_line(
                &serde_json::json!({"act": "action.confirm", "tool": "fs.write", "via": "cli", "why": why})
            ),
            format!("refused an answer to fs.write {why} through cli")
        );
        assert_eq!(
            job_refusal_line(
                &serde_json::json!({"act": "policy.untighten", "tool": "fs.edit", "via": "web", "why": why})
            ),
            format!("refused the undo of fs.edit's tightening {why} through web")
        );
    }

    /// The kernel line counts the job wrappers that linger, and says nothing
    /// when none does (theseus-6qy).
    #[test]
    fn the_kernel_line_counts_lingering_wrappers() {
        assert_eq!(lingering_note(0), "");
        assert_eq!(
            lingering_note(1),
            " · 1 job wrapper lingers for what its command left running"
        );
        assert_eq!(
            lingering_note(3),
            " · 3 job wrappers linger for what their commands left running"
        );
    }

    /// `theseus policy list` says what set each posture, and marks a
    /// tightening with who, when, and the call; `tighten` and `untighten`
    /// say what changed (theseus-sgh).
    #[test]
    fn policy_list_and_its_lines_say_what_set_each_posture() {
        let t = theseus_protocol::Tightening {
            tool: "proc.run".into(),
            posture: "approve".into(),
            by: "discord:eddie".into(),
            at_ms: 3_600_000,
            correlation_id: Some("act_1".into()),
            ..Default::default()
        };
        let mut run = tool(
            "proc.run",
            "approve",
            "notify",
            "tightened by discord:eddie",
        );
        run.tightened = Some(t.clone());
        let l = ToolListResult {
            tools: vec![
                run,
                tool("fs.write", "notify", "notify", "enforcement = notify"),
            ],
            roots: vec![],
            shell_fallback_ratio: 0.0,
            calls_total: 0,
        };
        let gone = theseus_protocol::Tightening {
            tool: "mcp:x/y".into(),
            ..t.clone()
        };
        let out = policy_list(&l, &[t.clone(), gone]);
        assert!(
            out.contains(
                "proc.run     approve  tightened by discord:eddie at 01:00:00.000Z, from act_1 \
                 (the config says notify, enforcement = notify)"
            ),
            "{out}"
        );
        assert!(
            out.contains("fs.write     notify   enforcement = notify"),
            "{out}"
        );
        assert!(
            out.contains("mcp:x/y      approve  tightened by discord:eddie"),
            "{out}"
        );
        assert!(out.ends_with("2 tightenings · undo one with `theseus policy untighten <tool>`\n"));
        let r = theseus_protocol::TightenResult {
            tool: "proc.run".into(),
            by: "sock#3".into(),
            tightening: t,
            posture: "notify".into(),
            setting: "enforcement = notify".into(),
            config_posture: "notify".into(),
            config_setting: "enforcement = notify".into(),
            changed: true,
            already: false,
        };
        assert_eq!(
            tightened_line(&r, false),
            "proc.run is back to what the config says: notify (enforcement = notify); the \
             tightening by discord:eddie is undone"
        );
        assert!(
            tightened_line(&r, true).starts_with("proc.run now asks first: tightened by sock#3")
        );
    }

    #[test]
    fn attach_sends_a_text_files_text_and_lists_anything_else() {
        let dir = std::env::temp_dir().join(format!("theseus-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "The fact is 42.\n").unwrap();
        let a = attachment_for(&notes).unwrap();
        assert_eq!(
            (a.name.as_str(), a.media_type.as_str(), a.size),
            ("notes.txt", "text/plain", 16)
        );
        assert_eq!(a.text.as_deref(), Some("The fact is 42.\n"));
        assert!(a.not_read.is_none());

        // Not text: its bytes go, and the daemon keeps them only if they are an image.
        let shot = dir.join("shot.png");
        std::fs::write(&shot, b"\x89PNG\r\n\x1a\n").unwrap();
        let b = attachment_for(&shot).unwrap();
        assert_eq!(b.data.as_deref(), Some("iVBORw0KGgo="));
        assert!(b.text.is_none() && b.not_read.is_none());

        assert!(attachment_for(&dir.join("missing.txt")).is_err());
        assert!(attachment_for(&dir).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `theseus health` says where the secrets stand and names a failure
    /// with its reason; the startup line splits the start path from what
    /// follows it (theseus-qa0).
    #[test]
    fn health_says_resolving_ready_or_failed_and_times_the_start() {
        use theseus_protocol::{SecretFailed, SecretsStatus, StartupPhase};
        let resolving = SecretsStatus {
            state: "resolving".into(),
            resolving: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        assert_eq!(
            secrets_line(&resolving, &[]),
            "secrets: resolving · 0 of 2 ready so far"
        );
        let failed = SecretsStatus {
            state: "failed".into(),
            ready: vec!["a".into()],
            failed: vec![SecretFailed {
                name: "b".into(),
                error: "could not find item".into(),
            }],
            method: Some("inject, then read".into()),
            settled_ms: Some(1720),
            retry_in_ms: Some(4000),
            ..Default::default()
        };
        assert_eq!(
            secrets_line(&failed, &[]),
            "secrets: failed b · 1 ready 1720 ms after start (inject, then read) [a]\n  b did not \
             resolve: could not find item\n  fetched again in 4 s"
        );
        assert_eq!(
            secrets_line(&SecretsStatus::default(), &["a".into()]),
            "secrets [a]",
            "an older daemon"
        );
        let phase = |name: &str, bg: bool, start: u64, end: Option<u64>| StartupPhase {
            name: name.into(),
            background: bg,
            start_us: start,
            end_us: end,
            detail: Value::Null,
        };
        let line = startup_line(&[
            phase("config", false, 100, Some(900)),
            phase("socket", false, 11_000, Some(11_400)),
            phase("secrets", true, 1000, None),
        ])
        .unwrap();
        // No phase names the 10.1 ms from config's end to the socket's
        // start, nor the 0.1 ms before config: 10.2 ms in all.
        assert_eq!(
            line,
            "startup: serving at 11.4 ms (config 800 µs · socket 400 µs · 10.2 ms between phases) \
             · after: secrets running"
        );
        assert!(startup_line(&[]).is_none());
    }

    /// `config:` in `theseus health` (theseus-2fo): a file, a read before
    /// serving, confirming, confirmed from the copy, and held after a restart
    /// onto a changed note, with the next read.
    #[test]
    fn health_says_where_the_config_came_from_and_whether_it_may_act() {
        use theseus_protocol::{ConfigRestart, ConfigStatus};
        let vault = |state: &str, from: &str| ConfigStatus {
            source: "vault".into(),
            reference: "op://V/c/notesPlain".into(),
            state: state.into(),
            started_from: from.into(),
            ..Default::default()
        };
        assert!(
            config_line(&ConfigStatus::default()).is_none(),
            "an older daemon"
        );
        let file = ConfigStatus {
            source: "file".into(),
            reference: "/x/c.toml".into(),
            state: "confirmed".into(),
            started_from: "file".into(),
            ..Default::default()
        };
        assert_eq!(config_line(&file).unwrap(), "config: file /x/c.toml");
        let first = ConfigStatus {
            detail: Some("there was no copy yet; the copy is kept for the next start".into()),
            confirmed_ms: Some(1012),
            ..vault("confirmed", "vault")
        };
        assert_eq!(
            config_line(&first).unwrap(),
            "config: vault (read before serving, in 1012 ms: there was no copy yet; the copy is \
             kept for the next start)"
        );
        assert_eq!(
            config_line(&vault("confirming", "copy")).unwrap(),
            "config: confirming · serving from the copy of op://V/c/notesPlain; nothing acts \
             until the vault confirms it"
        );
        let ok = ConfigStatus {
            confirmed_ms: Some(1034),
            detail: Some("the same text as the copy".into()),
            ..vault("confirmed", "copy")
        };
        assert_eq!(
            config_line(&ok).unwrap(),
            "config: vault (confirmed in 1034 ms)"
        );
        let held = ConfigStatus {
            detail: Some(
                "the vault's note changed again since the restart; restart to apply".into(),
            ),
            retry_in_ms: Some(10_000),
            restarted: Some(ConfigRestart {
                reference: "op://V/c/notesPlain".into(),
                at_unix_ms: 3_600_000,
                tables: vec!["kernel".into()],
                ..Default::default()
            }),
            ..vault("held", "copy")
        };
        assert_eq!(
            config_line(&held).unwrap(),
            "config: held: the vault's note changed again since the restart; restart to apply\n  \
             the vault is read again in 10 s\n  restarted at 01:00:00.000Z onto the vault's \
             changed note; changed since the copy: kernel"
        );
    }
}
