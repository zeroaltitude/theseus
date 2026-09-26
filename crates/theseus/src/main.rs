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
    method, notify, HealthResult, HooksListResult, HooksRegisterParams, Id, LedgerTailParams,
    LedgerTailResult, Message, ProfileListResult, ProfileUseParams, Request, SessionListResult,
    SessionOpenParams, TurnSubmitParams, TurnSubmitResult,
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
    },
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
        } => {
            let prompt = match prompt.as_deref() {
                None | Some("-") => read_stdin_prompt()?,
                Some(p) => p.to_string(),
            };
            let mut streamed_any = false;
            let call = conn
                .call(
                    method::TURN_SUBMIT,
                    serde_json::to_value(TurnSubmitParams {
                        session_id: session,
                        input: prompt,
                        profile,
                        provider,
                        model,
                    })?,
                    |m, p| {
                        if stream && m == notify::MODEL_DELTA {
                            if let Some(t) = p.get("text").and_then(Value::as_str) {
                                use std::io::Write;
                                let mut out = std::io::stdout().lock();
                                let _ = out.write_all(t.as_bytes());
                                let _ = out.flush();
                                streamed_any = true;
                            }
                        }
                    },
                )
                .await;
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
                if streamed_any && !r.output.ends_with('\n') {
                    println!();
                }
                eprintln!("{}", status_line(&r));
            } else {
                println!("{}", r.output);
                eprintln!("{}", status_line(&r));
            }
            if trace && !json {
                if let Some(t) = &r.trace {
                    eprintln!("--- trace ({} total)", fmt_us(t.duration_us()));
                    print_span(t, 0, t.duration_us().max(1));
                }
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
                println!(
                    "tokens total: in {} out {} cache-read {} cache-write {} · secrets [{}]",
                    h.usage_total.input_tokens,
                    h.usage_total.output_tokens,
                    h.usage_total.cache_read_input_tokens,
                    h.usage_total.cache_creation_input_tokens,
                    h.secrets_resolved.join(", ")
                );
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
                            "{}\t{:?}\tturns={}\tin={}\tout={}\t{}",
                            s.session_id,
                            s.kind,
                            s.turns,
                            s.usage.input_tokens,
                            s.usage.output_tokens,
                            s.label.unwrap_or_default()
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
                            "{} {:<12} {:<10} {:<28} max_tokens={}{}",
                            if p.live { "*" } else { " " },
                            p.name,
                            p.provider,
                            p.model,
                            p.max_tokens,
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
        "[{} → {}/{} · {} loop(s) · {} · tokens in {} out {}{} · {} ms{} · session {}]",
        r.profile,
        r.provider,
        r.model,
        r.loops,
        r.stop_reason,
        r.usage.input_tokens,
        r.usage.output_tokens,
        cache,
        r.elapsed_ms,
        r.first_token_ms
            .map(|t| format!(" (first token {t} ms)"))
            .unwrap_or_default(),
        r.session_id
    )
}
