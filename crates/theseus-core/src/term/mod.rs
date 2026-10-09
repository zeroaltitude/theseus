//! Terminals (theseus-n88g.4, B4): a program on a pty, driven by the model
//! with keys and read as a screen. `proc.run` takes a typed argv with no pty
//! and no stdin, so a REPL, an editor, or a program that asks for a password
//! cannot be driven through it; these can.
//!
//! - **The tools** (`tools.rs`): `term.open` starts a program on a new pty
//!   and gives its terminal's id; `term.send` types keys into it (text, and
//!   named keys: Enter, Tab, Ctrl-C, the arrows, Escape); `term.read` gives
//!   its screen as text, its cursor, and what changed since the last read,
//!   optionally once the screen is quiet or a text appears, bounded; and
//!   `term.close` ends it. Each runs as an async tool (DD5): its waits are a
//!   task's, never a worker's.
//! - **The gate.** `term.open` is a Run-class act, judged as `proc.run` is,
//!   by its program and arguments (its plan names its argv and its working
//!   directory). `term.send` counts as running the terminal's program: its
//!   plan names that argv, so the floor, the approve and allow lists, and the
//!   external-text hold judge it as they would the program's own run.
//!   `term.read` is a read. `term.close` is a read too: it stops what this
//!   session started, as a cancel does, and so never waits on a hold.
//! - **External text.** A terminal whose program `[policy] external_programs`
//!   lists (`external::Listed`, read from its argv as a `proc.run`'s is), or
//!   one sent text that names a listed program (`gh` typed into a shell), is
//!   outside text from then on: every screen it gives is marked, and holds
//!   its session (`via: program`).
//! - **Lifetime.** At most `PER_SESSION` terminals a session whose programs
//!   run (theseus-ggqf): one whose program has ended frees its slot, and its
//!   last screen stays readable until `term.close`, or until an open finds
//!   the session full and reclaims the oldest ended one, which its result
//!   names. Each is closed by `term.close`, at its session's end (its
//!   execution ends: a task that reported, a failure, a spent budget), at a
//!   cancel or a `/stop` of its execution, and at the daemon's stop.
//!   Terminals live in memory: a daemon that dies takes their ptys with it,
//!   and the kernel hangs up each program's session as its master closes.
//!   Their rows (`term.opened`, `term.closed`, `term.left`) are the record.
//! - **What a close leaves** (theseus-ggqf, `left.rs`): at a session's end
//!   and the daemon's stop, under `[tools.term] keep_background` (the
//!   default), a close ends the program and the pty's foreground process
//!   group and leaves what ran in the background, as `proc.run` does.
//!   `term.close`, a cancel and a `/stop` end everything, and a cancel or a
//!   `/stop` ends what its session's and its tasks' closes left too
//!   (`Core::end_terminals_left`).
//! - **Waiting for a typed command** (`term.read`'s `until_idle`): the
//!   program back in front of its pty (`Pty::idle`, `tcgetpgrp` on the
//!   master), looked at every `IDLE_LOOK`, never sooner than `IDLE_SETTLE`
//!   after the last keys were sent.
//! - **Surfaces.** A line per terminal in health (`terminals`), the
//!   narrative's lines, the ledger's rows, and the tool lines, whose subject
//!   is the program (`term.send python3`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};
use theseus_tools::{AsyncRun, ToolCtx, ToolFailure, ToolOutput};

pub mod keys;
pub mod left;
pub mod pty;
pub mod tools;
pub mod vt;

#[cfg(test)]
mod bench;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_idle;
#[cfg(test)]
mod tests_keep;
#[cfg(test)]
mod tests_slots;

/// The tools' family, and their names: the one place they are spelt.
pub const FAMILY: &str = "term";
pub const OPEN: &str = "term.open";
pub const SEND: &str = "term.send";
pub const READ: &str = "term.read";
pub const CLOSE: &str = "term.close";
/// Every terminal tool, for the config's check of `[policy.tools]`.
pub const NAMES: [&str; 4] = [OPEN, SEND, READ, CLOSE];

