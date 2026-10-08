//! What terminals' closes left running (theseus-ggqf). A close at a
//! session's end or at the daemon's stop, under `[tools.term]
//! keep_background`, ends the terminal's program and its foreground process
//! group and leaves the rest: a shell's `&` jobs, `nohup` and `setsid`
//! children, a server that daemonized, and anything outside the tree that
//! holds the pty. That is `proc.run`'s rule: its wrapper kills nothing a
//! job leaves in the background.
//!
//! - **Reaped.** A process left was the program's, or its children's; as its
//!   parent ends it is reparented to the daemon, a child subreaper, whose
//!   sweep reaps it as an orphan once it exits (theseusd's `reap_children`,
//!   theseus-kernel's `children`), so it leaves no zombie.
//! - **Listed.** Health's `terminals_left`, while each still runs; the close's
//!   `term.left` row and line name each one, with why it was left.
//! - **Ended by a cancel or a `/stop`** of its session's execution, or of the
//!   execution whose task's session it was (`Core::end_terminals_left`), as
//!   the terminals themselves are: its tree by theseus-kernel's `tree::stop`, the
//!   freeze and kill a job's stop uses, and the process itself after its
//!   grace. A process left at the daemon's stop is the next daemon's orphan
//!   only after an exec in place; a fresh daemon does not know it.
//! - **The pty.** A background job that still holds the pty's slave keeps
//!   the master open while it runs: the reader thread drains what it writes,
//!   so its writes never block and never fail at a session's end. At the
//!   daemon's stop the master closes with the daemon: the kernel hangs up
//!   only the session's foreground group, so a job left gets EIO on the pty
//!   and runs on (a `nohup` job writes elsewhere).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use theseus_kernel::tree;

use super::pty::Kept;

/// A process a close left, with what started it.
#[derive(Debug, Clone)]
pub struct Left {
    pub session: String,
    pub terminal: String,
    pub terminal_program: String,
    pub kept: Kept,
    pub since_ms: u64,
}

impl Left {
    pub fn info(&self) -> theseus_protocol::TerminalLeft {
        theseus_protocol::TerminalLeft {
            pid: self.kept.proc.pid,
            program: self.kept.program.clone(),
            why: self.kept.why.into(),
            terminal: self.terminal.clone(),
            terminal_program: self.terminal_program.clone(),
            session_id: self.session.clone(),
            left_at_unix_ms: self.since_ms,
        }
    }
}

/// Every process left that may still run.
#[derive(Default)]
pub struct Book(Mutex<Vec<Left>>);

impl Book {
    pub fn add(&self, left: impl IntoIterator<Item = Left>) {
        self.0.lock().unwrap().extend(left);
    }

    /// Those that still run, oldest first; the gone are forgotten.
    pub fn live(&self) -> Vec<Left> {
        let mut v = self.0.lock().unwrap();
        v.retain(|l| tree::alive(l.kept.proc));
        v.clone()
    }

    /// Whether `session` has a process left that still runs.
    pub fn holds(&self, session: &str) -> bool {
        self.live().iter().any(|l| l.session == session)
    }

    /// Take `session`'s, to end them.
    pub fn take(&self, session: &str) -> Vec<Left> {
        let mut v = self.0.lock().unwrap();
        let (mine, rest): (Vec<Left>, Vec<Left>) = std::mem::take(&mut *v)
            .into_iter()
            .partition(|l| l.session == session);
        *v = rest;
        mine.into_iter()
            .filter(|l| tree::alive(l.kept.proc))
            .collect()
    }
}

/// End each process `left` names, and its tree: SIGTERM to it and to its
/// descendants, up to `grace` for them to go, then the stop a job's tree
/// gets (`tree::stop`: the freeze, so nothing it forks escapes, and SIGKILL)
/// for what it started, and SIGKILL to it, held stopped meanwhile. Blocks:
/// call it off the runtime's workers. How many it had to kill.
pub fn end(left: &[Left], grace: Duration) -> usize {
    let roots: Vec<tree::Proc> = left.iter().map(|l| l.kept.proc).collect();
    for r in &roots {
        tree::signal(*r, libc::SIGTERM);
        for p in tree::descendants(r.pid) {
            tree::signal(p, libc::SIGTERM);
        }
    }
    let t0 = Instant::now();
    while t0.elapsed() < grace && roots.iter().any(|r| tree::alive(*r)) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let live: Vec<tree::Proc> = roots.into_iter().filter(|r| tree::alive(*r)).collect();
    std::thread::scope(|s| {
        for r in &live {
            s.spawn(move || {
                // Held stopped, so it starts nothing more while its tree is
                // stopped; a SIGKILL ends a stopped process all the same.
                tree::signal(*r, libc::SIGSTOP);
                tree::stop(r.pid, Duration::ZERO, &mut || tree::Left::Unknown);
                tree::signal(*r, libc::SIGKILL);
            });
        }
    });
    live.len()
}
