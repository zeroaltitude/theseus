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
use theseus_protocol::answer::{approves, WorkAnswerResult};
use theseus_protocol::{
    error_code, method, ConfirmListResult, Event, EventsLost, ExecutionStopResult, ExecutionView,
    ExecutionsWatchResult, RpcError, SessionHistoryResult, SessionKind, SessionListResult,
    TaskCancelResult, WorkWatchResult,
};

use crate::board::{short, Board, Moved, Only, Question, Row};
use crate::board_threads::ThreadBoard;
use crate::detail::Detail;
use crate::follow::Follow;
use crate::notice::{Delivery, Notices};
use crate::selfview::SelfView;
use crate::standing::{self, Standing};
use crate::threads::Picker;
use theseus_client::seen::Seen;
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
    /// `work.watch`: what the rows say (theseus-k2vca).
    Work,
    /// `session.list { ids }`: titles.
    Titles,
    /// `confirm.list`: questions a view lists that the board lacks whole.
    Questions,
    /// `session.history`: the focused session's pane.
    History(String),
    /// `session.history`'s newest nodes, for one an other surface wrote.
    Node {
        session: String,
        node: String,
    },
    /// `session.watch` and `session.unwatch`.
    Watch,
    /// `turn.submit` from the input line.
    Submit(String),
    /// `session.typing`: the input line's first key (theseus-tnky). Nothing
    /// waits for its answer.
    Typing,
    /// `health`, for the OpenClaw sync's warning in the header (theseus-pjmj7).
    Health,
    /// `self.log`, for the self view (theseus-pw1q.19).
    SelfLog,
    /// `standing.list` for the session in focus on this surface, for the
    /// header's pill and the list `p` opens (theseus-slvl.4).
    Standing(String),
    /// `work.digest`: what happened since the operator last looked (theseus-qkoi3).
    Digest,
    /// `execution.stop`.
    Stop(String),
    /// `task.cancel`.
    Cancel(String),
    /// `task.start`, from `N` (theseus-7oq0w).
    NewTask,
    /// `task.pause` or `task.resume`, from `P` (theseus-ypnxt.4).
    Hold,
    /// `action.confirm`: an answer to a question.
    Answer {
        correlation_id: String,
    },
    /// `thread.list`: the picker's and the status line's threads (theseus-j0yh).
    Threads,
    /// `thread.pause`, `.resume` or `.done` from the picker.
    ThreadAct(String),
    /// `turn.submit` that opens a thread's session (its slug).
    SubmitThread(String),
    /// `execution.dismiss`: a failure the owner is done with (theseus-7zgsm).
    Dismiss {
        session: String,
        execution: String,
    },
    /// `task.retry`: a failed task tried again.
    Retry {
        session: String,
        execution: String,
    },
    /// The follow pane (theseus-e1hu4): `action.list` finds the session's
    /// running job, `work.peek` reads it, `action.output` follows it, and
    /// `action.unfollow` lets it go.
    FollowFind,
    FollowPeek,
    FollowStart,
    Unfollow,
    /// A key in the Horizon section (a snooze, a pause, a run): the words
    /// the footer says when it is done.
    HorizonAct(String),
    /// `session.mode`: the focused conversation's mode, read when it is
    /// focused and set by Shift+Tab (theseus-lyxy).
    Mode(String),
    /// `turn.submit` to a task, with the text: refused because the task ended as it left, it goes to
    /// the task's parent instead (theseus-mu0e6).
    SubmitTask {
        task: String,
        text: String,
    },
    /// `turn.inspect` for the session's newest turn, from `I` (theseus-mu0e6).
    Inspect(String),
}

/// What the loop does for the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Call(Call),
    Quit,
    /// Tell the operator, as `--notify` says: a toast, the bell, OSC 9, or
    /// OSC 777. `title` names the task (a toast's title; empty while
    /// unknown), `line` is the notice's words, and `sound` whether the ping
    /// may sound (a finish rings no bell).
    Notice {
        title: String,
        line: String,
        sound: bool,
    },
    /// The terminal's title: `theseus (2)`.
    Title(String),
    /// Write the seen file: its path and its text.
    Save(PathBuf, String),
    /// Remember this terminal's current thread: the file, the terminal, the
    /// slug (none: home).
    Remember(PathBuf, String, Option<String>),
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

/// What a second key would do: `s`, `S` and `c` each ask for one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arm {
    Stop,
    /// `S`: the stop that also cancels every task under the conversation (theseus-cdcwr).
    StopAll,
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
    /// `N`: the brief of a new task, and the conversation it goes under
    /// (none: Home), typed into `App::brief` (theseus-7oq0w).
    NewTask(Option<String>),
    /// `T`: the keys move over the board's Threads block (theseus-5i8i).
    BoardThreads,
    /// `?`: the keys.
    Help,
    /// Ctrl-T: the thread picker (theseus-j0yh).
    Threads,
    /// `l`: what Theseus changed about itself (theseus-pw1q.19).
    SelfView,
    /// `p`: the standing records that hold in the session in focus.
    Standing,
    /// `H`: the Horizon section has the keys (theseus-nuhsz).
    Horizon,
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
    /// The threads on the work board (theseus-5i8i), fed by the one
    /// `work.watch` (`Purpose::Work`).
    pub board_threads: ThreadBoard,
    /// The session under the cursor.
    pub selected: Option<String>,
    /// The one-turn conversations' fold is open (`one_turn.rs`).
    pub one_turn_open: bool,
    /// The session in focus: its history and its events, in the detail pane.
    pub detail: Option<Detail>,
    pub pane: Pane,
    pub mode: Mode,
    pub only: Only,
    pub filter: String,
    /// The input line's text.
    pub input: String,
    /// The sessions told that someone types (theseus-tnky).
    typist: theseus_protocol::warm::Typist,
    /// The decline's note, while `n` waits for it.
    pub note: String,
    /// The new task's brief, while `N` waits for it.
    pub brief: String,
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
    /// `--notify` was given: the config's `[tui] notify` does not override it.
    pub notify_pinned: bool,
    /// `[tui] notify`, as the daemon's `health` says it (theseus-f0bi9).
    pub tui_notify: Option<Delivery>,
    /// The title last set.
    title: String,
    /// The OpenClaw sync's warning, from `health` at each connect: a failed or
    /// a missed night (theseus-pjmj7).
    pub sync_warn: Option<String>,
    /// The self view's last read (theseus-pw1q.19).
    pub selfview: SelfView,
    /// The standing records that hold in the session in focus here.
    pub standing: Option<Standing>,
    /// The daemon's own work (theseus-jn17m): the `Theseus` root and its jobs,
    /// from `work.watch` naming `system`, by id, kept by the position rule.
    /// The header says the root collapsed (`⚙ importing 41/120`).
    pub system: std::collections::BTreeMap<String, theseus_protocol::work::WorkView>,
    /// When the seen file is due to be written: a change of focus waits a
    /// second, so a walk through the queue writes once.
    save_at: Option<u64>,
    /// The threads as last read (theseus-j0yh), and the slug of the current one.
    pub threads: Vec<theseus_protocol::threads::ThreadRow>,
    pub thread: Option<String>,
    pub picker: Option<Picker>,
    /// Where this terminal's current thread is kept, and its name: none in tests.
    pub thread_file: Option<(PathBuf, String)>,
    /// The remembered thread's pane has been opened (once, at the first list).
    pub(crate) threads_focused: bool,
    /// The follow pane: a running job's output, over the detail area.
    pub follow: Option<Follow>,
    /// The future under the task tree (theseus-nuhsz).
    pub horizon: crate::horizon::Section,
    /// The newest board position applied: what "looked at the board" is
    /// recorded at (theseus-qkoi3).
    position: u64,
}