/// The most terminals a session holds whose programs run.
pub const PER_SESSION: usize = 4;

/// How long a close waits for a program to go after its hang-up and
/// SIGTERM, before SIGKILL.
pub const CLOSE_GRACE: Duration = Duration::from_millis(500);

/// The longest a read (or an open's first screen) waits; `until_idle` alone
/// may wait up to `[tools] proc_sync_secs` when that is longer.
pub const MAX_WAIT_MS: u64 = 60_000;

/// How often a wait `until_idle` looks at the pty's foreground group.
pub const IDLE_LOOK: Duration = Duration::from_millis(40);

/// How long after keys are sent before the program in front counts as
/// idle: a shell reads a typed line, then puts its command in front, and
/// between the two it is still in front itself.
pub const IDLE_SETTLE: Duration = Duration::from_millis(100);

/// Why a terminal closed, as its row and its line say.
pub const BY_TOOL: &str = "term.close";
pub const BY_SESSION_END: &str = "its session ended";
pub const BY_CANCEL: &str = "its execution was cancelled";
pub const BY_STOP: &str = "its conversation was stopped";
pub const BY_DAEMON: &str = "the daemon stopped";
pub const BY_RECLAIM: &str = "term.open reclaimed its slot";

/// One terminal: its program on its pty, and what its last read saw.
pub struct Terminal {
    pub id: String,
    pub session: String,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub opened_ms: u64,
    pub pty: pty::Pty,
    /// The listed program that makes its screen outside text, once one is
    /// known: its argv's, or one its keys named.
    external: Mutex<Option<crate::external::Listed>>,
    /// The rows and the scroll count its last read returned.
    last: Mutex<(Vec<String>, u64)>,
    closed: AtomicBool,
    /// When keys were last sent to it, in ms since the epoch.
    sent_ms: AtomicU64,
}

impl Terminal {
    /// The program, by its file name.
    pub fn program(&self) -> String {
        program(&self.argv)
    }

    pub fn external(&self) -> Option<crate::external::Listed> {
        self.external.lock().unwrap().clone()
    }

    /// Its program has ended: it holds no slot.
    pub fn ended(&self) -> bool {
        self.pty.exited().is_some()
    }

    /// Its program is in front of its pty, and no keys were sent to it in
    /// the last `IDLE_SETTLE` (`until_idle`).
    pub fn idle(&self) -> bool {
        let since =
            theseus_protocol::now_unix_ms().saturating_sub(self.sent_ms.load(Ordering::Relaxed));
        since >= IDLE_SETTLE.as_millis() as u64 && self.pty.idle()
    }

    /// What holds its pty's front now, for a wait that ended busy: the
    /// foreground group's leader, by its name.
    pub fn in_front(&self) -> Option<String> {
        let g = self.pty.foreground()?;
        std::fs::read_to_string(format!("/proc/{g}/comm"))
            .ok()
            .map(|c| c.trim_end().to_string())
    }

    /// Health's line for it.
    pub fn info(&self) -> theseus_protocol::TerminalInfo {
        let s = &self.pty.shared;
        let (rows, cols) = s.screen.lock().unwrap().size();
        theseus_protocol::TerminalInfo {
            id: self.id.clone(),
            session_id: self.session.clone(),
            program: self.program(),
            pid: self.pty.pid(),
            rows,
            cols,
            opened_at_unix_ms: self.opened_ms,
            running: self.pty.exited().is_none(),
            external: self.external().map(|l| l.program),
            bytes_out: s.bytes_out.load(Ordering::Relaxed),
            bytes_in: s.bytes_in.load(Ordering::Relaxed),
            last_output_unix_ms: s.last_output_ms.load(Ordering::Relaxed),
        }
    }
}

/// A program's file name, as its lines name it.
pub fn program(argv: &[String]) -> String {
    argv.first()
        .map(|a| {
            std::path::Path::new(a)
                .file_name()
                .map_or_else(|| a.clone(), |f| f.to_string_lossy().into_owned())
        })
        .unwrap_or_default()
}

