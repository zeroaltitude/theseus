//! The TUI's state and what changes it (design `stage2` §2.9): the board, the
//! cursor, the filters, the session in focus and its pane, its question's
//! card, what is being typed, the connection's state. It does no I/O. The
//! loop (`run.rs`) hands it the daemon's messages and the terminal's events,
//! and carries out the effects it returns: requests to send, and quitting.

use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use serde_json::{json, Value};
use theseus_client::render::{self, Tag};
use theseus_protocol::{
    error_code, method, ActionConfirmResult, ConfirmListResult, Event, EventsLost,
    ExecutionStopResult, ExecutionView, ExecutionsWatchResult, RpcError, SessionHistoryResult,
    SessionKind, SessionListResult, TaskCancelResult,
};

use crate::board::{short, Board, Moved, Only, Question, Row};
use crate::detail::Detail;
use crate::notice::Notices;
use crate::seen::Seen;
use theseus_protocol::notices::{self, Viewer};

/// What the TUI's requests carry as their author: the ledger names it, and
/// `[approval]` counts its answers as the CLI's (the `cli` channel).
pub const AUTHOR: &str = "the TUI";

/// Below this width, the detail pane gives way to a full-width list.
pub const WIDE: u16 = 100;

/// How many of a session's newest nodes its pane reads.
pub const HISTORY_NODES: usize = 200;

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
    /// `session.history`: the focused session's pane.
    History(String),
    /// `session.history`'s newest nodes, for one an other surface wrote.
    Node { session: String, node: String },
    /// `session.watch` and `session.unwatch`.
    Watch,
    /// `turn.submit` from the input line.
    Submit(String),
    /// `execution.stop`.
    Stop(String),
    /// `task.cancel`.
    Cancel(String),
    /// `action.confirm`: an answer to a question.
    Answer { correlation_id: String },
}

/// What the loop does for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Call(Call),
    Quit,
    /// Tell the operator, as `--notify` says: the bell, OSC 9, or OSC 777;
    /// and whether the ping may sound (a finish rings no bell).
    Notice(String, bool),
    /// The terminal's title: `theseus (2)`.
    Title(String),
    /// Write the seen file: its path and its text.
    Save(PathBuf, String),
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

/// What a second key would do: `s` and `c` each ask for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    Stop,
    Cancel,
}

/// What the keys type into, when not into the list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Normal,
    /// `/`: a text that filters the sidebar.
    Filter,
    /// `i`: the input line.
    Input,
    /// `s` or `c` was pressed once, for this session: the same key again acts.
    Armed(Arm, String),
    /// `n` on a question (its correlation id): the decline's note.
    Note(String),
    /// `?`: the keys.
    Help,
}

/// Which pane a narrow screen shows: the list, or the session in focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    List,
    Session,
}

pub struct App {
    pub board: Board,
    /// The session under the cursor.
    pub selected: Option<String>,
    /// The session in focus: its history and its events, in the detail pane.
    pub detail: Option<Detail>,
    pub pane: Pane,
    pub mode: Mode,
    pub only: Only,
    pub filter: String,
    /// The input line's text.
    pub input: String,
    /// The decline's note, while `n` waits for it.
    pub note: String,
    /// Why an answer did not count, by its question: on its card until the
    /// question closes.
    pub refused: HashMap<String, String>,
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
    /// What the operator has seen (done until seen), kept in the seen file.
    pub seen: Seen,
    pub notices: Notices,
    /// The terminal has focus (crossterm's focus events; assumed until one
    /// says otherwise). While it has, the session in focus is seen, and gets
    /// no notice.
    pub term_focus: bool,
    /// When the terminal last lost focus: how long the operator has been away.
    blurred_at: u64,
    /// `--notify off`: notices show in the footer and never ring.
    pub quiet: bool,
    /// The title last set.
    title: String,
    /// When the seen file is due to be written: a change of focus waits a
    /// second, so a walk through the queue writes once.
    save_at: Option<u64>,
}

/// How long after a change of focus the seen file is written.
const SAVE_AFTER_MS: u64 = 1_000;