/// How long after a change of focus the seen file is written.
const SAVE_AFTER_MS: u64 = 1_000;

/// A terminal that had lost focus this long, regaining it, hears the digest.
pub const AWAY_MS: u64 = 60_000;

impl App {
    pub fn new(hm: fn(u64) -> String) -> Self {
        Self {
            board: Board::default(),
            board_threads: ThreadBoard::default(),
            selected: None,
            one_turn_open: false,
            detail: None,
            pane: Pane::List,
            mode: Mode::Normal,
            only: Only::All,
            filter: String::new(),
            input: String::new(),
            typist: Default::default(),
            note: String::new(),
            brief: String::new(),
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
            position: 0,
            sync_warn: None,
            notify_pinned: false,
            tui_notify: None,
            threads: Vec::new(),
            thread: None,
            picker: None,
            thread_file: None,
            threads_focused: false,
            follow: None,
            selfview: SelfView::default(),
            standing: None,
            system: Default::default(),
            horizon: Default::default(),
        }
    }

    pub fn wide(&self) -> bool {
        self.width >= WIDE
    }

    /// Done until seen (design §2.2): the client's rule, from the seen state.
    pub fn is_done(&self, v: &ExecutionView) -> bool {
        !v.dismissed && self.seen.done(v)
    }

    /// The sidebar's rows, as the filters choose them.
    pub fn rows(&self) -> Vec<Row> {
        self.fold_one_turn(
            self.board
                .rows(self.only, &self.filter, &|v| self.is_done(v)),
        )
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
        // A board on a terminal that has focus is being looked at: the
        // digest starts after the last such moment (theseus-qkoi3).
        self.position = self.position.max(v.position);
        if self.term_focus {
            self.seen.look(self.position, self.now_ms);
        }
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
    /// Regaining it after `AWAY_MS` asks for the digest: what happened while
    /// the operator was elsewhere (theseus-qkoi3).
    pub fn term_focused(&mut self, focused: bool) -> Vec<Effect> {
        let regained = !self.term_focus && focused;
        let away = self.now_ms.saturating_sub(self.blurred_at);
        if self.term_focus && !focused {
            self.blurred_at = self.now_ms;
            // The last look is now, and the file follows a second later.
            self.seen.look(self.position, self.now_ms);
            self.save_at = Some(self.now_ms + SAVE_AFTER_MS);
        }
        self.term_focus = focused;
        self.mark_shown();
        if regained && away >= AWAY_MS {
            return self.digest_call().into_iter().collect();
        }
        Vec::new()
    }

    /// `work.digest` since the last look; none before there is one (a first
    /// start has nothing to be told).
    fn digest_call(&self) -> Option<Effect> {
        let since = self.seen.looked().position;
        (since > 0).then(|| {
            call(
                method::WORK_DIGEST,
                json!({ "since": since }),
                Purpose::Digest,
            )
        })
    }

    /// The digest's one notice line, and the look it ends: the board is shown
    /// now (theseus-qkoi3).
    fn digest_said(&mut self, v: Value) {
        let Ok(r) = serde_json::from_value::<theseus_protocol::WorkDigestResult>(v) else {
            return;
        };
        let since = self.seen.looked().at_ms;
        if let Some(line) = theseus_protocol::digest_summary(&r.lines, since, &self.hm) {
            self.flash = Some((Tag::Ok, format!("{line} · theseus digest")));
        }
        self.seen.look(r.to, self.now_ms);
        self.save_at = Some(self.now_ms + SAVE_AFTER_MS);
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

    /// The failure on screen (theseus-7zgsm): the shown session, when it
    /// failed and asks nothing else of the owner, and the owner has not
    /// dismissed it.
    pub fn failure(&self) -> Option<&ExecutionView> {
        let v = self.board.view(self.shown()?)?;
        theseus_protocol::needs_dismissal(v).then_some(v)
    }

    /// The failure under the cursor: what `r` retries and `d` dismisses.
    fn selected_failure(&self) -> Option<&ExecutionView> {
        let v = self.board.view(self.selected.as_deref()?)?;
        theseus_protocol::needs_dismissal(v).then_some(v)
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
        // A held question's label turns over by the second, the minute, the hour.
        let waited = self
            .card()
            .and_then(|(q, _)| q.next_label_change(self.now_ms));
        // An elapsed time on a row repaints on the minute, never each second.
        let minute = self
            .board
            .work
            .ticking()
            .then(|| (self.now_ms / 60_000 + 1) * 60_000);
        [
            countdown,
            waited,
            self.notices.next_due(),
            self.save_at,
            minute,
        ]
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
                out.push(Effect::Notice {
                    title: f.title,
                    line: f.line,
                    sound,
                });
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

    /// The terminal's title, when it changed: `theseus ●1`, what needs you
    /// and nothing else; plain `theseus` when nothing does (theseus-k2vca).
    pub fn title(&mut self) -> Option<Effect> {
        let t = match self.board.tally(&|v| self.is_done(v)).needs {
            0 => "theseus".to_string(),
            n => format!("theseus ●{n}"),
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
        self.board.work.reconnected();
        self.board_threads.reset();
        let mut out = vec![
            snapshot(),
            call(method::HEALTH, Value::Null, Purpose::Health),
            work_watch(),
        ];
        // A thread chosen before: its row, for the status line.
        if self.thread.is_some() {
            out.push(call(method::THREAD_LIST, Value::Null, Purpose::Threads));
        }
        if let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) {
            out.extend(read_session(&sid));
            out.push(read_standing(&sid));
        }
        out.extend(self.follow_reconnected());
        if self.mode == Mode::SelfView {
            out.extend(self.self_read());
        }
        out.extend(self.digest_call());
        out
    }

    /// The connection closed, or a connect failed: the loop tries again.
    pub fn disconnected(&mut self, attempt: u32) {
        self.selfview.dropped();
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
        let mut out = self.self_moved(&event);
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
            Event::WorkChanged(c) => {
                for v in &c.views {
                    self.board.work.apply(v.clone());
                }
                for id in &c.gone {
                    self.board.work.gone(id);
                }
                self.system_changed(&c);
                self.horizon.apply(&c);
                self.board_threads.changed(c);
            }
            Event::ConfirmRequested(c) => self.board.confirm_requested(c),
            Event::ConfirmResolved(r) => {
                self.refused.remove(&r.correlation_id);
                self.board.confirm_resolved(&r);
            }
            Event::ActionOutputChunk(_) | Event::ActionOutputEnd(_) => {
                out.extend(self.follow_event(&event))
            }
            Event::EventsLost(lost) => {
                out.extend(self.follow_lost(&lost));
                out.extend(self.lost(&lost))
            }
            Event::WakeNoticed(n) => out.extend(self.noticed(&n)),
            // A system job that failed or ended after a long run: a line in
            // the footer, no ring (an Inform; the TUI's own notices are the
            // executions').
            Event::WorkNotice(n) if n.work.kind == theseus_protocol::work::WorkKind::System => {
                self.flash = Some((Tag::Warn, n.line));
            }
            _ => {}
        }
        out
    }

    /// A reminder fired (theseus-8ux6x): the policy's notice for it (an
    /// Interrupt, whatever the TUI shows: it is the owner's), the flash line
    /// and the ring, unless `--notify off`. Its four answers are the CLI's:
    /// `theseus answer ID done`, `theseus answer ID snooze`, until `work.answer` routes them.
    fn noticed(&mut self, n: &theseus_protocol::WakeNoticed) -> Vec<Effect> {
        let notice = notices::reminder_notice(n, &self.hm);
        self.flash = Some((Tag::Ask, notice.line.clone()));
        let viewer = Viewer {
            focused: None,
            seen: 0,
            away_ms: 0,
            quiet: self.quiet,
            sound: true,
        };
        match notices::deliver(&notice, &viewer) {
            notices::Delivery::Ping { sound } if !self.quiet => {
                vec![Effect::Notice {
                    title: "Reminder".into(),
                    line: notice.line,
                    sound,
                }]
            }
            _ => Vec::new(),
        }
    }

    /// The daemon's health: the notify kind its config names, and the sync
    /// warning (out of `answered`, within clippy's 100 lines).
    fn health_read(&mut self, v: Value) {
        self.config_notify(&v);
        self.sync_warn = sync_warning(v);
    }

    /// `[tui] notify` from `health`: how notices reach the owner when
    /// `--notify` did not say. A daemon that doesn't say leaves `auto`.
    fn config_notify(&mut self, health: &Value) {
        let Some(d) = health
            .get("tui_notify")
            .and_then(Value::as_str)
            .and_then(Delivery::parse)
        else {
            return;
        };
        self.tui_notify = Some(d);
        if !self.notify_pinned {
            self.quiet = d == Delivery::Off;
        }
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
        if all || lost.streams.iter().any(|s| s == "work") {
            self.board.work.reconnected();
            out.push(work_watch());
        }
        if self.mode == Mode::SelfView && all {
            out.extend(self.self_read());
        }
        if let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) {
            let stream = format!("session:{sid}");
            if all || lost.streams.contains(&stream) {
                out.extend(read_session(&sid));
            }
        }
        out
    }

    /// `executions.watch`'s snapshot (out of `answered`, within clippy's 100
    /// lines with tui-rows' and retry-dismiss's arms).
    fn snapshot_read(&mut self, v: Value) -> Vec<Effect> {
        match serde_json::from_value::<ExecutionsWatchResult>(v) {
            Ok(snap) => {
                // A snapshot's views go into the seen state, without
                // notices: a notice is for a transition seen as it
                // happens. The first board of a first start counts as
                // seen (`Seen`).
                self.position = self.position.max(snap.position);
                for v in self.board.snapshot(snap) {
                    self.took(&v, None);
                }
                // A first start has been shown nothing yet, and is
                // told nothing of what came before it.
                if self.seen.looked().position == 0 && self.term_focus {
                    self.seen.look(self.position, self.now_ms);
                }
                self.seen.first_board_read();
                self.follow_up()
            }
            Err(e) => {
                self.flash = Some((Tag::Bad, format!("the board did not decode: {e}")));
                Vec::new()
            }
        }
    }

    /// An answer to one of the TUI's requests.
    pub fn answered(&mut self, purpose: Purpose, result: Result<Value, RpcError>) -> Vec<Effect> {
        if purpose == Purpose::SelfLog {
            return self.self_answered(result);
        }
        let v = match result {
            Ok(v) => v,
            Err(e) => return self.request_failed(&purpose, &e),
        };
        match purpose {
            Purpose::Snapshot => return self.snapshot_read(v),
            Purpose::Work => self.work_read(v),
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
            Purpose::Dismiss { .. } | Purpose::Retry { .. } => self.failure_answered(purpose, v),
            Purpose::Answer { correlation_id, .. } => {
                self.refused.remove(&correlation_id);
                if let Ok(r) = serde_json::from_value::<WorkAnswerResult>(v) {
                    self.flash = Some(if r.option.starts_with("choice:") {
                        (Tag::Ok, format!("✓ answered {correlation_id}"))
                    } else if approves(&r.option) {
                        (Tag::Ok, format!("✓ approved {correlation_id}"))
                    } else {
                        (Tag::Warn, format!("✗ declined {correlation_id}"))
                    });
                }
            }
            // A turn's answer comes at its end; its events said the rest.
            // The turn's sources, when its reply cited any (theseus-ejdk).
            Purpose::Submit(sid) | Purpose::SubmitTask { task: sid, .. } => {
                return self.submitted(&sid, v)
            }
            Purpose::Inspect(sid) => self.inspected(&sid, v),
            Purpose::Standing(sid) => self.standing_read(&sid, v),
            Purpose::Health => self.health_read(v),
            Purpose::Threads => return self.threads_listed(v),
            Purpose::ThreadAct(_) => return self.thread_acted(v),
            Purpose::SubmitThread(_) => return self.thread_submitted(v),
            Purpose::NewTask => self.task_started(v),
            Purpose::Hold => self.task_held(v),
            Purpose::HorizonAct(_) => self.horizon_answered(purpose),
            Purpose::FollowFind | Purpose::FollowPeek => return self.follow_answered(&purpose, v),
            Purpose::Digest => self.digest_said(v),
            Purpose::Mode(sid) => self.mode_read(&sid, v),
            Purpose::Watch | Purpose::Typing | Purpose::SelfLog => {}
            Purpose::FollowStart | Purpose::Unfollow => {}
        }
        Vec::new()
    }

    /// A turn's answer, at its end (its events said the rest): the sources its
    /// reply cited (theseus-ejdk), the thread list again for a thread's session
    /// (theseus-j0yh), and the standing records, which may have changed while
    /// the turn ran (theseus-slvl.4).
    fn submitted(&mut self, sid: &str, v: Value) -> Vec<Effect> {
        let mut out = Vec::new();
        if let Ok(r) = serde_json::from_value::<theseus_protocol::TurnSubmitResult>(v) {
            if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == sid) {
                d.cited(&r.cites);
            }
            out = self.thread_replied(sid);
        }
        out.push(read_standing(sid));
        out
    }

