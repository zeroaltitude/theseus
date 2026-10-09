//! The loop (design `stage2` §2.9's data flow): one connection, read in a
//! `select!` with the terminal's events and one deadline, so the TUI is quiet
//! by construction. It redraws on events, at most 30 frames a second, and
//! runs no timer but the deadlines the app asks for. A closed connection is
//! tried again with backoff (500 ms, doubling to 10 s, as the web UI's
//! `ProtocolClient` does), and the board is read again. It writes the
//! terminal's own sequences (the bell, a notice, the title) and the seen file.

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::Event as TermEvent;
use ratatui::backend::Backend;
use ratatui::Terminal;
use theseus_client::Conn;
use theseus_protocol::{Id, Message};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::Instant;

use crate::app::{App, Effect, Purpose};
use crate::notice::Delivery;

/// How the loop gets a connection: a socket in the binary, a scripted daemon
/// in tests.
pub type Connector = Box<dyn FnMut() -> Pin<Box<dyn Future<Output = Result<Conn>> + Send>> + Send>;

/// The first wait before trying again, and the longest.
const RETRY_FIRST_MS: u64 = 500;
const RETRY_MAX_MS: u64 = 10_000;

/// How long a connect may take before it counts as failed: a connect that
/// hangs must not hold the keys.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// At most 30 frames a second.
pub const FRAME: Duration = Duration::from_millis(33);

pub struct Runner<B: Backend> {
    pub app: App,
    pub term: Terminal<B>,
    conn: Option<Conn>,
    connect: Connector,
    events: UnboundedReceiver<TermEvent>,
    /// What each request in flight is for, by its id.
    pending: HashMap<Id, Purpose>,
    attempt: u32,
    retry_at: Option<Instant>,
    /// The frame's interval: `FRAME`, or zero in tests.
    pub frame: Duration,
    last_draw: Option<Instant>,
    draw_at: Option<Instant>,
    dirty: bool,
    quit: bool,
    /// The wall clock, in ms since the epoch.
    clock: fn() -> u64,
    /// Where the terminal's own sequences go (the bell, a notice, the title):
    /// stdout in the binary, a buffer in tests.
    pub out: Box<dyn Write + Send>,
    /// How a notice reaches the operator (`--notify`).
    pub delivery: Delivery,
    /// How many terminal events the loop has handled: a test waits on it.
    #[cfg(test)]
    pub terminal_events: u64,
}

/// What woke the loop.
enum Woke {
    Daemon(Result<Option<Message>>),
    Terminal(Option<TermEvent>),
    Deadline,
}

impl<B: Backend> Runner<B> {
    pub fn new(
        app: App,
        term: Terminal<B>,
        connect: Connector,
        events: UnboundedReceiver<TermEvent>,
        clock: fn() -> u64,
    ) -> Self {
        Self {
            app,
            term,
            conn: None,
            connect,
            events,
            pending: HashMap::new(),
            attempt: 0,
            // The first connect is due at once.
            retry_at: Some(Instant::now()),
            frame: FRAME,
            last_draw: None,
            draw_at: None,
            dirty: true,
            quit: false,
            clock,
            out: Box::new(std::io::sink()),
            delivery: Delivery::default(),
            #[cfg(test)]
            terminal_events: 0,
        }
    }

    /// Run until the operator quits.
    pub async fn run(&mut self) -> Result<()> {
        while !self.quit {
            self.step().await?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn quitting(&self) -> bool {
        self.quit
    }

    /// One wake: a message, a terminal event, or a deadline; then a draw, if
    /// one is due.
    pub async fn step(&mut self) -> Result<()> {
        let deadline = self.deadline();
        let woke = {
            let conn = &mut self.conn;
            let events = &mut self.events;
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
            }
        };
        self.app.now_ms = (self.clock)();
        match woke {
            Woke::Daemon(Ok(Some(msg))) => self.message(msg).await,
            Woke::Daemon(Ok(None)) => self.lost_connection(),
            Woke::Daemon(Err(_)) => self.lost_connection(),
            Woke::Terminal(Some(e)) => self.terminal(e).await,
            // The terminal's reader ended: nothing more can be typed.
            Woke::Terminal(None) => self.apply(vec![Effect::Quit]).await,
            Woke::Deadline => self.deadlines().await,
        }
        // The title carries the queue's count (design §2.9): set when it
        // changes.
        if let Some(t) = self.app.title() {
            self.apply(vec![t]).await;
        }
        self.maybe_draw()?;
        Ok(())
    }

    /// The soonest of: the retry's backoff, the next frame, and what the app
    /// asks for (a notice's second, the seen file's write, the card's
    /// countdown), on the wall clock.
    fn deadline(&self) -> Option<Instant> {
        let now_ms = (self.clock)();
        let app = self
            .app
            .deadline()
            .map(|at| Instant::now() + Duration::from_millis(at.saturating_sub(now_ms)));
        [self.retry_at, self.draw_at, app]
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
        }
        let effects = self.app.tick();
        self.apply(effects).await;
        // The countdown moved, or a notice fired: draw it.
        self.dirty = true;
    }

