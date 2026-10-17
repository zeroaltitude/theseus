//! The chat's loop: one connection, the terminal's events and one deadline in
//! a `select!`, so the chat is quiet by construction. It draws on events and
//! on the dock's own clock while something runs, at most 30 times a second,
//! and runs no timer otherwise. A lost connection is tried again with
//! backoff, and the session watched again.

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::Event as TermEvent;
use theseus_client::Conn;
use theseus_protocol::{Id, Message};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use super::dock;
use super::env;
use super::screen::Screen;
use super::state::{Chat, Effect, Purpose};

/// How the loop gets a connection again: a socket in the binary, a scripted
/// daemon in tests.
pub type Connector = Box<dyn FnMut() -> Pin<Box<dyn Future<Output = Result<Conn>> + Send>> + Send>;

const RETRY_FIRST_MS: u64 = 500;
const RETRY_MAX_MS: u64 = 10_000;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// At most 30 frames a second.
pub const FRAME: Duration = Duration::from_millis(33);

enum Woke {
    Daemon(Result<Option<Message>>),
    Terminal(Option<TermEvent>),
    Deadline,
    Signal,
}

pub struct Runner {
    pub chat: Chat,
    conn: Option<Conn>,
    connect: Connector,
    events: UnboundedReceiver<TermEvent>,
    pending: HashMap<Id, Purpose>,
    attempt: u32,
    retry_at: Option<Instant>,
    screen: Screen,
    /// Where the terminal's bytes go: stdout in the binary, a buffer in tests.
    pub out: Box<dyn Write + Send>,
    /// The frame's interval: `FRAME`, or zero in tests.
    pub frame: Duration,
    last_draw: Option<Instant>,
    draw_at: Option<Instant>,
    dirty: bool,
    quit: bool,
    clock: fn() -> u64,
    pub signals: Option<UnboundedReceiver<()>>,
    /// Frames drawn, for a test that holds the redraw count.
    pub frames: u64,
}

impl Runner {
    pub fn new(
        chat: Chat,
        conn: Option<Conn>,
        connect: Connector,
        events: UnboundedReceiver<TermEvent>,
        clock: fn() -> u64,
    ) -> Self {
        Self {
            chat,
            conn,
            connect,
            events,
            pending: HashMap::new(),
            attempt: 0,
            retry_at: None,
            screen: Screen::default(),
            out: Box::new(std::io::sink()),
            frame: FRAME,
            last_draw: None,
            draw_at: None,
            dirty: true,
            quit: false,
            clock,
            signals: None,
            frames: 0,
        }
    }

    #[cfg(test)]
    pub fn quitting(&self) -> bool {
        self.quit
    }

    /// Run until the person leaves. The first frame is drawn before anything
    /// is asked of the daemon, so the prompt is ready at once.
    pub async fn run(&mut self) -> Result<()> {
        self.start().await;
        while !self.quit {
            self.step().await?;
        }
        self.finish();
        Ok(())
    }

    /// The first frame, then the first questions to the daemon.
    pub async fn start(&mut self) {
        self.chat.now_ms = (self.clock)();
        self.draw();
        if self.conn.is_some() {
            let effects = self.chat.connected((self.clock)());
            self.apply(effects).await;
        } else {
            self.retry_at = Some(Instant::now());
        }
    }

    /// One wake: a message, a key, a deadline or a signal; then a draw if one
    /// is due.
    pub async fn step(&mut self) -> Result<()> {
        let deadline = self.deadline();
        let woke = {
            let conn = &mut self.conn;
            let events = &mut self.events;
            let signals = &mut self.signals;
            tokio::select! {
                m = async {
                    match conn {
                        Some(c) => c.next().await,
                        None => std::future::pending().await,
                    }
                } => Woke::Daemon(m),
                e = events.recv() => Woke::Terminal(e),
                _ = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => Woke::Deadline,
                () = async {
                    let got = match signals {
                        Some(rx) => rx.recv().await,
                        None => None,
                    };
                    if got.is_none() {
                        std::future::pending::<()>().await;
                    }
                } => Woke::Signal,
            }
        };
        let now = (self.clock)();
        self.chat.now_ms = now;
        match woke {
            Woke::Daemon(Ok(Some(msg))) => self.message(msg).await,
            Woke::Daemon(Ok(None) | Err(_)) => self.lost_connection(),
            Woke::Terminal(Some(e)) => self.terminal(e).await,
            // The terminal's reader ended, or a signal came: leave as a quit does.
            Woke::Terminal(None) | Woke::Signal => {
                let effects = self.chat.leave();
                self.apply(effects).await;
            }
            Woke::Deadline => self.deadlines().await,
        }
        self.maybe_draw();
        Ok(())
    }

    /// The soonest of: the retry, the next frame, and what the chat asks for.
    fn deadline(&self) -> Option<Instant> {
        let now_ms = (self.clock)();
        let chat = self
            .chat
            .deadline()
            .map(|at| Instant::now() + Duration::from_millis(at.saturating_sub(now_ms)));
        [self.retry_at, self.draw_at, chat]
            .into_iter()
            .flatten()
            .min()
    }

