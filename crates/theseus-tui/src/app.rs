//! The TUI's state and what changes it (design `stage2` §2.9): the board, the
//! cursor, the filters, the connection's state. It does no I/O. The loop
//! (`run.rs`) hands it the daemon's messages and the terminal's events, and
//! carries out the effects it returns: requests to send, and quitting.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{json, Value};
use theseus_client::render::Tag;
use theseus_protocol::{
    method, ConfirmListResult, Event, EventsLost, ExecutionView, ExecutionsWatchResult, RpcError,
    SessionListResult,
};

use crate::board::{Board, Only, Row};

/// Below this width, the detail pane gives way to a full-width list.
pub const WIDE: u16 = 100;

/// A request the loop sends, and what its answer is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: &'static str,
    pub params: Value,
    pub purpose: Purpose,
}

/// What an answer is for: the loop keeps it by the request's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// `executions.watch`: the board.
    Snapshot,
    /// `session.list { ids }`: titles.
    Titles,
    /// `confirm.list`: questions a view lists that the board lacks whole.
    Questions,
}

/// What the loop does for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Call(Call),
    Quit,
}

/// The connection, as the header says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// The first connect has not answered yet.
    Connecting,
    Up,
    /// Lost: the loop tries again with backoff; the attempt's number.
    Down {
        attempt: u32,
    },
}

/// What the keys type into, when not into the list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// `/`: a text that filters the sidebar.
    Filter,
    /// `?`: the keys.
    Help,
}

pub struct App {
    pub board: Board,
    /// The session under the cursor.
    pub selected: Option<String>,
    pub mode: Mode,
    pub only: Only,
    pub filter: String,
    pub link: Link,
    /// A line for the operator in the footer: an error, a refusal, a result.
    pub flash: Option<(Tag, String)>,
    /// The time now, set by the loop before each event and each draw.
    pub now_ms: u64,
    pub width: u16,
    pub height: u16,
    /// A time of day as the header writes it: the local time in the binary,
    /// UTC in tests.
    pub hm: fn(u64) -> String,
    /// A `confirm.list` is in flight: one at a time.
    asking_questions: bool,
}

impl App {
    pub fn new(hm: fn(u64) -> String) -> Self {
        Self {
            board: Board::default(),
            selected: None,
            mode: Mode::Normal,
            only: Only::All,
            filter: String::new(),
            link: Link::Connecting,
            flash: None,
            now_ms: 0,
            width: 80,
            height: 24,
            hm,
            asking_questions: false,
        }
    }

    pub fn wide(&self) -> bool {
        self.width >= WIDE
    }

    /// The sidebar's rows, as the filters choose them.
    pub fn rows(&self) -> Vec<Row> {
        self.board
            .rows(self.only, &self.filter, &|_: &ExecutionView| false)
    }

    // ------------------------------------------------------------ the link

    /// Connected (again): read the board. Its first snapshot is the truth: a
    /// restarted daemon may serve another store.
    pub fn connected(&mut self) -> Vec<Effect> {
        self.link = Link::Up;
        self.asking_questions = false;
        self.board.ask_again();
        self.board.reconnected();
        vec![snapshot()]
    }

    /// The connection closed, or a connect failed: the loop tries again.
    pub fn disconnected(&mut self, attempt: u32) {
        self.link = Link::Down { attempt };
        self.asking_questions = false;
    }

    // ------------------------------------------------------------ the daemon

    /// A notification from the daemon.
    pub fn notified(&mut self, method_name: &str, params: &Value) -> Vec<Effect> {
        let event = match Event::from_notification(method_name, params) {
            Ok(Some(e)) => e,
            // A newer daemon's notification, or one that does not decode.
            Ok(None) | Err(_) => return Vec::new(),
        };
        match event {
            Event::ExecutionChanged(view) => {
                self.board.apply(view);
                self.follow_up()
            }
            Event::ConfirmRequested(c) => {
                self.board.confirm_requested(c);
                Vec::new()
            }
            Event::ConfirmResolved(r) => {
                self.board.confirm_resolved(&r);
                Vec::new()
            }
            Event::EventsLost(lost) => self.lost(&lost),
            _ => Vec::new(),
        }
    }

    /// The connection fell behind and the daemon dropped notifications
    /// (design §2.5): read each stream again.
    fn lost(&mut self, lost: &EventsLost) -> Vec<Effect> {
        self.flash = Some((Tag::Warn, theseus_client::render::lost_line(lost)));
        let mut out = Vec::new();
        if lost.streams.iter().any(|s| s == "executions") || lost.streams.is_empty() {
            out.push(snapshot());
        }
        out
    }

