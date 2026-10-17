//! The chat (theseus-cyha): bare `theseus` on a terminal, and `theseus chat
//! [SID]`, open an inline conversation in this directory. A second mode of
//! this binary, started by `theseus-tui chat`, which the CLI execs as it does
//! `theseus tui`: no binary of its own.
//!
//! It is inline and append-only: finished lines go once into the terminal's
//! own scrollback, so search, copy and scrolling work as they always do, and
//! only the dock at the bottom (the live line, the input, the status line) is
//! redrawn. Nothing above the dock is ever redrawn; nothing is lost on exit.
//!
//! - `state.rs`, `events.rs`: the state and every change to it. No I/O.
//! - `keys.rs`: the one key map. `commands.rs`: the one command registry.
//! - `dock.rs`: the dock's rows, as providers a later row adds to.
//! - `render_seam.rs`: the one place a reply and a tool call are worded.
//! - `editor.rs`: the multi-line input, its history and the folded paste.
//! - `screen.rs`: the bytes. `run.rs`: the loop. `guard.rs`: the terminal's
//!   modes, put back on every way out.

mod ask;
mod banner;
mod commands;
mod diff;
mod dock;
mod editor;
mod env;
mod events;
mod footer;
mod guard;
mod keys;
mod live;
mod picker;
mod queue;
mod render_seam;
mod reopen;
mod rewind;
mod run;
mod screen;
mod state;
mod stop;
mod style;
mod tasks;
mod usage;
mod welcome;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_ask;
#[cfg(test)]
mod tests_mode;
#[cfg(test)]
mod tests_reopen;
#[cfg(test)]
mod tests_rewind;
#[cfg(test)]
mod tests_tasks;
#[cfg(test)]
mod tests_usage;
#[cfg(test)]
mod tests_welcome;

use anyhow::{anyhow, Result};
use theseus_client::Conn;

use self::run::{Connector, Runner};
pub use self::state::Start;
use self::state::{Cfg, Chat};

/// What `theseus-tui chat` is given.
pub struct Args {
    pub socket: String,
    /// A conversation to go on with.
    pub resume: Option<String>,
    /// With none named: a new conversation, the last here, or a pick.
    pub start: Start,
}

/// Run the chat on the terminal in front of us.
pub async fn run(args: Args) -> Result<()> {
    let socket = args.socket.clone();
    // The first connect fails fast and before the terminal is touched: a
    // daemon that is not there is told in a line of the shell's.
    let conn = match Conn::socket_within(&socket, Some(std::time::Duration::from_secs(2))).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "theseus-tui: cannot reach theseusd at {socket}: {}\nIs it running? Start it with `theseusd`.",
                e.root_cause()
            );
            std::process::exit(3);
        }
    };
    let connect: Connector = Box::new(move || {
        let path = socket.clone();
        Box::pin(async move { Conn::socket(&path).await })
    });

    let dir =
        std::env::current_dir().map_err(|e| anyhow!("cannot read the current directory: {e}"))?;
    let dir_text = dir.to_string_lossy().into_owned();
    let home = std::env::var("HOME").ok();
    let history_path = env::history_path();
    let history = history_path
        .as_deref()
        .map(|p| env::load_history(p, &dir_text))
        .unwrap_or_default();
    let cfg = Cfg {
        shown_dir: env::shown_dir(&dir_text, home.as_deref()),
        branch: env::git_branch(&dir),
        dir: dir_text,
        resume: args.resume,
        start: args.start,
        color: style::color_wanted(),
        history,
        history_path,
        seen_path: theseus_client::seen::Seen::default_path(),
    };
    let (w, h) = crossterm::terminal::size().unwrap_or((80, 24));
    let mut chat = Chat::new(cfg, crate::now_ms());
    chat.resized(w, h);
    chat.job_session = theseus_client::client::job_session();

    // The terminal's events, read on a thread of their own: a blocking read
    // wakes nothing while the person types nothing.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(e) = crossterm::event::read() {
            if tx.send(e).is_err() {
                break;
            }
        }
    });
    let signals = crate::ending_signals()?;
    let mut guard = guard::Guard::enter()?;
    let mut runner = Runner::new(chat, Some(conn), connect, rx, crate::now_ms);
    runner.out = Box::new(std::io::stdout());
    runner.signals = Some(signals);
    let ran = runner.run().await;
    guard.restore();
    ran
}