/// How a terminal ended: what its `term.closed` row says.
#[derive(Debug, Clone)]
pub struct Closed {
    pub id: String,
    pub session: String,
    pub argv: Vec<String>,
    pub by: String,
    /// Its exit code, or the signal that ended it.
    pub exit: Option<i32>,
    pub signal: Option<i32>,
    /// Processes the close had to kill after its grace.
    pub killed: usize,
    /// What it left running (theseus-ggqf).
    pub left: Vec<pty::Kept>,
    pub open_ms: u64,
    pub bytes_out: u64,
    pub bytes_in: u64,
}

impl Closed {
    /// How it ended, in words: "exit 0", "signal 1", "ended".
    pub fn how(&self) -> String {
        match (self.exit, self.signal) {
            (Some(c), _) => format!("exit {c}"),
            (None, Some(s)) => format!("signal {s}"),
            _ => "ended".into(),
        }
    }

    pub fn meta(&self) -> Value {
        let mut m = json!({"terminal": self.id, "program": program(&self.argv), "argv": self.argv,
            "by": self.by, "exit": self.exit, "signal": self.signal, "killed": self.killed,
            "open_ms": self.open_ms, "bytes_out": self.bytes_out, "bytes_in": self.bytes_in});
        if !self.left.is_empty() {
            m["left"] = left_json(&self.left);
        }
        m
    }
}

/// Processes left, as a row names them.
pub fn left_json(left: &[pty::Kept]) -> Value {
    left.iter()
        .map(|k| json!({"pid": k.proc.pid, "program": k.program, "why": k.why}))
        .collect()
}

/// Every open terminal, by id.
pub struct Terms {
    map: Mutex<BTreeMap<String, Arc<Terminal>>>,
    next: AtomicU64,
    /// The environment each program gets: the job environment (`[tools]
    /// proc_env`, resolved at the start), its session, and `TERM`.
    pub env: Vec<(String, String)>,
    /// `[policy] external_programs`.
    pub external_programs: Vec<String>,
    /// `[tools.term] keep_background` (theseus-ggqf).
    pub keep_background: bool,
    /// The longest `until_idle` alone waits: `[tools] proc_sync_secs`, or
    /// `MAX_WAIT_MS` when that is longer.
    pub idle_max_ms: u64,
    /// What closes left running.
    pub left: left::Book,
}

/// The terminal type a program is told it has: the subset `vt` models.
pub const TERM: &str = "xterm";

impl Terms {
    pub fn new(env: Vec<(String, String)>, external_programs: Vec<String>) -> Self {
        Self {
            map: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(1),
            env,
            external_programs,
            keep_background: crate::config::term::KEEP_BACKGROUND,
            idle_max_ms: MAX_WAIT_MS,
            left: left::Book::default(),
        }
    }

    /// The config's terminal settings.
    pub fn configured(mut self, keep_background: bool, proc_sync_secs: u64) -> Self {
        self.keep_background = keep_background;
        self.idle_max_ms = MAX_WAIT_MS.max(proc_sync_secs.saturating_mul(1000));
        self
    }

    /// What closes left that still runs, for health.
    pub fn left_info(&self) -> Vec<theseus_protocol::TerminalLeft> {
        self.left.live().iter().map(left::Left::info).collect()
    }

    /// A terminal by id.
    pub fn get(&self, id: &str) -> Option<Arc<Terminal>> {
        self.map.lock().unwrap().get(id).cloned()
    }

    /// A session's terminal by id, or why there is none.
    fn of(&self, session: &str, id: &str) -> Result<Arc<Terminal>, String> {
        self.get(id)
            .filter(|t| t.session == session)
            .ok_or_else(|| format!("this session has no terminal {id}; term_open starts one"))
    }

