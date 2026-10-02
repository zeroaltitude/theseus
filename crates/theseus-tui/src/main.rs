//! `theseus-tui` (design `stage2` §2.9, theseus-7yx): every session's state
//! in one sidebar, with its task trees, kept current by the daemon's push.
//! A client of `theseusd` like the CLI, over the same socket: it links the
//! protocol crate and the CLI's library (`theseus_client`), never the core.
//! `theseus tui` will exec it (step 10f).

mod app;
mod board;
mod detail;
mod run;
mod ui;

#[cfg(test)]
mod tests;

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use theseus_client::Conn;

use crate::app::App;
use crate::run::{Connector, Runner};

const USAGE: &str = "theseus-tui: every session of a running theseusd, in one terminal

usage: theseus-tui [--socket PATH]

  --socket PATH   the daemon's Unix socket (default: $THESEUS_SOCKET, else ~/.theseus/theseus.sock)
  -h, --help      this help
  -V, --version   the version

Keys: ? in the TUI.";

/// What the command line says.
struct Args {
    socket: String,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Self>> {
        let mut socket = std::env::var("THESEUS_SOCKET")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "~/.theseus/theseus.sock".to_string());
        while let Some(a) = args.next() {
            match a.as_str() {
                "-h" | "--help" => {
                    println!("{USAGE}");
                    return Ok(None);
                }
                "-V" | "--version" => {
                    println!("theseus-tui {}", env!("CARGO_PKG_VERSION"));
                    return Ok(None);
                }
                "--socket" => match args.next() {
                    Some(p) => socket = p,
                    None => bail!("--socket needs a path\n\n{USAGE}"),
                },
                other => match other.strip_prefix("--socket=") {
                    Some(p) => socket = p.to_string(),
                    None => bail!("unknown argument `{other}`\n\n{USAGE}"),
                },
            }
        }
        Ok(Some(Self { socket }))
    }
}

fn main() -> Result<()> {
    let Some(args) = Args::parse(std::env::args().skip(1))? else {
        return Ok(());
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(tui(args))
}

async fn tui(args: Args) -> Result<()> {
    let socket = args.socket;
    let connect: Connector = Box::new(move || {
        let path = socket.clone();
        Box::pin(async move { Conn::socket(&path).await })
    });
    // The terminal's events, read on a thread of their own: a blocking read
    // wakes nothing while the operator types nothing.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(e) = crossterm::event::read() {
            if tx.send(e).is_err() {
                break;
            }
        }
    });
    // Raw mode and the alternate screen, put back on exit and on a panic.
    let term = ratatui::try_init()?;
    let mut runner = Runner::new(App::new(local_hm), term, connect, rx, now_ms);
    let ran = runner.run().await;
    ratatui::try_restore()?;
    ran
}

/// The wall clock, in ms since the epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// A time of day on this machine's clock, `15:42`, as the daemon writes its
/// labels' times (`sleeping until 16:00`).
fn local_hm(unix_ms: u64) -> String {
    jiff::Timestamp::from_millisecond(unix_ms as i64)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| theseus_protocol::utc_hm(unix_ms))
}
