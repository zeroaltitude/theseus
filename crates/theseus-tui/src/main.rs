//! `theseus-tui` (design `stage2` §2.9, theseus-7yx): every session's state
//! in one sidebar, with its task trees, kept current by the daemon's push;
//! the session in focus, with its input line; and the queue of what needs
//! you, answered inline, and of what finished since you looked (done until
//! seen), with a notice and the count in the terminal's title. A client of
//! `theseusd` like the CLI, over the same socket: it links the protocol crate
//! and the CLI's library (`theseus_client`), never the core. `theseus tui`
//! execs it, with the CLI's `--socket` first (step 10f).

mod app;
mod board;
mod card;
mod detail;
mod notice;
mod run;
mod term;
mod ui;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_names;
#[cfg(test)]
mod tests_notice;
#[cfg(test)]
mod tests_order;
#[cfg(test)]
mod tests_paste;
#[cfg(test)]
mod tests_typing;

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use theseus_client::Conn;
use tokio::signal::unix::{signal, SignalKind};

use crate::app::App;
use crate::notice::Delivery;
use crate::run::{Connector, Runner};
use theseus_client::seen::Seen;

const USAGE: &str = "theseus-tui: every session of a running theseusd, in one terminal

usage: theseus-tui [--socket PATH] [--notify HOW]

  --socket PATH   the daemon's Unix socket (default: $THESEUS_SOCKET, else ~/.theseus/theseus.sock)
  --notify HOW    how a notice (needs you, finished) reaches you: bell (the default), osc9,
                  osc777, or off; the terminal's title carries the count either way
  -h, --help      this help
  -V, --version   the version

What you have seen is kept in $XDG_STATE_HOME/theseus/seen.json
(~/.local/state/theseus/seen.json), on this machine only. Keys: ? in the TUI.";

/// What the command line says.
struct Args {
    socket: String,
    notify: Delivery,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Self>> {
        let mut socket = std::env::var("THESEUS_SOCKET")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "~/.theseus/theseus.sock".to_string());
        let mut notify = Delivery::default();
        let how = |v: Option<&str>| {
            v.and_then(Delivery::parse).ok_or_else(|| {
                anyhow::anyhow!("--notify takes bell, osc9, osc777, or off\n\n{USAGE}")
            })
        };
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
                "--notify" => notify = how(args.next().as_deref())?,
                other => {
                    if let Some(p) = other.strip_prefix("--socket=") {
                        socket = p.to_string();
                    } else if let Some(v) = other.strip_prefix("--notify=") {
                        notify = how(Some(v))?;
                    } else {
                        bail!("unknown argument `{other}`\n\n{USAGE}");
                    }
                }
            }
        }
        Ok(Some(Self { socket, notify }))
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
    let mut app = App::new(local_hm);
    app.seen = Seen::open(Seen::default_path());
    // SIGTERM and SIGHUP end the loop as a quit does, so the terminal is
    // put back (theseus-8hcg).
    let (end, signals) = tokio::sync::mpsc::unbounded_channel();
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;
    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sighup.recv() => {}
        }
        let _ = end.send(());
    });
    // Raw mode and the alternate screen, put back on exit and on a panic;
    // the loop turns on focus events and bracketed paste, and off again
    // (`term`), and a panic turns them off.
    let terminal = ratatui::try_init()?;
    term::on_panic(|| Box::new(std::io::stdout()));
    let mut runner = Runner::new(app, terminal, connect, rx, now_ms);
    runner.out = Box::new(std::io::stdout());
    runner.delivery = args.notify;
    runner.app.quiet = args.notify == Delivery::Off;
    runner.signals = Some(signals);
    let ran = runner.run().await;
    if let Err(e) = ratatui::try_restore() {
        // The terminal is gone, as after a hangup: there is nothing to put
        // back. A print to it fails, and a failed `eprintln!` (the error's
        // own, or the terminal's drop showing its cursor) is a panic, which
        // aborts. So say it where it can be said, and leave without the
        // drops; the seen file was written as the loop ended.
        let mut err = std::io::stderr();
        if let Err(r) = &ran {
            let _ = writeln!(err, "theseus-tui: {r:#}");
        }
        let _ = writeln!(err, "theseus-tui: the terminal: {e}");
        std::process::exit(i32::from(ran.is_err()));
    }
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
/// labels' times (`sleeping until 16:00`), and as every time the TUI and the
/// CLI show is written (theseus-0n1v).
fn local_hm(unix_ms: u64) -> String {
    theseus_client::render::time::fmt_hm(unix_ms)
}