    /// Every open terminal, oldest first, for health.
    pub fn all(&self) -> Vec<Arc<Terminal>> {
        let mut v: Vec<_> = self.map.lock().unwrap().values().cloned().collect();
        v.sort_by_key(|t| (t.opened_ms, t.id.clone()));
        v
    }

    /// A session's open terminals.
    pub fn of_session(&self, session: &str) -> Vec<Arc<Terminal>> {
        self.all()
            .into_iter()
            .filter(|t| t.session == session)
            .collect()
    }

    /// Start `argv` on a new pty for `session`.
    pub fn open(
        &self,
        session: &str,
        argv: &[String],
        cwd: PathBuf,
        (rows, cols): (u16, u16),
        umask: Option<u32>,
    ) -> Result<(Arc<Terminal>, Option<Closed>), String> {
        let mut map = self.map.lock().unwrap();
        let mut held: Vec<&Arc<Terminal>> = map.values().filter(|t| t.session == session).collect();
        held.sort_by_key(|t| (t.opened_ms, t.id.clone()));
        let running = held.iter().filter(|t| !t.ended()).count();
        if running >= PER_SESSION {
            return Err(format!(
                "this session has {running} terminals running, the most it may: close one first \
                 (term_close)"
            ));
        }
        // Full of running terminals and ended ones: the oldest ended one
        // gives its slot back (theseus-ggqf).
        let reclaim = (held.len() >= PER_SESSION)
            .then(|| held.iter().find(|t| t.ended()).map(|t| (*t).clone()))
            .flatten();
        if !cwd.is_dir() {
            return Err(format!(
                "working directory {} does not exist",
                cwd.display()
            ));
        }
        let mut env = self.env.clone();
        for (k, v) in [(theseus_protocol::JOB_SESSION_ENV, session), ("TERM", TERM)] {
            env.retain(|(ek, _)| ek != k);
            env.push((k.into(), v.into()));
        }
        let pty = pty::Pty::spawn(&pty::Spawn {
            argv,
            cwd: &cwd,
            env: &env,
            rows,
            cols,
            umask,
        })
        .map_err(|e| format!("could not start {}: {e}", program(argv)))?;
        let id = format!("t{}", self.next.fetch_add(1, Ordering::Relaxed));
        let external =
            crate::external::Listed::of(&json!({ "argv": argv }), &self.external_programs);
        let t = Arc::new(Terminal {
            id: id.clone(),
            session: session.into(),
            argv: argv.to_vec(),
            cwd,
            opened_ms: theseus_protocol::now_unix_ms(),
            pty,
            external: Mutex::new(external),
            last: Mutex::new((Vec::new(), 0)),
            closed: AtomicBool::new(false),
            sent_ms: AtomicU64::new(0),
        });
        map.insert(id, t.clone());
        drop(map);
        // Its program has ended, so its close waits for nothing.
        let reclaimed = reclaim.and_then(|r| self.close_one(&r, BY_RECLAIM, CLOSE_GRACE));
        Ok((t, reclaimed))
    }

    /// Type `bytes` into a session's terminal; `text` is what of them was
    /// text, read for a listed program's name.
    pub fn send(
        &self,
        session: &str,
        id: &str,
        bytes: &[u8],
        text: &str,
    ) -> Result<Arc<Terminal>, String> {
        let t = self.of(session, id)?;
        if t.pty.shared.hung_up.load(Ordering::Relaxed) {
            return Err(format!(
                "terminal {id}'s program has ended ({}); term_read shows its last screen, \
                 and term_close frees it",
                t.program()
            ));
        }
        if !text.is_empty() {
            let mut ext = t.external.lock().unwrap();
            if ext.is_none() {
                *ext = crate::external::Listed::in_text(text, &self.external_programs);
            }
        }
        t.sent_ms
            .store(theseus_protocol::now_unix_ms(), Ordering::Relaxed);
        t.pty
            .send(bytes)
            .map_err(|e| format!("could not write to terminal {id}: {e}"))?;
        Ok(t)
    }