    async fn deadlines(&mut self) {
        let now = Instant::now();
        if self.retry_at.is_some_and(|at| at <= now) {
            self.retry_at = None;
            self.reconnect().await;
        }
        if self.draw_at.is_some_and(|at| at <= now) {
            self.draw_at = None;
            self.dirty = true;
        }
        if self.chat.tick((self.clock)()) {
            self.dirty = true;
        }
        // The banner or a notice ended: its lines are waiting.
        if !self.chat.out.is_empty() {
            self.dirty = true;
        }
    }

    async fn reconnect(&mut self) {
        let connecting = (self.connect)();
        match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
            Ok(Ok(conn)) => {
                self.conn = Some(conn);
                self.attempt = 0;
                let effects = self.chat.connected((self.clock)());
                self.apply(effects).await;
            }
            _ => self.schedule_retry(),
        }
        self.dirty = true;
    }

    fn lost_connection(&mut self) {
        self.conn = None;
        self.pending.clear();
        self.schedule_retry();
        self.dirty = true;
    }

    fn schedule_retry(&mut self) {
        self.attempt += 1;
        let wait = RETRY_FIRST_MS
            .saturating_mul(1u64 << (self.attempt - 1).min(16))
            .min(RETRY_MAX_MS);
        self.retry_at = Some(Instant::now() + Duration::from_millis(wait));
        self.chat.disconnected();
    }

    async fn message(&mut self, msg: Message) {
        let effects = match msg {
            Message::Notification(n) => self.chat.notified(&n.method, &n.params),
            Message::Response(r) => match self.pending.remove(&r.id) {
                Some(purpose) => {
                    let result = match r.error {
                        Some(e) => Err(e),
                        None => Ok(r.result.unwrap_or(serde_json::Value::Null)),
                    };
                    self.chat.answered(purpose, result)
                }
                None => Vec::new(),
            },
            Message::Request(_) => Vec::new(),
        };
        self.apply(effects).await;
        self.dirty = true;
    }

    async fn terminal(&mut self, e: TermEvent) {
        let effects = match e {
            TermEvent::Key(k) => self.chat.key(k),
            // One event, never keys (theseus-8hcg).
            TermEvent::Paste(text) => self.chat.paste(&text),
            TermEvent::Resize(w, h) => {
                self.chat.resized(w, h);
                Vec::new()
            }
            _ => return,
        };
        self.apply(effects).await;
        self.dirty = true;
    }

    async fn apply(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                Effect::Quit => self.quit = true,
                Effect::Bell => self.write(b"\x07"),
                Effect::Look(path, position, at_ms) => {
                    drop(tokio::task::spawn_blocking(move || {
                        let _ = theseus_client::seen::record_looked(&path, position, at_ms);
                    }));
                }
                Effect::SaveHistory(messages) => self.save_history(messages),
                Effect::Call(call) => {
                    let Some(conn) = self.conn.as_mut() else {
                        // Not connected: the answer to a send is lost, and the
                        // person is told.
                        if call.purpose == Purpose::Submit {
                            self.chat.flash("not connected · the message was not sent");
                        }
                        continue;
                    };
                    match conn.send(call.method, call.params).await {
                        Ok(id) => {
                            self.pending.insert(id, call.purpose);
                        }
                        Err(_) => self.lost_connection(),
                    }
                }
            }
        }
    }

    /// The directory's history, written off the loop's thread.
    fn save_history(&self, messages: Vec<String>) {
        let (Some(path), dir) = (
            self.chat.cfg.history_path.clone(),
            self.chat.cfg.dir.clone(),
        ) else {
            return;
        };
        drop(tokio::task::spawn_blocking(move || {
            env::save_history(&path, &dir, &messages)
        }));
    }

    fn write(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.out.write_all(bytes).and_then(|()| self.out.flush());
        }
    }

    /// Draw if something changed and a frame is due; else ask for a deadline
    /// at the next frame.
    fn maybe_draw(&mut self) {
        if !self.dirty && self.chat.out.is_empty() {
            return;
        }
        let now = Instant::now();
        match self.last_draw {
            Some(last) if now < last + self.frame => self.draw_at = Some(last + self.frame),
            _ => self.draw(),
        }
    }

    fn draw(&mut self) {
        let width = usize::from(self.chat.width);
        let d = dock::build(&self.chat);
        let scroll = std::mem::take(&mut self.chat.out);
        let bytes = self.screen.frame(&scroll, &d, width, self.chat.cfg.color);
        self.write(&bytes);
        self.last_draw = Some(Instant::now());
        self.draw_at = None;
        self.dirty = false;
        self.frames += 1;
    }

    /// On the way out: the dock comes off, the last lines are written, and
    /// the cursor is left at the start of a fresh line.
    fn finish(&mut self) {
        // The last board this chat showed is the look the next start tells from.
        if let Some(Effect::Look(path, position, at_ms)) =
            self.chat.look(self.chat.board_position()).pop()
        {
            let _ = theseus_client::seen::record_looked(&path, position, at_ms);
        }
        let width = usize::from(self.chat.width);
        let clear = self.screen.clear();
        self.write(&clear);
        let rest = std::mem::take(&mut self.chat.out);
        let mut text = String::new();
        for l in rest {
            text.push_str(&l);
            text.push_str("\x1b[K\r\n");
        }
        let _ = width;
        self.write(text.as_bytes());
    }
}
