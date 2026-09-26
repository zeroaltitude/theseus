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
    method, notify, HealthResult, HooksListResult, HooksRegisterParams, Id, Message, Request,
    SessionListResult, SessionOpenParams, TurnSubmitParams, TurnSubmitResult,
};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

#[derive(Parser, Debug)]
#[command(
    name = "theseus",
    version,
    about = "Theseus CLI",
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
    },
    /// Server health.
    Health,
    /// Sessions.
    Sessions {
        #[command(subcommand)]
        cmd: Option<SessionsCmd>,
    },
    /// Hook events and registered handlers.
    Hooks {
        #[command(subcommand)]
        cmd: HooksCmd,
    },
    /// Send a raw JSON-RPC request: METHOD and optional PARAMS (JSON).
    Rpc {
        method: String,
        params: Option<String>,
    },
    /// Ask the server to stop.
    Shutdown,
}

#[derive(Subcommand, Debug)]
enum SessionsCmd {
    List,
    Open {
        #[arg(long)]
        label: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum HooksCmd {
    List,
    /// Register as an observer of EVENT and print each hook.event as it arrives (Ctrl-C to stop).
    Watch {
        event: String,
        #[arg(long, default_value = "cli-watch")]
        handler_id: String,
    },
}

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
                        return Err(anyhow!("{} (code {})", e.message, e.code));
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
        Cmd::Ask { prompt, session } => {
            let prompt = match prompt.as_deref() {
                None | Some("-") => read_stdin_prompt()?,
                Some(p) => p.to_string(),
            };
            let mut streamed_any = false;
            let result = conn
                .call(
                    method::TURN_SUBMIT,
                    serde_json::to_value(TurnSubmitParams {
                        session_id: session,
                        input: prompt,
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
                .await?;
            let r: TurnSubmitResult = serde_json::from_value(result.clone())?;
            if json {
                println!("{}", serde_json::to_string(&result)?);
            } else if stream {
                if streamed_any && !r.output.ends_with('\n') {
                    println!();
                }
                eprintln!(
                    "[{} · {} loop(s) · {} · in {} out {} · {} ms · session {}]",
                    r.model,
                    r.loops,
                    r.stop_reason,
                    r.usage.input_tokens,
                    r.usage.output_tokens,
                    r.elapsed_ms,
                    r.session_id
                );
            } else {
                println!("{}", r.output);
            }
        }
        Cmd::Health => {
            let v = conn.call(method::HEALTH, Value::Null, |_, _| {}).await?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                let h: HealthResult = serde_json::from_value(v)?;
                println!(
                    "{} {} · protocol {} · up {}s · model {} · sessions {} · turns {} · secrets [{}]",
                    h.name,
                    h.version,
                    h.protocol,
                    h.uptime_secs,
                    h.model,
                    h.sessions,
                    h.turns,
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
                            "{}\t{:?}\tturns={}\t{}",
                            s.session_id,
                            s.kind,
                            s.turns,
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