    /// The answer to `execution.dismiss` or `task.retry` (theseus-7zgsm): a
    /// line in the session's pane and in the footer.
    fn failure_answered(&mut self, purpose: Purpose, v: Value) {
        match purpose {
            Purpose::Dismiss { session, execution } => {
                self.refused.remove(&execution);
                let line = format!("dismissed {}", short(&execution));
                self.note(&session, Tag::Dim, &line);
                self.flash = Some((Tag::Ok, format!("✓ {line}")));
            }
            Purpose::Retry { session, execution } => {
                self.refused.remove(&execution);
                if let Ok(r) = serde_json::from_value::<theseus_protocol::TaskRetryResult>(v) {
                    let line = format!(
                        "retried {} as {} with {} (it had {}); the failed one is dismissed",
                        short(&execution),
                        short(&r.task_id),
                        theseus_protocol::usd(r.limit_usd),
                        theseus_protocol::usd(r.failed_limit_usd)
                    );
                    self.note(&session, Tag::Ok, &line);
                    self.flash = Some((Tag::Ok, format!("✓ {line}")));
                }
            }
            _ => {}
        }
    }

    /// `session.mode`'s answer: the conversation's mode and plan, if it is
    /// still the one in focus.
    fn mode_read(&mut self, sid: &str, v: Value) {
        let r = serde_json::from_value::<theseus_protocol::modes::SessionModeResult>(v);
        if let (Ok(r), Some(d)) = (r, self.detail.as_mut().filter(|d| d.session_id == sid)) {
            d.set_mode(r.mode, &r.plan);
        }
    }