    /// Close one terminal: it leaves the map at once, and its program is
    /// stopped. At its session's end and the daemon's stop, under
    /// `keep_background`, what ran in the background is left, and booked.
    /// Blocks for up to the grace: call it off the workers.
    pub fn close_one(&self, t: &Terminal, by: &str, grace: Duration) -> Option<Closed> {
        if t.closed.swap(true, Ordering::AcqRel) {
            return None;
        }
        self.map.lock().unwrap().remove(&t.id);
        let keep = self.keep_background && matches!(by, BY_SESSION_END | BY_DAEMON);
        let pty::Ended {
            status,
            killed,
            left,
        } = t.pty.close(grace, keep);
        let now = theseus_protocol::now_unix_ms();
        if !left.is_empty() {
            let each: Vec<String> = left
                .iter()
                .map(|k| format!("{} ({}: {})", k.proc.pid, k.program, k.why))
                .collect();
            tracing::info!(terminal = %t.id, session = %t.session, by, left = %each.join(", "),
                "a terminal's close left processes running");
        }
        self.left.add(left.iter().map(|k| left::Left {
            session: t.session.clone(),
            terminal: t.id.clone(),
            terminal_program: t.program(),
            kept: k.clone(),
            since_ms: now,
        }));
        use std::os::unix::process::ExitStatusExt;
        let s = &t.pty.shared;
        Some(Closed {
            id: t.id.clone(),
            session: t.session.clone(),
            argv: t.argv.clone(),
            by: by.into(),
            exit: status.and_then(|s| s.code()),
            signal: status.and_then(|s| s.signal()),
            killed,
            left,
            open_ms: now.saturating_sub(t.opened_ms),
            bytes_out: s.bytes_out.load(Ordering::Relaxed),
            bytes_in: s.bytes_in.load(Ordering::Relaxed),
        })
    }

    /// Close every terminal `which` names, together: each is signalled at
    /// once, so the grace is waited once, not once a terminal.
    pub fn close_where(&self, by: &str, which: impl Fn(&Terminal) -> bool) -> Vec<Closed> {
        let ts: Vec<Arc<Terminal>> = self.all().into_iter().filter(|t| which(t)).collect();
        std::thread::scope(|s| {
            let hs: Vec<_> = ts
                .iter()
                .map(|t| s.spawn(|| self.close_one(t, by, CLOSE_GRACE)))
                .collect();
            hs.into_iter()
                .filter_map(|h| h.join().ok().flatten())
                .collect()
        })
    }

    /// Close a session's terminals.
    pub fn close_session(&self, session: &str, by: &str) -> Vec<Closed> {
        if !self
            .map
            .lock()
            .unwrap()
            .values()
            .any(|t| t.session == session)
        {
            return Vec::new();
        }
        self.close_where(by, |t| t.session == session)
    }

    /// A cancel's or a `/stop`'s end of what the session's earlier closes
    /// left running (`left::end`): each terminal's, with how many it had to
    /// kill. Blocks for up to the grace: call it off the workers.
    pub fn end_left(&self, session: &str) -> Vec<(String, String, Vec<pty::Kept>)> {
        let taken = self.left.take(session);
        if taken.is_empty() {
            return Vec::new();
        }
        left::end(&taken, CLOSE_GRACE);
        let mut by_terminal: BTreeMap<String, (String, Vec<pty::Kept>)> = BTreeMap::new();
        for l in taken {
            by_terminal
                .entry(l.terminal)
                .or_insert_with(|| (l.terminal_program, Vec::new()))
                .1
                .push(l.kept);
        }
        by_terminal
            .into_iter()
            .map(|(t, (p, k))| (t, p, k))
            .collect()
    }

    /// A session's terminals closed off the runtime's workers, with each
    /// recorded as `rec`'s fact.
    pub async fn close_session_recorded(
        self: &Arc<Self>,
        session: &str,
        by: &'static str,
        rec: &crate::fact::Rec<'_>,
    ) {
        if self.of_session(session).is_empty() {
            return;
        }
        let (terms, s) = (self.clone(), session.to_string());
        let closed = tokio::task::spawn_blocking(move || terms.close_session(&s, by))
            .await
            .unwrap_or_default();
        for c in &closed {
            record_closed(rec, c);
        }
    }