impl App {
    pub fn new(hm: fn(u64) -> String) -> Self {
        Self {
            board: Board::default(),
            selected: None,
            detail: None,
            pane: Pane::List,
            mode: Mode::Normal,
            only: Only::All,
            filter: String::new(),
            input: String::new(),
            note: String::new(),
            refused: HashMap::new(),
            link: Link::Connecting,
            flash: None,
            now_ms: 0,
            width: 80,
            height: 24,
            hm,
            asking_questions: false,
            seen: Seen::default(),
            notices: Notices::default(),
            term_focus: true,
            blurred_at: 0,
            quiet: false,
            title: String::new(),
            save_at: None,
        }
    }

    pub fn wide(&self) -> bool {
        self.width >= WIDE
    }

    /// Done until seen (design §2.2): the client's rule, from the seen state.
    pub fn is_done(&self, v: &ExecutionView) -> bool {
        self.seen.done(v)
    }

    /// The sidebar's rows, as the filters choose them.
    pub fn rows(&self) -> Vec<Row> {
        self.board
            .rows(self.only, &self.filter, &|v| self.is_done(v))
    }

    /// The session the operator sees now: in focus, on screen, on a terminal
    /// that has focus.
    fn watching(&self, sid: &str) -> bool {
        self.term_focus && self.shown() == Some(sid)
    }

    /// A view the board took: the seen state, and a notice if it moved live.
    fn took(&mut self, v: &ExecutionView, moved: Option<Moved>) {
        let watching = self.watching(&v.session_id);
        self.seen.applied(v, watching);
        let title = self.board.name(&v.session_id);
        match moved {
            Some(_) => self.notices.moved(v, &title, self.now_ms, &self.hm),
            None => self.notices.saw(v, &title),
        }
    }

    /// What is on screen now is seen: the session in focus, if the terminal
    /// has focus.
    fn mark_shown(&mut self) {
        if !self.term_focus {
            return;
        }
        if let Some(v) = self.shown().and_then(|sid| self.board.view(sid)).cloned() {
            self.seen.display(&v);
        }
    }

    /// The terminal gained or lost focus.
    pub fn term_focused(&mut self, focused: bool) {
        if self.term_focus && !focused {
            self.blurred_at = self.now_ms;
        }
        self.term_focus = focused;
        self.mark_shown();
    }

    /// The session whose pane is on screen: the focused one, beside the list
    /// on a wide screen, or instead of it on a narrow one.
    pub fn shown(&self) -> Option<&str> {
        let d = self.detail.as_ref()?;
        (self.wide() || self.pane == Pane::Session).then_some(d.session_id.as_str())
    }

    /// The card: the first question of the session on screen, and how many
    /// wait behind it.
    pub fn card(&self) -> Option<(Question<'_>, usize)> {
        let questions = self.board.questions(self.shown()?);
        let behind = questions.len().saturating_sub(1);
        questions.into_iter().next().map(|q| (q, behind))
    }

    /// When the app needs the loop next, if ever (design §2.9, quiet): a
    /// notice's second, the seen file's write, and the card's countdown,
    /// which ticks once a second while it is on screen.
    pub fn deadline(&self) -> Option<u64> {
        let countdown = self.card().and_then(|(q, _)| {
            let left = q
                .expires_at_ms()
                .checked_sub(self.now_ms)
                .filter(|l| *l > 0)?;
            // The moment the countdown's seconds change.
            Some(self.now_ms + if left % 1000 == 0 { 1000 } else { left % 1000 })
        });
        [countdown, self.notices.next_due(), self.save_at]
            .into_iter()
            .flatten()
            .min()
    }

