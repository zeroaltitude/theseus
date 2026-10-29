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
mod board_threads;
mod card;
mod chat;
mod detail;
mod follow;
mod glyph;
mod held;
mod horizon;
mod new_task;
mod notice;
mod one_turn;
mod run;
mod selfview;
mod standing;
mod styled;
mod task_return;
mod term;
mod threads;
mod ui;
mod work;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_answer;
#[cfg(test)]
mod tests_board_edges;
#[cfg(test)]
mod tests_board_threads;
#[cfg(test)]
mod tests_cancel;
#[cfg(test)]
mod tests_cite;
#[cfg(test)]
mod tests_diff;
#[cfg(test)]
mod tests_digest;
#[cfg(test)]
mod tests_failure;
#[cfg(test)]
mod tests_follow;
#[cfg(test)]
mod tests_grant;
#[cfg(test)]
mod tests_horizon;
#[cfg(test)]
mod tests_markdown;
#[cfg(test)]
mod tests_mode;
#[cfg(test)]
mod tests_names;
#[cfg(test)]
mod tests_new_task;
#[cfg(test)]
mod tests_notice;
#[cfg(test)]
mod tests_order;
#[cfg(test)]
mod tests_paste;
#[cfg(test)]
mod tests_progress;
#[cfg(test)]
mod tests_questions;
#[cfg(test)]
mod tests_self;
#[cfg(test)]
mod tests_standing;
#[cfg(test)]
mod tests_stop_all;
#[cfg(test)]
mod tests_sync;
#[cfg(test)]
mod tests_system;
#[cfg(test)]
mod tests_task_return;
#[cfg(test)]
mod tests_threads;
#[cfg(test)]
mod tests_typing;
#[cfg(test)]
mod tests_work;

use std::io::Write;
use theseus_protocol::now_unix_ms;

use anyhow::{bail, Result};
use theseus_client::Conn;
use tokio::signal::unix::{signal, SignalKind};

use crate::app::App;
use crate::notice::Delivery;
use crate::run::{Connector, Runner};
use theseus_client::seen::Seen;

const USAGE: &str = "theseus-tui: every session of a running theseusd, in one terminal

usage: theseus-tui [--socket PATH] [--notify HOW]
       theseus-tui [--socket PATH] chat [SESSION | -c | -r | --task ID [--inspect]]

  chat [SESSION]  an inline conversation in this directory (a new one, or SESSION's);
                  `theseus` and `theseus chat` run it
  chat -c         continue this directory's last conversation (`theseus -c`)
  chat -r         pick a conversation to continue, searched as you type (`theseus -r`)
  chat --task ID  open a task joined, with its conversation behind it (esc goes back);
                  with --inspect, read-only
  --socket PATH   the daemon's Unix socket (default: $THESEUS_SOCKET, else ~/.theseus/theseus.sock)
  --notify HOW    how a notice (needs you, finished) reaches you: auto (the default: a Windows
                  toast under WSL, OSC 9 in a terminal that has it, else the bell), toast, bell,
                  osc9, osc777, or off; the terminal's title carries the count either way. Without
                  the flag, the daemon's `[tui] notify` says
  -h, --help      this help
  -V, --version   the version

What you have seen is kept in $XDG_STATE_HOME/theseus/seen.json
(~/.local/state/theseus/seen.json), on this machine only. Keys: ? in the TUI.";

/// What the command line says.
struct Args {
    socket: String,
    /// `--notify`, when given; else the config's `[tui] notify`, else `auto`.
    notify: Option<Delivery>,
    /// `chat [SESSION]`: the inline conversation instead of the board.
    chat: Option<Option<String>>,
    /// `chat -c` and `chat -r`: the last conversation here, or a pick.
    start: chat::Start,
    /// `chat --task ID [--inspect]`: a task's id, and whether to join it.
    task: Option<(String, bool)>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<Self>> {
        let mut socket = std::env::var("THESEUS_SOCKET")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "~/.theseus/theseus.sock".to_string());
        let mut notify = None;
        let mut chat: Option<Option<String>> = None;
        let mut start = chat::Start::New;
        let mut task: Option<String> = None;
        let mut inspect = false;
        let how = |v: Option<&str>| {
            v.and_then(Delivery::parse)
                .ok_or_else(|| anyhow::anyhow!("--notify takes {}\n\n{USAGE}", Delivery::NAMES))
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
                "--notify" => notify = Some(how(args.next().as_deref())?),
                "chat" if chat.is_none() => chat = Some(None),
                "-c" | "--continue" if chat.is_some() => start = chat::Start::Continue,
                "-r" | "--resume" if chat.is_some() => start = chat::Start::Pick,
                "--task" if chat.is_some() => match args.next() {
                    Some(id) => task = Some(id),
                    None => bail!("--task needs a task's id\n\n{USAGE}"),
                },
                "--inspect" if chat.is_some() => inspect = true,
                other if matches!(chat, Some(None)) && !other.starts_with('-') => {
                    chat = Some(Some(other.to_string()));
                }
                other => {
                    if let Some(p) = other.strip_prefix("--socket=") {
                        socket = p.to_string();
                    } else if let Some(v) = other.strip_prefix("--notify=") {
                        notify = Some(how(Some(v))?);
                    } else {
                        bail!("unknown argument `{other}`\n\n{USAGE}");
                    }
                }
            }
        }
        if inspect && task.is_none() {
            bail!("--inspect goes with --task ID\n\n{USAGE}");
        }
        Ok(Some(Self {
            socket,
            notify,
            chat,
            start,
            task: task.map(|id| (id, !inspect)),
        }))
    }
}