    /// The run of a `term.*` call for `session` (`toolrun`'s async path):
    /// each tool's run needs its session, which a toollet's context lacks.
    pub fn run(
        self: &Arc<Self>,
        tool: &str,
        session: &str,
        input: &Value,
        ctx: &ToolCtx,
    ) -> AsyncRun {
        let (terms, session, input, ctx, tool) = (
            self.clone(),
            session.to_string(),
            input.clone(),
            ctx.clone(),
            tool.to_string(),
        );
        Box::pin(async move { tools::run(&terms, &tool, &session, &input, &ctx).await })
    }
}

impl crate::rpc::Core {
    /// A cancel's or a `/stop`'s end of what terminals' closes left running
    /// (theseus-ggqf): its execution's session's, and its tasks' (whose
    /// sessions ended with them, which is when their terminals left
    /// something), off the runtime's workers, each recorded in its session.
    pub async fn end_terminals_left(&self, execution_id: &str, by: &'static str) {
        let terms = self.tools.terms.clone();
        if terms.left.live().is_empty() {
            return;
        }
        let mut sessions = Vec::new();
        let mut under = vec![execution_id.to_string()];
        while let Some(id) = under.pop() {
            if let Ok(Some(e)) = self.kernel.execution(&id) {
                sessions.push(e.session_id);
            }
            under.extend(
                self.kernel
                    .tasks(Some(&id))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.id),
            );
        }
        sessions.retain(|s| terms.left.holds(s));
        let ended = tokio::task::spawn_blocking(move || {
            sessions
                .into_iter()
                .map(|s| (terms.end_left(&s), s))
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (each, session) in &ended {
            for (terminal, program, procs) in each {
                self.session_rec(session)
                    .record(&crate::fact::term::TermLeft {
                        terminal,
                        program,
                        by,
                        procs,
                        ended: true,
                    });
            }
        }
    }

    /// At the daemon's stop: every terminal closed, off the runtime's
    /// workers, each recorded in its own session.
    pub async fn close_terminals(&self) {
        let terms = self.tools.terms.clone();
        if terms.all().is_empty() {
            return;
        }
        let closed = tokio::task::spawn_blocking(move || terms.close_where(BY_DAEMON, |_| true))
            .await
            .unwrap_or_default();
        for c in &closed {
            record_closed(&self.session_rec(&c.session), c);
        }
    }
}

/// A close's facts: its `term.closed`, and its `term.left` when it left
/// something running.
pub fn record_closed(rec: &crate::fact::Rec<'_>, c: &Closed) {
    rec.record(&crate::fact::term::TermClosed { closed: c });
    if !c.left.is_empty() {
        rec.record(&crate::fact::term::TermLeft {
            terminal: &c.id,
            program: &program(&c.argv),
            by: &c.by,
            procs: &c.left,
            ended: false,
        });
    }
}

impl Drop for Terms {
    fn drop(&mut self) {
        // A daemon's stop closes its terminals and records it first; this
        // is the backstop for a core dropped without it (tests).
        let ts: Vec<_> = std::mem::take(&mut *self.map.lock().unwrap())
            .into_values()
            .collect();
        for t in ts {
            t.closed.store(true, Ordering::Release);
            t.pty.close(Duration::ZERO, false);
        }
    }
}

/// A screen as a read gives it: the rows, the cursor, and what changed
/// since this terminal's last read, which it then becomes.
pub fn snapshot(
    t: &Terminal,
    waited: Option<String>,
) -> Result<(ToolOutput, Option<theseus_tools::External>), ToolFailure> {
    let (rows, (crow, ccol), scrolled, back, size, alt) = {
        let s = t.pty.shared.screen.lock().unwrap();
        let back = s.scrollback_tail(vt::SCROLLBACK);
        (
            s.rows(),
            s.cursor(),
            s.scrolled(),
            back,
            s.size(),
            s.alternate(),
        )
    };
    let mut last = t.last.lock().unwrap();
    let state = match t.pty.exited() {
        None if t.pty.shared.hung_up.load(Ordering::Relaxed) => "its program has ended".to_string(),
        None => "running".to_string(),
        Some(st) => {
            use std::os::unix::process::ExitStatusExt;
            match (st.code(), st.signal()) {
                (Some(c), _) => format!("its program exited ({c})"),
                (_, Some(s)) => format!("its program was ended by signal {s}"),
                _ => "its program has ended".to_string(),
            }
        }
    };
    let mut text = String::new();
    let ext = t.external();
    if let Some(l) = &ext {
        text.push_str(&l.line());
    }
    text.push_str(&format!(
        "Terminal {} ({}), {}x{}, cursor at row {}, column {}{}; {state}.",
        t.id,
        t.program(),
        size.0,
        size.1,
        crow + 1,
        ccol + 1,
        if alt { ", full-screen" } else { "" },
    ));
    if let Some(w) = &waited {
        text.push_str(&format!(" {w}."));
    }
    text.push('\n');
    // Lines that scrolled off the top since the last read: the part of a
    // long output the screen no longer shows.
    let off = scrolled.saturating_sub(last.1);
    if off > 0 {
        let shown = (off as usize).min(back.len());
        text.push_str(&format!(
            "Scrolled off the top since the last read: {off} line{}{}:\n",
            if off == 1 { "" } else { "s" },
            if shown < off as usize {
                format!(", the last {shown} kept")
            } else {
                String::new()
            },
        ));
        for l in &back[back.len() - shown..] {
            text.push_str(l);
            text.push('\n');
        }
    }
    let used = rows
        .iter()
        .rposition(|r| !r.is_empty())
        .map_or(0, |i| i + 1)
        .max(crow + 1);
    text.push_str(&match used == rows.len() {
        true => "Screen:\n".to_string(),
        false => format!("Screen (rows {}-{} are blank):\n", used + 1, rows.len()),
    });
    let width = rows.len().to_string().len();
    for (i, r) in rows.iter().take(used).enumerate() {
        text.push_str(&format!("{:>width$}|{r}\n", i + 1));
    }
    let changed: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            last.0.get(i).map(String::as_str).unwrap_or("") != rows[i]
                || (last.0.is_empty() && !rows[i].is_empty())
        })
        .collect();
    text.push_str(&match (last.0.is_empty(), changed.is_empty()) {
        (true, _) => "This is its first read.".to_string(),
        (false, true) => "Nothing changed since the last read.".to_string(),
        (false, false) => format!("Changed since the last read: {}.", ranges(&changed)),
    });
    *last = (rows, scrolled);
    let meta = json!({"terminal": t.id, "program": t.program(), "cursor": [crow + 1, ccol + 1],
        "changed_rows": changed.iter().map(|i| i + 1).collect::<Vec<_>>(), "scrolled_off": off,
        crate::external::PROGRAM_KEY: ext.as_ref().map(|l| l.program.clone())});
    let mut meta = meta;
    if ext.is_none() {
        meta.as_object_mut()
            .map(|m| m.remove(crate::external::PROGRAM_KEY));
    }
    Ok((ToolOutput { text, meta }, ext.map(|l| l.marker())))
}

/// Row numbers as runs: "rows 2-4, 7".
fn ranges(rows: &[usize]) -> String {
    let mut parts = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let mut j = i;
        while j + 1 < rows.len() && rows[j + 1] == rows[j] + 1 {
            j += 1;
        }
        parts.push(match i == j {
            true => format!("{}", rows[i] + 1),
            false => format!("{}-{}", rows[i] + 1, rows[j] + 1),
        });
        i = j + 1;
    }
    format!(
        "row{} {}",
        if rows.len() == 1 { "" } else { "s" },
        parts.join(", ")
    )
}