    /// A deadline came: the notices that still hold fire, and the seen file
    /// is written if it is due.
    pub fn tick(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        let fired = {
            let (board, seen) = (&self.board, &self.seen);
            let viewer = Viewer {
                focused: self.shown().map(String::from),
                seen: 0,
                away_ms: if self.term_focus {
                    0
                } else {
                    self.now_ms.saturating_sub(self.blurred_at).max(1)
                },
                quiet: self.quiet,
                sound: true,
            };
            let view = |sid: &str| board.view(sid).cloned();
            let viewer = |sid: &str| Viewer {
                seen: board
                    .view(sid)
                    .map_or(0, |v| seen.displayed(&v.execution_id)),
                ..viewer.clone()
            };
            self.notices.take_due(self.now_ms, &view, &viewer, &self.hm)
        };
        for f in fired {
            self.flash = Some((Tag::Ok, f.line.clone()));
            if let notices::Delivery::Ping { sound } = f.delivery {
                out.push(Effect::Notice(f.line, sound));
            }
        }
        if self.save_at.is_some_and(|at| at <= self.now_ms) {
            out.extend(self.save());
        }
        out
    }

    /// The seen file's write, if there is a file and something changed; only
    /// the executions the board holds are kept, so it does not grow forever.
    pub fn save(&mut self) -> Option<Effect> {
        self.save_at = None;
        if !self.seen.dirty {
            return None;
        }
        let path = self.seen.path()?.clone();
        self.seen.dirty = false;
        let ids = self.board.execution_ids();
        Some(Effect::Save(path, self.seen.text(&|id| ids.contains(id))))
    }

    /// The terminal's title, when it changed: `theseus (2)`, the queue's
    /// length (needs you, and done until seen).
    pub fn title(&mut self) -> Option<Effect> {
        let (need, done) = self.board.counts(&|v| self.is_done(v));
        let t = match need + done {
            0 => "theseus".to_string(),
            n => format!("theseus ({n})"),
        };
        if t == self.title {
            return None;
        }
        self.title.clone_from(&t);
        Some(Effect::Title(t))
    }

    // ------------------------------------------------------------ the link

