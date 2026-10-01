//! The crate's example client, for live checks against real servers:
//! connect, list, call, get a prompt, and print what came back, briefly.
//!
//! ```text
//! client [--list] [--call NAME [--args JSON]] [--progress] [--timeout-ms N]
//!        [--prompt NAME [--arg K=V]...] [--ping] [--watch-ms N]
//!        [--bearer-file F] [--connect-ms N]
//!        (--port N [--path /mcp] | --url-file F | --stdio CMD ARGS...)
//! ```
//!
//! `--port` reaches a server on 127.0.0.1, its URL made here; `--url-file`
//! reads one from a file. `--stdio` takes the rest of the line as the
//! server's command.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use theseus_mcp::client::{
    stdio_command, CallOptions, Client, Event, HttpTarget, Options, StdioServer, Transport,
};
use theseus_mcp::jsonrpc::clip;

#[derive(Default)]
struct Args {
    list: bool,
    call: Option<String>,
    call_args: Option<Value>,
    progress: bool,
    timeout_ms: Option<u64>,
    prompt: Option<String>,
    prompt_args: BTreeMap<String, String>,
    ping: bool,
    watch_ms: u64,
    bearer_file: Option<String>,
    connect_ms: u64,
    port: Option<u16>,
    path: Option<String>,
    url_file: Option<String>,
    stdio: Vec<String>,
}

fn fail(msg: &str) -> ! {
    eprintln!("client: {msg}");
    std::process::exit(2)
}

fn need(it: &mut impl Iterator<Item = String>, flag: &str) -> String {
    it.next()
        .unwrap_or_else(|| fail(&format!("{flag} needs a value")))
}

fn parse() -> Args {
    let mut a = Args {
        connect_ms: 5_000,
        ..Args::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--list" => a.list = true,
            "--call" => a.call = Some(need(&mut it, "--call")),
            "--args" => {
                let raw = need(&mut it, "--args");
                a.call_args = Some(
                    serde_json::from_str(&raw)
                        .unwrap_or_else(|e| fail(&format!("--args is not JSON: {e}"))),
                );
            }
            "--progress" => a.progress = true,
            "--timeout-ms" => {
                a.timeout_ms = Some(
                    need(&mut it, "--timeout-ms")
                        .parse()
                        .unwrap_or_else(|_| fail("--timeout-ms wants a number")),
                )
            }
            "--prompt" => a.prompt = Some(need(&mut it, "--prompt")),
            "--arg" => {
                let kv = need(&mut it, "--arg");
                let (k, v) = kv
                    .split_once('=')
                    .unwrap_or_else(|| fail("--arg wants K=V"));
                a.prompt_args.insert(k.into(), v.into());
            }
            "--ping" => a.ping = true,
            "--watch-ms" => {
                a.watch_ms = need(&mut it, "--watch-ms")
                    .parse()
                    .unwrap_or_else(|_| fail("--watch-ms wants a number"))
            }
            "--bearer-file" => a.bearer_file = Some(need(&mut it, "--bearer-file")),
            "--connect-ms" => {
                a.connect_ms = need(&mut it, "--connect-ms")
                    .parse()
                    .unwrap_or_else(|_| fail("--connect-ms wants a number"))
            }
            "--port" => {
                a.port = Some(
                    need(&mut it, "--port")
                        .parse()
                        .unwrap_or_else(|_| fail("--port wants a number")),
                )
            }
            "--path" => a.path = Some(need(&mut it, "--path")),
            "--url-file" => a.url_file = Some(need(&mut it, "--url-file")),
            "--stdio" => {
                a.stdio = it.by_ref().collect();
                if a.stdio.is_empty() {
                    fail("--stdio needs the server's command");
                }
            }
            other => fail(&format!("unknown argument {other:?}")),
        }
    }
    a
}

fn transport(a: &Args) -> Transport {
    if !a.stdio.is_empty() {
        let child = stdio_command(&a.stdio)
            .expect("a command")
            .spawn()
            .unwrap_or_else(|e| fail(&format!("starting {}: {e}", a.stdio[0])));
        return Transport::Stdio(StdioServer {
            child,
            stderr_log: None,
        });
    }
    let url = match (&a.port, &a.url_file) {
        (Some(port), _) => format!(
            "http://127.0.0.1:{port}{}",
            a.path.as_deref().unwrap_or("/mcp")
        ),
        (None, Some(f)) => std::fs::read_to_string(f)
            .unwrap_or_else(|e| fail(&format!("reading {f}: {e}")))
            .trim()
            .to_string(),
        (None, None) => fail("name a server: --port, --url-file, or --stdio"),
    };
    let mut t = HttpTarget::new(url);
    t.connect_timeout = Duration::from_millis(a.connect_ms);
    if let Some(f) = &a.bearer_file {
        let key = std::fs::read_to_string(f).unwrap_or_else(|e| fail(&format!("reading {f}: {e}")));
        t.bearer = Some(key.trim().to_string());
    }
    Transport::Http(t)
}

