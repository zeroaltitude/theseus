//! `theseus-sim fake-mcp` (M7 36b): the fake MCP server
//! (`theseus_mcp::fake`) for a scratch daemon's `[mcp.servers]` and the
//! live checks. Over stdio by default, as a daemon starts a server (a crash
//! exits with status 3, and it ends when its stdin closes); with `--http`,
//! streamable HTTP on 127.0.0.1 at `/mcp`, its address on stderr.

use anyhow::{bail, Context, Result};
use clap::Args;
use theseus_mcp::fake::{Config, Fake, Mode};

#[derive(Args, Debug)]
pub struct FakeMcpArgs {
    /// `ok`, `slow`, `crash-after=N`, `change-tools`, or `error`.
    #[arg(long, default_value = "ok")]
    mode: String,
    /// Serve streamable HTTP on 127.0.0.1 instead of stdio.
    #[arg(long)]
    http: bool,
    /// The HTTP port (0: any free one).
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Tools and prompts per page of a list.
    #[arg(long, default_value_t = 2)]
    page_size: usize,
    /// How long a call takes in `slow`.
    #[arg(long, default_value_t = 5_000)]
    slow_ms: u64,
    /// The name it gives in `initialize`.
    #[arg(long, default_value = "fake")]
    name: String,
}

pub fn run(a: FakeMcpArgs) -> Result<()> {
    let Some(mode) = Mode::parse(&a.mode) else {
        bail!(
            "no mode {:?}: ok, slow, crash-after=N, change-tools, or error",
            a.mode
        )
    };
    let cfg = Config {
        mode,
        page_size: a.page_size.max(1),
        slow_ms: a.slow_ms,
        name: a.name,
        exit_on_crash: !a.http,
        ..Config::default()
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the runtime")?;
    rt.block_on(async move {
        if a.http {
            let addr = Fake::new(cfg.clone())
                .serve_http(a.port)
                .await
                .with_context(|| format!("binding 127.0.0.1:{}", a.port))?;
            eprintln!("fake mcp on http://{addr}/mcp");
            std::future::pending::<()>().await;
        }
        Fake::new(cfg)
            .serve_pipes(tokio::io::stdin(), tokio::io::stdout())
            .await;
        Ok(())
    })
}