    /// Connected (again): read the board, and the focused session's pane.
    /// The board's first snapshot is the truth: a restarted daemon may serve
    /// another store.
    pub fn connected(&mut self) -> Vec<Effect> {
        self.link = Link::Up;
        self.asking_questions = false;
        self.board.ask_again();
        self.board.reconnected();
        let mut out = vec![snapshot()];
        if let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) {
            out.extend(read_session(&sid));
        }
        out
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
        let mut out = Vec::new();
        if let Some(d) = self.detail.as_mut() {
            d.event(&event);
            if let Event::NodeWritten(n) = &event {
                if n.kind == "user_message"
                    && n.session_id == d.session_id
                    && d.others_message(&n.node_id)
                {
                    out.push(Effect::Call(Call {
                        method: method::SESSION_HISTORY,
                        params: json!({ "session_id": n.session_id, "n": 5 }),
                        purpose: Purpose::Node {
                            session: n.session_id.clone(),
                            node: n.node_id.clone(),
                        },
                    }));
                }
            }
        }
        match event {
            Event::ExecutionChanged(view) => {
                if let Some(moved) = self.board.apply(view.clone()) {
                    self.took(&view, Some(moved));
                }
                out.extend(self.follow_up());
            }
            Event::ConfirmRequested(c) => self.board.confirm_requested(c),
            Event::ConfirmResolved(r) => {
                self.refused.remove(&r.correlation_id);
                self.board.confirm_resolved(&r);
            }
            Event::EventsLost(lost) => out.extend(self.lost(&lost)),
            _ => {}
        }
        out
    }

    /// The connection fell behind and the daemon dropped notifications
    /// (design §2.5): read each stream again.
    fn lost(&mut self, lost: &EventsLost) -> Vec<Effect> {
        self.flash = Some((Tag::Warn, render::lost_line(lost)));
        let mut out = Vec::new();
        let all = lost.streams.is_empty();
        if all || lost.streams.iter().any(|s| s == "executions") {
            out.push(snapshot());
        }
        if let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) {
            let stream = format!("session:{sid}");
            if all || lost.streams.contains(&stream) {
                out.extend(read_session(&sid));
            }
        }
        out
    }

    /// An answer to one of the TUI's requests.
    pub fn answered(&mut self, purpose: Purpose, result: Result<Value, RpcError>) -> Vec<Effect> {
        let v = match result {
            Ok(v) => v,
            Err(e) => {
                self.failed(&purpose, &e);
                return Vec::new();
            }
        };
        match purpose {
            Purpose::Snapshot => match serde_json::from_value::<ExecutionsWatchResult>(v) {
                Ok(snap) => {
                    // A snapshot's views go into the seen state, without
                    // notices: a notice is for a transition seen as it
                    // happens. The first board of a first start counts as
                    // seen (`Seen`).
                    for v in self.board.snapshot(snap) {
                        self.took(&v, None);
                    }
                    self.seen.first_board_read();
                    return self.follow_up();
                }
                Err(e) => self.flash = Some((Tag::Bad, format!("the board did not decode: {e}"))),
            },
            Purpose::Titles => {
                if let Ok(list) = serde_json::from_value::<SessionListResult>(v) {
                    self.board.titles(list.sessions);
                }
            }
            Purpose::Questions => {
                self.asking_questions = false;
                if let Ok(list) = serde_json::from_value::<ConfirmListResult>(v) {
                    for c in list.confirms {
                        self.board.confirm_requested(c);
                    }
                }
            }
            Purpose::History(sid) => {
                if let Ok(h) = serde_json::from_value::<SessionHistoryResult>(v) {
                    self.board.titles(vec![h.session.clone()]);
                    if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == sid) {
                        d.history(&h);
                    }
                }
            }
            // The node goes in the place its `node.written` marked; a read
            // that does not find it leaves nothing behind (theseus-v6yc).
            Purpose::Node { session, node } => {
                let h = serde_json::from_value::<SessionHistoryResult>(v).ok();
                let found = h
                    .as_ref()
                    .and_then(|h| h.nodes.iter().find(|n| n.node_id == node));
                if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == session) {
                    match found {
                        Some(n) => d.node(n),
                        None => d.unmark(&node),
                    }
                }
            }
            Purpose::Stop(sid) => {
                if let Ok(r) = serde_json::from_value::<ExecutionStopResult>(v) {
                    let line = render::stop_line(&r);
                    self.note(&sid, Tag::Warn, &line);
                    self.flash = Some((Tag::Warn, line));
                }
            }
            Purpose::Cancel(sid) => {
                if let Ok(r) = serde_json::from_value::<TaskCancelResult>(v) {
                    let line = format!(
                        "cancelled task {}{}",
                        r.task.short,
                        match r.cancelled_actions.len() {
                            0 => String::new(),
                            n => format!(" and {n} call{}", if n == 1 { "" } else { "s" }),
                        }
                    );
                    self.note(&sid, Tag::Warn, &line);
                    self.flash = Some((Tag::Warn, line));
                }
            }
            Purpose::Answer { correlation_id, .. } => {
                self.refused.remove(&correlation_id);
                if let Ok(r) = serde_json::from_value::<ActionConfirmResult>(v) {
                    self.flash = Some(if r.approved {
                        (Tag::Ok, format!("✓ approved {correlation_id}"))
                    } else {
                        (Tag::Warn, format!("✗ declined {correlation_id}"))
                    });
                }
            }
            // A turn's answer comes at its end; its events said the rest.
            Purpose::Submit(_) | Purpose::Watch => {}
        }
        Vec::new()
    }

    /// A request that failed: say so where the operator looks.
    fn failed(&mut self, purpose: &Purpose, e: &RpcError) {
        // An answer that did not count (theseus-sgh): its card says why, and
        // the question stays.
        if let Purpose::Answer { correlation_id, .. } = purpose {
            if e.code == error_code::REFUSED {
                self.refused
                    .insert(correlation_id.clone(), e.message.clone());
                self.flash = Some((Tag::Bad, format!("refused: {}", e.message)));
                return;
            }
        }
        if let Purpose::Node { session, node } = purpose {
            if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == *session) {
                d.unmark(node);
            }
        }
        let what = match purpose {
            Purpose::Snapshot => method::EXECUTIONS_WATCH,
            Purpose::Titles => method::SESSION_LIST,
            Purpose::Questions => {
                self.asking_questions = false;
                method::CONFIRM_LIST
            }
            Purpose::History(_) | Purpose::Node { .. } => method::SESSION_HISTORY,
            Purpose::Watch => method::SESSION_WATCH,
            Purpose::Submit(_) => method::TURN_SUBMIT,
            Purpose::Stop(_) => method::EXECUTION_STOP,
            Purpose::Cancel(_) => method::TASK_CANCEL,
            Purpose::Answer { .. } => method::ACTION_CONFIRM,
        };
        let text = format!("{what}: {}", e.message);
        if let Purpose::Submit(sid) | Purpose::Stop(sid) | Purpose::Cancel(sid) = purpose {
            self.note(sid, Tag::Bad, &format!("✗ {text}"));
        }
        self.flash = Some((Tag::Bad, text));
    }

    /// A line of the TUI's own in a session's pane, if it is in focus.
    fn note(&mut self, sid: &str, tag: Tag, text: &str) {
        if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == sid) {
            d.note(tag, text);
        }
    }

    /// What the board needs after a change: the titles of new sessions, and
    /// the whole of any question it holds only in brief.
    fn follow_up(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        let ids = self.board.untitled();
        if !ids.is_empty() {
            out.push(call(
                method::SESSION_LIST,
                json!({ "ids": ids }),
                Purpose::Titles,
            ));
        }
        if !self.asking_questions && self.board.missing_questions() {
            self.asking_questions = true;
            out.push(call(method::CONFIRM_LIST, Value::Null, Purpose::Questions));
        }
        out
    }

    // ------------------------------------------------------------ focus

    /// Focus a session: its history, then its events. The session before it
    /// is unwatched.
    pub fn focus(&mut self, sid: &str) -> Vec<Effect> {
        self.selected = Some(sid.to_string());
        self.pane = Pane::Session;
        if self.detail.as_ref().is_some_and(|d| d.session_id == sid) {
            // Back to its pane (a narrow screen): seen again.
            self.mark_shown();
            return Vec::new();
        }
        let mut out = Vec::new();
        if let Some(old) = self.detail.take() {
            out.push(call(
                method::SESSION_UNWATCH,
                json!({ "session_id": old.session_id }),
                Purpose::Watch,
            ));
        }
        self.detail = Some(Detail::new(sid));
        // What finished in it is seen now; the seen file follows a second
        // later, so a walk through the queue writes it once.
        self.mark_shown();
        self.save_at = Some(self.now_ms + SAVE_AFTER_MS);
        out.extend(read_session(sid));
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
        match self.mode.clone() {
            Mode::Help => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Mode::Filter => {
                self.filter_key(k);
                Vec::new()
            }
            Mode::Input => self.input_key(k),
            Mode::Armed(arm, sid) => self.armed_key(k, arm, &sid),
            Mode::Note(id) => self.note_key(k, &id),
            Mode::Normal => self.normal_key(k),
        }
    }

    fn normal_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        self.flash = None;
        // A narrow screen showing a session: the arrows scroll it, and enter
        // has no row to open.
        if !self.wide() && self.shown().is_some() {
            match k.code {
                KeyCode::Up | KeyCode::Char('k') => return self.scroll_by(1),
                KeyCode::Down | KeyCode::Char('j') => return self.scroll_by(-1),
                KeyCode::Enter => return Vec::new(),
                _ => {}
            }
        }
        match k.code {
            KeyCode::Char('q') => return vec![Effect::Quit],
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Enter => {
                if let Some(sid) = self.selected.clone() {
                    return self.focus(&sid);
                }
                self.step(1);
            }
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
                if !self.wide() && self.pane == Pane::Session {
                    self.pane = Pane::List;
                } else {
                    self.only = Only::All;
                    self.filter.clear();
                }
            }
            KeyCode::Char('i') => {
                if self.on_screen().is_some() {
                    self.mode = Mode::Input;
                }
            }
            KeyCode::Tab => return self.jump(true),
            KeyCode::BackTab => return self.jump(false),
            KeyCode::Char('y') => return self.answer_key(true, false),
            KeyCode::Char('t') => return self.answer_key(true, true),
            KeyCode::Char('n') => return self.answer_key(false, false),
            KeyCode::Char('s') => self.arm(Arm::Stop),
            KeyCode::Char('c') => self.arm(Arm::Cancel),
            KeyCode::PageUp => self.scroll(true),
            KeyCode::PageDown => self.scroll(false),
            KeyCode::End => {
                if let Some(d) = self.detail.as_mut() {
                    d.scroll = 0;
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// The session the keys act on: the one in focus, if its pane is on
    /// screen; else the footer says how to open one.
    fn on_screen(&mut self) -> Option<String> {
        match self.shown() {
            Some(sid) => Some(sid.to_string()),
            None => {
                self.flash = Some((Tag::Dim, "open a session first: enter".to_string()));
                None
            }
        }
    }

    /// `s` or `c`, once: the footer asks for the same key again.
    fn arm(&mut self, arm: Arm) {
        let Some(sid) = self.on_screen() else {
            return;
        };
        let kind = self.board.view(&sid).map(|v| v.kind);
        let refusal = match (arm, kind) {
            (Arm::Stop, Some(SessionKind::Task)) => {
                Some("a task is not stopped but cancelled: c, then c again")
            }
            (Arm::Cancel, Some(SessionKind::Conversation)) => {
                Some("only a task is cancelled; a conversation is stopped: s, then s again")
            }
            (_, None) => Some("the board does not hold this session yet"),
            _ => None,
        };
        match refusal {
            Some(why) => self.flash = Some((Tag::Warn, why.to_string())),
            None => self.mode = Mode::Armed(arm, sid),
        }
    }

    /// `y`, `t`, or `n` on the card's question: `n` asks for a note first.
    fn answer_key(&mut self, approve: bool, trust: bool) -> Vec<Effect> {
        if self.on_screen().is_none() {
            return Vec::new();
        }
        let Some((q, _)) = self.card() else {
            self.flash = Some((Tag::Dim, "no question waits here".to_string()));
            return Vec::new();
        };
        let (id, trusts) = (q.correlation_id().to_string(), q.trusts());
        if trust && !trusts {
            self.flash = Some((
                Tag::Dim,
                "nothing to trust: the session holds no external text (y approves)".to_string(),
            ));
            return Vec::new();
        }
        if !approve {
            self.note.clear();
            self.mode = Mode::Note(id);
            return Vec::new();
        }
        vec![answer(&id, true, trust, None)]
    }

    /// The decline's note: `enter` sends the decline, with the note if one
    /// was typed; `esc` answers nothing.
    fn note_key(&mut self, k: KeyEvent, id: &str) -> Vec<Effect> {
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.flash = Some((Tag::Dim, "not declined".to_string()));
            }
            KeyCode::Backspace => {
                self.note.pop();
            }
            KeyCode::Char(c) => self.note.push(c),
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                let note = std::mem::take(&mut self.note).trim().to_string();
                return vec![answer(
                    id,
                    false,
                    false,
                    Some(note).filter(|n| !n.is_empty()),
                )];
            }
            _ => {}
        }
        Vec::new()
    }

    /// `tab` / `shift-tab`: the next or previous session that needs attention
    /// (design §2.2's queue), focused with its pane open. Both wrap.
    fn jump(&mut self, forward: bool) -> Vec<Effect> {
        let queue = self.board.queue(&|v| self.is_done(v));
        if queue.is_empty() {
            self.flash = Some((Tag::Dim, "nothing needs you".to_string()));
            return Vec::new();
        }
        let here = self
            .detail
            .as_ref()
            .map(|d| d.session_id.as_str())
            .and_then(|s| queue.iter().position(|q| q == s));
        let n = queue.len();
        let next = match (here, forward) {
            (Some(i), true) => (i + 1) % n,
            (Some(i), false) => (i + n - 1) % n,
            (None, true) => 0,
            (None, false) => n - 1,
        };
        let sid = queue[next].clone();
        self.focus(&sid)
    }

    fn armed_key(&mut self, k: KeyEvent, arm: Arm, sid: &str) -> Vec<Effect> {
        self.mode = Mode::Normal;
        let again = matches!(
            (arm, k.code),
            (Arm::Stop, KeyCode::Char('s')) | (Arm::Cancel, KeyCode::Char('c'))
        );
        if !again {
            self.flash = Some((
                Tag::Dim,
                match arm {
                    Arm::Stop => "not stopped",
                    Arm::Cancel => "not cancelled",
                }
                .to_string(),
            ));
            return Vec::new();
        }
        match arm {
            Arm::Stop => {
                let Some(exe) = self.board.view(sid).map(|v| v.execution_id.clone()) else {
                    return Vec::new();
                };
                vec![call(
                    method::EXECUTION_STOP,
                    json!({ "execution_id": exe, "author": AUTHOR }),
                    Purpose::Stop(sid.to_string()),
                )]
            }
            Arm::Cancel => vec![call(
                method::TASK_CANCEL,
                json!({ "task": sid, "author": AUTHOR }),
                Purpose::Cancel(sid.to_string()),
            )],
        }
    }

    /// The input line: `enter` sends it to the focused session as
    /// `turn.submit`, and `esc` leaves it (keeping what was typed).
    fn input_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => self.input.push(c),
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) else {
                    self.mode = Mode::Normal;
                    return Vec::new();
                };
                if text.is_empty() {
                    return Vec::new();
                }
                self.input.clear();
                self.mode = Mode::Normal;
                if let Some(d) = self.detail.as_mut() {
                    d.sent(&text);
                    d.scroll = 0;
                }
                return vec![call(
                    method::TURN_SUBMIT,
                    json!({ "session_id": sid, "input": text, "author": AUTHOR }),
                    Purpose::Submit(sid),
                )];
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

    /// Scroll the focused session's pane by most of a screen.
    fn scroll(&mut self, up: bool) {
        let page = i64::from(self.height.saturating_sub(6)).max(1);
        self.scroll_by(if up { page } else { -page });
    }

    /// Scroll the focused session's pane by `rows`, up for a positive count.
    fn scroll_by(&mut self, rows: i64) -> Vec<Effect> {
        if let Some(d) = self.detail.as_mut() {
            d.scroll = if rows > 0 {
                d.scroll.saturating_add(rows as usize)
            } else {
                d.scroll.saturating_sub(rows.unsigned_abs() as usize)
            };
        }
        Vec::new()
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

    /// What the footer says while `s` or `c` waits for its second key.
    pub fn armed_words(&self) -> Option<String> {
        let Mode::Armed(arm, sid) = &self.mode else {
            return None;
        };
        let name = self.board.name(sid);
        Some(match arm {
            Arm::Stop => format!(
                "stop {name} (…{})? s again to stop; any other key: no",
                short(sid)
            ),
            Arm::Cancel => {
                format!(
                    "cancel task {name} (…{})? c again to cancel; any other key: no",
                    short(sid)
                )
            }
        })
    }
}