    async fn reconnect(&mut self) {
        let connecting = (self.connect)();
        match tokio::time::timeout(CONNECT_TIMEOUT, connecting).await {
            Ok(Ok(conn)) => {
                self.conn = Some(conn);
                self.attempt = 0;
                let effects = self.app.connected();
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
        self.app.disconnected(self.attempt);
    }

    async fn message(&mut self, msg: Message) {
        let effects = match msg {
            Message::Notification(n) => self.app.notified(&n.method, &n.params),
            Message::Response(r) => match self.pending.remove(&r.id) {
                Some(purpose) => {
                    let result = match r.error {
                        Some(e) => Err(e),
                        None => Ok(r.result.unwrap_or(serde_json::Value::Null)),
                    };
                    self.app.answered(purpose, result)
                }
                None => Vec::new(),
            },
            Message::Request(_) => Vec::new(),
        };
        self.apply(effects).await;
        self.dirty = true;
    }

    async fn terminal(&mut self, e: TermEvent) {
        #[cfg(test)]
        {
            self.terminal_events += 1;
        }
        let effects = match e {
            TermEvent::Key(k) => self.app.key(k),
            TermEvent::Resize(w, h) => {
                self.app.resized(w, h);
                Vec::new()
            }
            TermEvent::FocusGained => {
                self.app.term_focused(true);
                Vec::new()
            }
            TermEvent::FocusLost => {
                self.app.term_focused(false);
                Vec::new()
            }
            _ => Vec::new(),
        };
        self.apply(effects).await;
        self.dirty = true;
    }

    /// Carry out the app's effects. A request on a closed connection is
    /// dropped: the reconnect reads everything again.
    async fn apply(&mut self, effects: Vec<Effect>) {
        for e in effects {
            match e {
                // On the way out, what was seen is written (design §2.9).
                Effect::Quit => {
                    self.quit = true;
                    if let Some(Effect::Save(path, text)) = self.app.save() {
                        self.save(&path, &text);
                    }
                }
                Effect::Notice(text, sound) => {
                    let bytes = self.delivery.bytes(&text, sound);
                    self.write(&bytes);
                }
                Effect::Title(t) => self.write(format!("\x1b]0;{t}\x07").as_bytes()),
                Effect::Save(path, text) => self.save(&path, &text),
                Effect::Call(call) => {
                    let Some(conn) = self.conn.as_mut() else {
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

    /// The terminal's own sequence: a failed write costs a notice, never the
    /// loop.
    fn write(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.out.write_all(bytes).and_then(|()| self.out.flush());
        }
    }

    /// Write the seen file whole: a temporary file beside it, then a rename,
    /// so a crash never leaves half a file. A failure says so in the footer.
    fn save(&mut self, path: &Path, text: &str) {
        // Merged with what another client recorded (theseus-yus0), which is
        // then taken up.
        match theseus_client::seen::write_merged(path, text) {
            Ok(marks) => self.app.seen.adopt(marks),
            Err(e) => {
                self.app.flash = Some((
                    theseus_client::render::Tag::Bad,
                    format!("the seen file {}: {e}", path.display()),
                ));
            }
        }
    }

    /// Draw if the screen changed and a frame is due; else ask for a
    /// deadline at the next frame.
    fn maybe_draw(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let now = Instant::now();
        match self.last_draw {
            Some(last) if now < last + self.frame => {
                self.draw_at = Some(last + self.frame);
                Ok(())
            }
            _ => self.draw(),
        }
    }

    /// Draw now.
    pub fn draw(&mut self) -> Result<()> {
        self.app.now_ms = (self.clock)();
        let size = self.term.size().map_err(|e| anyhow::anyhow!("{e}"))?;
        self.app.resized(size.width, size.height);
        let app = &self.app;
        self.term
            .draw(|f| {
                if let Some((x, y)) = crate::ui::draw(app, f.buffer_mut()) {
                    f.set_cursor_position((x, y));
                }
            })
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.last_draw = Some(Instant::now());
        self.draw_at = None;
        self.dirty = false;
        Ok(())
    }
}