fn show(e: &Event) -> String {
    match e {
        Event::Progress {
            progress, total, ..
        } => format!("progress {progress}/{}", total.unwrap_or(0.0)),
        Event::Log { level, data, .. } => format!("log {level}: {}", clip(&data.to_string(), 120)),
        Event::Notification { method, .. } => format!("notification {method}"),
        Event::Closed { reason } => format!("closed: {}", clip(reason, 200)),
        other => format!("{other:?}"),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a = parse();
    let started = Instant::now();
    let (client, mut events) = match Client::connect(transport(&a), Options::default()).await {
        Ok(c) => c,
        Err(e) => fail(&format!("connect failed: {e}")),
    };
    let info = client.server_info();
    println!(
        "connected in {} ms: {} {}, revision {}{}{}",
        started.elapsed().as_millis(),
        info.server.name,
        info.server.version,
        info.protocol_version,
        client
            .session_id()
            .map(|s| format!(", session {}", clip(&s, 12)))
            .unwrap_or_default(),
        client
            .pid()
            .map(|p| format!(", pid {p}"))
            .unwrap_or_default(),
    );
    let mut failed = false;
    if a.list {
        match client.list_tools().await {
            Ok(tools) => {
                let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
                println!("tools ({}): {}", tools.len(), names.join(", "));
            }
            Err(e) => {
                failed = true;
                println!("tools/list failed: {e}");
            }
        }
        if info.capabilities.prompts.is_some() {
            match client.list_prompts().await {
                Ok(prompts) => {
                    let names: Vec<&str> = prompts.iter().map(|p| p.name.as_str()).collect();
                    println!("prompts ({}): {}", prompts.len(), names.join(", "));
                }
                Err(e) => {
                    failed = true;
                    println!("prompts/list failed: {e}");
                }
            }
        }
    }
    if let Some(name) = &a.call {
        let call = CallOptions {
            timeout: a.timeout_ms.map(Duration::from_millis),
            progress_token: a.progress.then(|| json!("example-1")),
        };
        let t = Instant::now();
        let args = a.call_args.clone().unwrap_or_else(|| json!({}));
        match client.call_tool_with(name, args, &call).await {
            Ok(r) => println!(
                "call {name}: {} in {} ms: {}",
                if r.is_error { "isError" } else { "ok" },
                t.elapsed().as_millis(),
                clip(&r.text_for_model().replace('\n', " | "), 300)
            ),
            Err(e) => {
                failed = true;
                println!("call {name} failed in {} ms: {e}", t.elapsed().as_millis());
            }
        }
    }
    if let Some(name) = &a.prompt {
        match client.get_prompt(name, &a.prompt_args).await {
            Ok(p) => {
                let text: Vec<String> = p
                    .messages
                    .iter()
                    .map(|m| format!("{}: {}", m.role, m.content.as_model_text()))
                    .collect();
                println!(
                    "prompt {name}: {}",
                    clip(&text.join(" | ").replace('\n', " "), 300)
                );
            }
            Err(e) => {
                failed = true;
                println!("prompt {name} failed: {e}");
            }
        }
    }
    if a.ping {
        match client.ping().await {
            Ok(d) => println!("ping: {} ms", d.as_millis()),
            Err(e) => {
                failed = true;
                println!("ping failed: {e}");
            }
        }
    }
    if a.watch_ms > 0 {
        let until = tokio::time::Instant::now() + Duration::from_millis(a.watch_ms);
        while let Ok(Some(e)) = tokio::time::timeout_at(until, events.recv()).await {
            println!("event: {}", show(&e));
        }
    }
    while let Ok(e) = events.try_recv() {
        println!("event: {}", show(&e));
    }
    client.close().await;
    println!("closed after {} ms", started.elapsed().as_millis());
    if failed {
        std::process::exit(1);
    }
}