    /// An answer to one of the TUI's requests.
    pub fn answered(&mut self, purpose: Purpose, result: Result<Value, RpcError>) -> Vec<Effect> {
        match (purpose, result) {
            (Purpose::Snapshot, Ok(v)) => {
                match serde_json::from_value::<ExecutionsWatchResult>(v) {
                    Ok(snap) => {
                        self.board.snapshot(snap);
                        self.follow_up()
                    }
                    Err(e) => {
                        self.flash = Some((Tag::Bad, format!("the board did not decode: {e}")));
                        Vec::new()
                    }
                }
            }
            (Purpose::Titles, Ok(v)) => {
                if let Ok(list) = serde_json::from_value::<SessionListResult>(v) {
                    self.board.titles(list.sessions);
                }
                Vec::new()
            }
            (Purpose::Questions, Ok(v)) => {
                self.asking_questions = false;
                if let Ok(list) = serde_json::from_value::<ConfirmListResult>(v) {
                    for c in list.confirms {
                        self.board.confirm_requested(c);
                    }
                }
                Vec::new()
            }
            (Purpose::Questions, Err(e)) => {
                self.asking_questions = false;
                self.flash = Some((Tag::Bad, format!("confirm.list: {}", e.message)));
                Vec::new()
            }
            (p, Err(e)) => {
                self.flash = Some((Tag::Bad, format!("{}: {}", purpose_word(&p), e.message)));
                Vec::new()
            }
        }
    }

    /// What the board needs after a change: the titles of new sessions, and
    /// the whole of any question it holds only in brief.
    fn follow_up(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        let ids = self.board.untitled();
        if !ids.is_empty() {
            out.push(Effect::Call(Call {
                method: method::SESSION_LIST,
                params: json!({ "ids": ids }),
                purpose: Purpose::Titles,
            }));
        }
        if !self.asking_questions && self.board.missing_questions() {
            self.asking_questions = true;
            out.push(Effect::Call(Call {
                method: method::CONFIRM_LIST,
                params: Value::Null,
                purpose: Purpose::Questions,
            }));
        }
        out
    }

    // ------------------------------------------------------------ the terminal

    pub fn resized(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// A key. Ctrl-C quits from anywhere.
    pub fn key(&mut self, k: KeyEvent) -> Vec<Effect> {
        if k.kind == KeyEventKind::Release {
            return Vec::new();
        }
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            return vec![Effect::Quit];
        }
        match self.mode {
            Mode::Help => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Mode::Filter => {
                self.filter_key(k);
                Vec::new()
            }
            Mode::Normal => self.normal_key(k),
        }
    }

    fn normal_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        self.flash = None;
        match k.code {
            KeyCode::Char('q') => return vec![Effect::Quit],
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('b') => self.only = Only::NeedsYou,
            KeyCode::Char('w') => self.only = Only::Working,
            KeyCode::Char('r') => self.only = Only::Ready,
            KeyCode::Char('d') => self.only = Only::Done,
            KeyCode::Char('a') => {
                self.only = Only::All;
                self.filter.clear();
            }
            KeyCode::Esc => {
                self.only = Only::All;
                self.filter.clear();
            }
            _ => {}
        }
        Vec::new()
    }

    fn filter_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Enter => self.mode = Mode::Normal,
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                self.filter.pop();
            }
            KeyCode::Char(c) => self.filter.push(c),
            _ => {}
        }
    }

    /// Move the cursor by `by` rows, from the selected session (which keeps
    /// the cursor when the rows reorder), or from the top.
    fn step(&mut self, by: i64) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let at = self
            .selected
            .as_ref()
            .and_then(|s| rows.iter().position(|r| &r.session_id == s));
        let next = match at {
            Some(i) => (i as i64 + by).clamp(0, rows.len() as i64 - 1) as usize,
            None => 0,
        };
        self.selected = Some(rows[next].session_id.clone());
    }

    /// The row index the cursor is on, if its session is shown.
    pub fn cursor(&self, rows: &[Row]) -> Option<usize> {
        let s = self.selected.as_ref()?;
        rows.iter().position(|r| &r.session_id == s)
    }
}

/// `executions.watch`: the board's snapshot, then its events.
fn snapshot() -> Effect {
    Effect::Call(Call {
        method: method::EXECUTIONS_WATCH,
        params: json!({}),
        purpose: Purpose::Snapshot,
    })
}

fn purpose_word(p: &Purpose) -> &'static str {
    match p {
        Purpose::Snapshot => method::EXECUTIONS_WATCH,
        Purpose::Titles => method::SESSION_LIST,
        Purpose::Questions => method::CONFIRM_LIST,
    }
}