    /// A request that failed: say so where the operator looks.
    pub(crate) fn failed(&mut self, purpose: &Purpose, e: &RpcError) {
        // A notice, whose daemon may not know it (an older one): nothing to say.
        if matches!(
            purpose,
            Purpose::Typing
                | Purpose::Health
                | Purpose::Work
                | Purpose::Standing(_)
                | Purpose::Digest
        ) {
            return;
        }
        // An older daemon lacks `work.watch`: the rows say their labels, as
        // they did (theseus-k2vca). Nothing to tell the operator.
        if matches!(purpose, Purpose::Work) {
            self.board.work.live = false;
            return;
        }
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
        // A dismissal or a retry that did not count (theseus-7zgsm): the
        // failure's card says why, and the failure stays.
        if let Purpose::Dismiss { execution, .. } | Purpose::Retry { execution, .. } = purpose {
            if e.code == error_code::REFUSED {
                self.refused.insert(execution.clone(), e.message.clone());
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
            Purpose::Work => method::WORK_WATCH,
            Purpose::Titles => method::SESSION_LIST,
            Purpose::Questions => {
                self.asking_questions = false;
                method::CONFIRM_LIST
            }
            Purpose::History(_) | Purpose::Node { .. } => method::SESSION_HISTORY,
            Purpose::Watch => method::SESSION_WATCH,
            Purpose::Submit(_) | Purpose::SubmitTask { .. } => method::TURN_SUBMIT,
            Purpose::Inspect(_) => method::TURN_INSPECT,
            Purpose::Stop(_) => method::EXECUTION_STOP,
            Purpose::Cancel(_) => method::TASK_CANCEL,
            Purpose::NewTask => method::TASK_START,
            Purpose::Hold => "task.pause or task.resume",
            Purpose::Answer { .. } => method::WORK_ANSWER,
            Purpose::Dismiss { .. } => method::EXECUTION_DISMISS,
            Purpose::Retry { .. } => method::TASK_RETRY,
            Purpose::Typing => method::SESSION_TYPING,
            Purpose::Health => method::HEALTH,
            Purpose::Threads => method::THREAD_LIST,
            Purpose::ThreadAct(_) => "thread.pause, resume or done",
            Purpose::SubmitThread(_) => method::TURN_SUBMIT,
            Purpose::FollowFind => return self.follow_failed(method::ACTION_LIST, e),
            Purpose::FollowPeek => return self.follow_failed(method::WORK_PEEK, e),
            Purpose::FollowStart => return self.follow_failed(method::ACTION_OUTPUT, e),
            Purpose::Unfollow => return,
            Purpose::SelfLog => method::SELF_LOG,
            Purpose::Standing(_) => method::STANDING_LIST,
            Purpose::HorizonAct(_) => "the Horizon section",
            Purpose::Digest => method::WORK_DIGEST,
            Purpose::Mode(_) => method::SESSION_MODE,
        };
        let text = format!("{what}: {}", e.message);
        if let Purpose::Submit(sid)
        | Purpose::SubmitTask { task: sid, .. }
        | Purpose::Stop(sid)
        | Purpose::Cancel(sid) = purpose
        {
            self.note(sid, Tag::Bad, &format!("✗ {text}"));
        }
        self.flash = Some((Tag::Bad, text));
    }

    /// A line of the TUI's own in a session's pane, if it is in focus.
    pub(crate) fn note(&mut self, sid: &str, tag: Tag, text: &str) {
        if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == sid) {
            d.note(tag, text);
        }
    }