fn call(method: &'static str, params: Value, purpose: Purpose) -> Effect {
    Effect::Call(Call {
        method,
        params,
        purpose,
    })
}

/// `executions.watch`: the board's snapshot, then its events.
fn snapshot() -> Effect {
    call(method::EXECUTIONS_WATCH, json!({}), Purpose::Snapshot)
}

/// A session's pane: its newest nodes, then its events (design §2.9: the
/// focused session only gets `session.history`, then `session.watch`).
fn read_session(sid: &str) -> Vec<Effect> {
    vec![
        call(
            method::SESSION_HISTORY,
            json!({ "session_id": sid, "n": HISTORY_NODES }),
            Purpose::History(sid.to_string()),
        ),
        call(
            method::SESSION_WATCH,
            json!({ "session_id": sid }),
            Purpose::Watch,
        ),
    ]
}

/// An answer to a question (design §2.9, "Answering"): `action.confirm` as
/// the TUI, so `[approval]` judges it as the `cli` channel's.
fn answer(id: &str, approve: bool, trust: bool, note: Option<String>) -> Effect {
    let mut params = json!({
        "correlation_id": id,
        "approve": approve,
        "author": AUTHOR,
    });
    if trust {
        params["trust"] = json!(true);
    }
    if let Some(n) = note {
        params["note"] = json!(n);
    }
    call(
        method::ACTION_CONFIRM,
        params,
        Purpose::Answer {
            correlation_id: id.to_string(),
        },
    )
}