fn main() -> Result<()> {
    let Some(args) = Args::parse(std::env::args().skip(1))? else {
        return Ok(());
    };
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    if let Some(resume) = args.chat {
        let chat = chat::Args {
            socket: args.socket,
            resume,
            start: args.start,
            task: args.task,
        };
        return rt.block_on(chat::run(chat));
    }
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
    // The thread this terminal chose (theseus-j0yh): the environment's, else the one it kept.
    app.thread = theseus_client::current::of_terminal();
    app.thread_file =
        theseus_client::current::default_path().map(|p| (p, theseus_client::current::terminal()));
    let signals = ending_signals()?;
    // Raw mode and the alternate screen, put back on exit and on a panic;
    // the loop turns on focus events and bracketed paste, and off again
    // (`term`), and a panic turns them off.
    let terminal = ratatui::try_init()?;
    term::on_panic(|| Box::new(std::io::stdout()));
    let mut runner = Runner::new(app, terminal, connect, rx, now_unix_ms);
    runner.out = Box::new(std::io::stdout());
    runner.delivery = args.notify.unwrap_or_default();
    runner.app.notify_pinned = args.notify.is_some();
    runner.app.quiet = args.notify == Some(Delivery::Off);
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

/// The signals that end the loop as a quit does, so the terminal is put back:
/// SIGTERM and SIGHUP (theseus-8hcg), and SIGINT and SIGQUIT (its review,
/// finding 2). Raw mode reads ctrl-c and ctrl-\\ as keys, so these two come
/// from `kill` or a parent; by default they end the process with the terminal
/// still raw, SIGQUIT with a core.
fn ending_signals() -> std::io::Result<tokio::sync::mpsc::UnboundedReceiver<()>> {
    let (end, signals) = tokio::sync::mpsc::unbounded_channel();
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sighup = signal(SignalKind::hangup())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigquit = signal(SignalKind::quit())?;
    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => {}
            _ = sighup.recv() => {}
            _ = sigint.recv() => {}
            _ = sigquit.recv() => {}
        }
        let _ = end.send(());
    });
    Ok(signals)
}

/// A time of day on this machine's clock, `15:42`, as the daemon writes its
/// labels' times (`sleeping until 16:00`), and as every time the TUI and the
/// CLI show is written (theseus-0n1v).
fn local_hm(unix_ms: u64) -> String {
    theseus_client::render::time::fmt_hm(unix_ms)
}

#[cfg(test)]
mod args_tests {
    use super::Args;

    fn parse(words: &[&str]) -> anyhow::Result<Option<Args>> {
        Args::parse(words.iter().map(|w| (*w).to_string()))
    }

    #[test]
    fn chat_task_joins_and_with_inspect_only_reads() {
        let a = parse(&["chat", "--task", "a1b2c3"]).unwrap().unwrap();
        assert_eq!(a.task, Some(("a1b2c3".to_string(), true)));
        let a = parse(&["chat", "--task", "a1b2c3", "--inspect"])
            .unwrap()
            .unwrap();
        assert_eq!(a.task, Some(("a1b2c3".to_string(), false)));
        assert!(parse(&["chat"]).unwrap().unwrap().task.is_none());
        assert!(
            parse(&["chat", "--inspect"]).is_err(),
            "--inspect needs --task"
        );
        assert!(parse(&["chat", "--task"]).is_err(), "--task needs an id");
    }
}