    /// What the board needs after a change: the titles of new sessions (and
    /// of the session in focus, for its profile), and the whole of any
    /// question it holds only in brief.
    fn follow_up(&mut self) -> Vec<Effect> {
        let mut out = Vec::new();
        let focus = self.detail.as_ref().map(|d| d.session_id.clone());
        let ids = self.board.untitled(focus.as_deref());
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
        self.standing = None;
        out.push(read_standing(sid));
        out.extend(self.follow_up());
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
        let ctrl_t = k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('t');
        if ctrl_t && matches!(self.mode, Mode::Normal | Mode::Input) {
            return self.open_picker();
        }
        match self.mode.clone() {
            Mode::Threads => self.picker_key(k),
            Mode::Horizon => self.horizon_key(k),
            Mode::Help | Mode::Standing => {
                self.mode = Mode::Normal;
                Vec::new()
            }
            Mode::SelfView => self.selfview_key(k),
            Mode::Filter => {
                self.filter_key(k);
                Vec::new()
            }
            Mode::Input => self.input_key(k),
            Mode::Armed(arm, sid) => self.armed_key(k, arm, &sid),
            Mode::Note(id) => self.note_key(k, &id),
            Mode::NewTask(home) => self.new_task_key(k, home),
            Mode::BoardThreads => self.board_threads_key(k),
            Mode::Normal => self.normal_key(k),
        }
    }

    /// `work.watch`'s snapshot (theseus-k2vca): the rows' words, the daemon's
    /// own jobs (theseus-jn17m) and the threads on the board (theseus-5i8i),
    /// all from the one watch.
    fn work_read(&mut self, v: Value) {
        match serde_json::from_value::<WorkWatchResult>(v) {
            Ok(snap) => {
                // The Horizon section reads the same watch (theseus-nuhsz).
                self.horizon.load(snap.clone());
                self.board.work.snapshot(snap.views.clone());
                self.system = snap
                    .views
                    .iter()
                    .filter(|w| w.kind == theseus_protocol::work::WorkKind::System)
                    .map(|w| (w.id.clone(), w.clone()))
                    .collect();
                self.board_threads.snapshot(snap);
            }
            // An answer this build can't read is no board: rows say
            // their labels, as with a daemon that lacks the method.
            Err(_) => self.board.work.live = false,
        }
    }

    /// The daemon's own jobs from a `work.changed` (theseus-jn17m): the system
    /// views, each kept by the position rule; a view gone leaves.
    fn system_changed(&mut self, c: &theseus_protocol::WorkChanged) {
        for v in c
            .views
            .iter()
            .filter(|v| v.kind == theseus_protocol::work::WorkKind::System)
        {
            if self
                .system
                .get(&v.id)
                .is_none_or(|o| v.position > o.position)
            {
                self.system.insert(v.id.clone(), v.clone());
            }
        }
        for id in &c.gone {
            self.system.remove(id);
        }
    }

    /// `standing.list`'s answer for `sid`, kept while that session is in focus.
    fn standing_read(&mut self, sid: &str, v: Value) {
        if self.detail.as_ref().is_some_and(|d| d.session_id == sid) {
            self.standing = Standing::read(sid, v);
        }
    }

    /// `p`: the standing records of the session in focus, read again.
    fn standing_key(&mut self) -> Vec<Effect> {
        let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) else {
            self.flash = Some((Tag::Dim, "open a session first (enter)".to_string()));
            return Vec::new();
        };
        self.mode = Mode::Standing;
        vec![read_standing(&sid)]
    }

    /// The header's pill: the live records that hold in the session in focus.
    pub fn standing_pill(&self) -> Option<String> {
        let d = self.detail.as_ref()?;
        self.standing
            .as_ref()
            .filter(|s| s.session == d.session_id)?
            .pill()
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
            KeyCode::Char('L') => return self.open_self(),
            // In a task, `p` and Backspace go back to the conversation that started it (theseus-mu0e6).
            KeyCode::Backspace => {
                if let Some(out) = self.parent_key() {
                    return out;
                }
            }
            KeyCode::Char('p') => return self.parent_key().unwrap_or_else(|| self.standing_key()),
            KeyCode::Char(c @ ('I' | 'P')) => return self.row_key(c),
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Enter => return self.enter_row(),
            KeyCode::Char('/') => self.mode = Mode::Filter,
            KeyCode::Char('N') => self.new_task_prompt(),
            // On a failure (theseus-7zgsm) `r` retries, `d` dismisses and `o`
            // opens; anywhere else they filter as before.
            KeyCode::Char('r')
                if self
                    .selected_failure()
                    .is_some_and(|v| v.kind == SessionKind::Task) =>
            {
                return self.failure_key(true);
            }
            KeyCode::Char('d') if self.selected_failure().is_some() => {
                return self.failure_key(false);
            }
            KeyCode::Char('o') if self.selected_failure().is_some() => {
                if let Some(sid) = self.selected.clone() {
                    return self.focus(&sid);
                }
            }
            KeyCode::Char('T') => match self.board_threads.is_empty() {
                true => self.flash = Some((Tag::Dim, "no threads on the board".to_string())),
                false => self.mode = Mode::BoardThreads,
            },
            KeyCode::Char('H') => self.focus_horizon(),
            KeyCode::Char('b') => self.only = Only::NeedsYou,
            KeyCode::Char('w') => self.only = Only::Working,
            KeyCode::Char('r') => self.only = Only::Ready,
            KeyCode::Char('d') => self.only = Only::Done,
            // `a` and `e` take the card's grant when it offers one
            // (theseus-drjs); else `a` shows every session.
            KeyCode::Char(k @ ('a' | 'e')) if self.grant_offered(k) => return self.grant_key(),
            KeyCode::Char('a') => {
                self.only = Only::All;
                self.filter.clear();
            }
            KeyCode::Esc if self.follow.is_some() => return self.follow_close(),
            KeyCode::Char('l') => return self.follow_key(),
            KeyCode::Esc => {
                if !self.wide() && self.pane == Pane::Session {
                    self.pane = Pane::List;
                } else {
                    self.only = Only::All;
                    self.filter.clear();
                }
            }
            KeyCode::Char('i') => {
                if self.on_screen().is_some() || self.sessionless_thread().is_some() {
                    self.mode = Mode::Input;
                }
            }
            KeyCode::Tab => return self.jump(true),
            KeyCode::BackTab => return self.jump(false),
            KeyCode::Char(c @ '1'..='6') => return self.choice_key(c),
            KeyCode::Char('y') => return self.answer_key(true),
            KeyCode::Char('n') => return self.answer_key(false),
            KeyCode::Char('s') => self.arm(Arm::Stop),
            KeyCode::Char('S') => self.arm(Arm::StopAll),
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

    /// The keys over the Threads block: the arrows move, enter folds a
    /// thread or opens an item's session, esc and `T` go back to the list.
    fn board_threads_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        let (now, width) = (self.now_ms, self.threads_width());
        match k.code {
            KeyCode::Down | KeyCode::Char('j') => self.board_threads.step(1, now, width),
            KeyCode::Up | KeyCode::Char('k') => self.board_threads.step(-1, now, width),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some(sid) = self.board_threads.session_at(now, width) {
                    self.mode = Mode::Normal;
                    return self.focus(&sid);
                }
                self.board_threads.toggle(now, width);
            }
            KeyCode::Esc | KeyCode::Char('T') | KeyCode::Char('q') => self.mode = Mode::Normal,
            _ => {}
        }
        Vec::new()
    }

    /// The columns the Threads block's lines have: the sidebar's.
    pub fn threads_width(&self) -> usize {
        match self.wide() {
            true => crate::ui::sidebar_width(self.width) as usize,
            false => self.width as usize,
        }
    }

    /// The session the keys act on: the one in focus, if its pane is on
    /// screen; else the footer says how to open one.
    pub(crate) fn on_screen(&mut self) -> Option<String> {
        match self.shown() {
            Some(sid) => Some(sid.to_string()),
            None => {
                self.flash = Some((Tag::Dim, "open a session first: enter".to_string()));
                None
            }
        }
    }

    /// `s`, `S` or `c`, once: the footer asks for the same key again (`S` also takes `y`).
    /// `c` acts on the row under the cursor when the list is on screen beside another open
    /// session, else on the open one (theseus-tlhjt); `s` and `S` keep the open one.
    fn arm(&mut self, arm: Arm) {
        let Some(open) = self.on_screen() else {
            return;
        };
        let sid = match arm {
            Arm::Cancel if self.wide() => {
                let rows = self.rows();
                self.cursor(&rows)
                    .map_or(open, |i| rows[i].session_id.clone())
            }
            _ => open,
        };
        // The work view may be trimmed (the job is not in it): the execution's state says then.
        let ended = match self.board.work.of_session(&sid) {
            Some(w) => w.state.ended(),
            None => self
                .board
                .view(&sid)
                .is_some_and(|v| crate::board::ended_state(&v.state)),
        };
        if arm == Arm::Cancel && ended {
            self.flash = Some((Tag::Dim, "that task has already finished".to_string()));
            return;
        }
        let kind = self.board.view(&sid).map(|v| v.kind);
        let refusal = match (arm, kind) {
            (Arm::Stop | Arm::StopAll, Some(SessionKind::Task)) => {
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

    /// `r` or `d` on the failure under the cursor: `task.retry`, or
    /// `execution.dismiss`.
    fn failure_key(&mut self, retry: bool) -> Vec<Effect> {
        let Some(v) = self.selected_failure() else {
            return Vec::new();
        };
        let (session, execution) = (v.session_id.clone(), v.execution_id.clone());
        self.refused.remove(&execution);
        let params = json!({ "execution_id": execution });
        let purpose = if retry {
            Purpose::Retry { session, execution }
        } else {
            Purpose::Dismiss { session, execution }
        };
        let m = if retry {
            method::TASK_RETRY
        } else {
            method::EXECUTION_DISMISS
        };
        vec![call(m, params, purpose)]
    }

    /// `y` or `n` on the card's question: `n` asks for a note first.
    fn answer_key(&mut self, approve: bool) -> Vec<Effect> {
        if self.on_screen().is_none() {
            return Vec::new();
        }
        let Some((q, _)) = self.card() else {
            self.flash = Some((Tag::Dim, "no question waits here".to_string()));
            return Vec::new();
        };
        let id = q.correlation_id().to_string();
        if q.is_ask() {
            self.flash = Some((
                Tag::Dim,
                "this question has options: press their number (1 to 6)".to_string(),
            ));
            return Vec::new();
        }
        if !q.may_answer() {
            self.flash = Some((
                Tag::Bad,
                "refused: not from here; answer from the CLI (`theseus answer`), the web UI, or a private place"
                    .to_string(),
            ));
            return Vec::new();
        }
        let yes = q.yes().to_string();
        if !approve {
            self.note.clear();
            self.mode = Mode::Note(id);
            return Vec::new();
        }
        vec![answer(&id, &yes, None)]
    }

    /// Whether the card on screen offers a grant whose key is `k`: `a` for a
    /// command's prefix, `e` for edits in the project (theseus-drjs).
    fn grant_offered(&self, k: char) -> bool {
        self.shown().is_some()
            && matches!(self.card(), Some((Question::Whole(c), _))
                if c.grant_offer.as_ref().is_some_and(|o| o.key() == k))
    }

    /// Approve the card's question and take its grant: the daemon grants the
    /// offer it made, never one this key names.
    fn grant_key(&mut self) -> Vec<Effect> {
        let Some((q, _)) = self.card() else {
            return Vec::new();
        };
        let id = q.correlation_id().to_string();
        if !q.may_answer() {
            self.flash = Some((
                Tag::Bad,
                "refused: not from here; answer from the CLI (`theseus answer`), the web UI, or a private place"
                    .to_string(),
            ));
            return Vec::new();
        }
        vec![answer(&id, theseus_protocol::grants::APPROVE_GRANT, None)]
    }

    /// `1` to `6` on a question with options (`owner_ask`, theseus-j7cje):
    /// that option, as `work.answer`.
    fn choice_key(&mut self, key: char) -> Vec<Effect> {
        if self.on_screen().is_none() {
            return Vec::new();
        }
        let Some((q, _)) = self.card() else {
            self.flash = Some((Tag::Dim, "no question waits here".to_string()));
            return Vec::new();
        };
        if !q.is_ask() {
            self.flash = Some((
                Tag::Dim,
                "this question has no numbered options".to_string(),
            ));
            return Vec::new();
        }
        if !q.may_answer() {
            self.flash = Some((
                Tag::Bad,
                "refused: not from here; answer from the CLI (`theseus answer`), the web UI, or a private place"
                    .to_string(),
            ));
            return Vec::new();
        }
        let n = key.to_digit(10).unwrap_or(0) as usize;
        let (id, option) = (
            q.correlation_id().to_string(),
            q.choice(n).map(str::to_string),
        );
        match option {
            Some(option) => vec![answer(&id, &option, None)],
            None => {
                self.flash = Some((Tag::Dim, format!("this question has no option {n}")));
                Vec::new()
            }
        }
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
                return vec![answer(id, "decline", Some(note).filter(|n| !n.is_empty()))];
            }
            _ => {}
        }
        Vec::new()
    }

    /// `tab` / `shift-tab`: the next or previous session that needs attention
    /// (design §2.2's queue), focused with its pane open. Both wrap.
    /// Shift+Tab: the open conversation's next mode, ask → edits → plan →
    /// auto, asked of the daemon, which says whether it counts.
    fn cycle_mode(&mut self) -> Vec<Effect> {
        let Some(d) = self.detail.as_ref() else {
            return Vec::new();
        };
        let params = json!({"session_id": d.session_id, "mode": d.mode.next()});
        vec![call(
            method::SESSION_MODE,
            params,
            Purpose::Mode(d.session_id.clone()),
        )]
    }

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
            (Arm::Stop, KeyCode::Char('s'))
                | (Arm::StopAll, KeyCode::Char('S' | 'y'))
                | (Arm::Cancel, KeyCode::Char('c'))
        );
        if !again {
            self.flash = Some((
                Tag::Dim,
                match arm {
                    Arm::Stop | Arm::StopAll => "not stopped",
                    Arm::Cancel => "not cancelled",
                }
                .to_string(),
            ));
            return Vec::new();
        }
        match arm {
            Arm::Stop | Arm::StopAll => {
                let Some(exe) = self.board.view(sid).map(|v| v.execution_id.clone()) else {
                    return Vec::new();
                };
                let mut params = json!({ "execution_id": exe, "author": AUTHOR });
                if arm == Arm::StopAll {
                    params["tasks"] = json!(true);
                }
                vec![call(
                    method::EXECUTION_STOP,
                    params,
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
            // At the prompt, Shift+Tab sets the conversation's mode (theseus-lyxy);
            // in the list it still walks back through the queue.
            KeyCode::BackTab => return self.cycle_mode(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
                return self.typing();
            }
            KeyCode::Enter => {
                let text = self.input.trim().to_string();
                if let Some(arg) = text
                    .strip_prefix("/t")
                    .filter(|r| r.chars().next().is_none_or(char::is_whitespace))
                {
                    return self.thread_line(arg.trim());
                }
                let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) else {
                    return self.first_thread_message(&text);
                };
                if text.is_empty() {
                    return Vec::new();
                }
                // A task that has reported takes no more: the message goes to main (theseus-mu0e6).
                if self.ended_task(&sid).is_some() {
                    return self.send_to_main(&sid, &text, "the task is over");
                }
                self.input.clear();
                self.mode = Mode::Normal;
                if let Some(d) = self.detail.as_mut() {
                    d.sent(&text);
                    d.scroll = 0;
                }
                self.steer_said(&sid);
                // On the profile the session's last turn ran on, carried and
                // not chosen, as `watch --interactive` sends it: without it the
                // daemon's live profile took the turn, another model without a
                // word (theseus-nu3z, theseus-0n1v).
                let mut params =
                    json!({ "session_id": sid, "input": text, "author": AUTHOR, "via": "tui" });
                if let Some(profile) = self.board.info(&sid).and_then(|i| i.profile.clone()) {
                    params["profile"] = json!(profile);
                    params["carried"] = json!(true);
                }
                let purpose = match self.task_parent(&sid) {
                    Some(_) => Purpose::SubmitTask { task: sid, text },
                    None => Purpose::Submit(sid),
                };
                return vec![call(method::TURN_SUBMIT, params, purpose)];
            }
            _ => {}
        }
        Vec::new()
    }

    /// The first key of an idle spell in the focused session's input line:
    /// one cheap `session.typing`, so the daemon warms what the message will
    /// wait for. Fire and forget (theseus-tnky).
    fn typing(&mut self) -> Vec<Effect> {
        let Some(sid) = self.detail.as_ref().map(|d| d.session_id.clone()) else {
            return Vec::new();
        };
        if !self.typist.keystroke(&sid, self.now_ms) {
            return Vec::new();
        }
        let params = json!({ "session_id": sid, "author": AUTHOR });
        vec![call(method::SESSION_TYPING, params, Purpose::Typing)]
    }

    /// Enter on the board: the row the cursor is on opens (the fold row folds or opens); when a filter
    /// hides it, the first row shown (theseus-0n1v).
    fn enter_row(&mut self) -> Vec<Effect> {
        if self.selected.is_some() {
            let rows = self.rows();
            let shown = match self.cursor(&rows) {
                Some(i) => rows.get(i),
                None => rows.first(),
            };
            if let Some(sid) = shown.map(|r| r.session_id.clone()) {
                if sid == crate::one_turn::FOLD_ID {
                    self.toggle_one_turn();
                    return Vec::new();
                }
                return self.focus(&sid);
            }
        }
        self.open_first()
    }

    /// Enter with no row under the cursor, as on a fresh start: the first
    /// row shown opens (theseus-8hcg).
    fn open_first(&mut self) -> Vec<Effect> {
        let rows = self.rows();
        match rows
            .iter()
            .find(|r| r.session_id != crate::one_turn::FOLD_ID)
        {
            Some(r) => self.focus(&r.session_id.clone()),
            None => Vec::new(),
        }
    }

    /// A paste (bracketed paste, theseus-8hcg): text, never keys, so a
    /// pasted `q` quits nothing and a pasted `y` answers no question. Typed
    /// into the input line with its line breaks, into the filter as one line,
    /// or into a decline's note; anywhere else it opens the input line with
    /// the text in it. It sends nothing and answers nothing: enter does. With
    /// no session open it is dropped, and the footer says so: kept, it went to
    /// whichever session opened next (theseus-8hcg's review, finding 5).
    pub fn paste(&mut self, text: &str) {
        let text = pasted(text);
        match self.mode {
            Mode::Input => self.input.push_str(&text),
            Mode::Filter => {
                self.filter.push_str(&text.replace(['\n', '\t'], " "));
                self.cursor_to_first_match();
            }
            Mode::Note(_) => self.note.push_str(&text),
            Mode::Threads => {
                if let Some(p) = self.picker.as_mut() {
                    p.filter.push_str(&text.replace(['\n', '\t'], " "));
                    p.cursor = 0;
                }
            }
            Mode::NewTask(_) => self.brief.push_str(&text.replace(['\n', '\t'], " ")),
            // The self view takes no paste.
            Mode::SelfView => {}
            Mode::Normal
            | Mode::Help
            | Mode::Standing
            | Mode::Horizon
            | Mode::Armed(..)
            | Mode::BoardThreads => {
                if let Mode::Armed(arm, _) = self.mode {
                    let not = match arm {
                        Arm::Stop | Arm::StopAll => "not stopped",
                        Arm::Cancel => "not cancelled",
                    };
                    self.flash = Some((Tag::Dim, not.to_string()));
                }
                self.mode = Mode::Normal;
                if self.shown().is_some() {
                    self.input.push_str(&text);
                    self.mode = Mode::Input;
                } else {
                    self.flash = Some((
                        Tag::Dim,
                        "paste dropped: no session is open (enter opens one, then paste)"
                            .to_string(),
                    ));
                }
            }
        }
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
                self.cursor_to_first_match();
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.cursor_to_first_match();
            }
            _ => {}
        }
    }

    /// A filter puts the cursor on the first row that matches it, so `N` after a filter goes under
    /// the conversation shown first.
    fn cursor_to_first_match(&mut self) {
        if let Some(r) = self.rows().first() {
            self.selected = Some(r.session_id.clone());
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
            Arm::StopAll => format!(
                "stop {name} (…{}) and cancel all its tasks? S or y again to do it; any other key: no",
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

/// A paste's text as the input line keeps it: a line break as `\n` (a
/// terminal sends `\r`), and no other control character.
fn pasted(text: &str) -> String {
    // One pass: a megabyte's paste is read once (theseus-8hcg's review).
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// `standing.list` for `sid` on this surface (theseus-slvl.4).
fn read_standing(sid: &str) -> Effect {
    call(
        method::STANDING_LIST,
        standing::params(sid),
        Purpose::Standing(sid.to_string()),
    )
}

pub(crate) fn call(method: &'static str, params: Value, purpose: Purpose) -> Effect {
    Effect::Call(Call {
        method,
        params,
        purpose,
    })
}

/// The OpenClaw sync's warning in a `health` answer (theseus-pjmj7): only that
/// is read, so an older daemon's answer has none.
fn sync_warning(v: Value) -> Option<String> {
    #[derive(serde::Deserialize, Default)]
    struct Sync {
        #[serde(default)]
        imported: Option<theseus_protocol::import::HealthImported>,
    }
    let h: Sync = serde_json::from_value(v).unwrap_or_default();
    h.imported.and_then(|i| i.sync).and_then(|s| s.warn)
}

/// `executions.watch`: the board's snapshot, then its events.
fn snapshot() -> Effect {
    call(method::EXECUTIONS_WATCH, json!({}), Purpose::Snapshot)
}

/// `work.watch`, as a screen a person looks at: the rows' words, then their
/// changes (theseus-k2vca).
fn work_watch() -> Effect {
    call(
        method::WORK_WATCH,
        json!({ "interactive": true }),
        Purpose::Work,
    )
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
        // Its mode and plan, which its notifications then keep (theseus-lyxy).
        call(
            method::SESSION_MODE,
            json!({ "session_id": sid }),
            Purpose::Mode(sid.to_string()),
        ),
    ]
}

/// An answer to a question (design §2.9, "Answering"): `work.answer` with
/// one of its options, as the TUI, so `[approval]` judges it as the `cli`
/// channel's. One path for every kind of question.
fn answer(id: &str, option: &str, note: Option<String>) -> Effect {
    let mut params = json!({
        "question_id": id,
        "option": option,
        "author": AUTHOR,
    });
    if let Some(n) = note {
        params["note"] = json!(n);
    }
    call(
        method::WORK_ANSWER,
        params,
        Purpose::Answer {
            correlation_id: id.to_string(),
        },
    )
}
