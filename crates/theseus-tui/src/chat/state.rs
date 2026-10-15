//! The chat's state and every change to it. It does no I/O: a key, a paste, a
//! daemon message or the clock goes in, and the requests to send come out as
//! [`Effect`]s, with the finished lines in [`Chat::out`] for the scrollback
//! (the runner writes them once and never again).

use std::collections::HashSet;
use std::path::PathBuf;

use crossterm::event::KeyEvent;
use serde_json::{json, Value};
use theseus_protocol::{method, warm::Typist, ConfirmRequest};

use super::banner;
use super::commands;
use super::editor::Editor;
use super::keys::{self, Action};
use super::live::Live;
use super::render_seam::reply_lines;
use super::style::{one, render, span, wrap_words, Line, Sty};

/// What the chat's requests carry as their author.
pub const AUTHOR: &str = "the chat";

/// What the chat is told when it starts.
#[derive(Debug, Clone)]
pub struct Cfg {
    /// The directory it works in, absolute.
    pub dir: String,
    /// `dir` with the home written `~`.
    pub shown_dir: String,
    pub branch: Option<String>,
    /// A conversation to go on with, as the person named it.
    pub resume: Option<String>,
    pub color: bool,
    /// This directory's messages, oldest first.
    pub history: Vec<String>,
    pub history_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    Health,
    Open,
    /// The newest sessions, for the banner's last conversation.
    ListLast,
    /// All sessions, to find the one a person named.
    ListResume(String),
    History(String),
    Watch,
    Submit,
    Answer,
    Typing,
    /// Rewind (theseus-23rk): the picker's messages, a restore of the files,
    /// and a fork at the node before the message.
    RewindPoints,
    RewindRestore(super::rewind::Rewinding),
    RewindFork(super::rewind::Rewinding),
}

#[derive(Debug, Clone)]
pub struct Call {
    pub method: &'static str,
    pub params: Value,
    pub purpose: Purpose,
}

#[derive(Debug)]
pub enum Effect {
    Call(Call),
    /// Write this directory's history file.
    SaveHistory(Vec<String>),
    Quit,
}

pub(super) fn call(method: &'static str, params: Value, purpose: Purpose) -> Effect {
    Effect::Call(Call {
        method,
        params,
        purpose,
    })
}

/// How long the banner waits for the store's answer about the last
/// conversation before it shows without it.
const BANNER_WAIT_MS: u64 = 250;
/// A notice (a line in the dock) stays this long.
const NOTICE_MS: u64 = 4_000;

pub struct Chat {
    pub cfg: Cfg,
    pub width: u16,
    pub height: u16,
    pub now_ms: u64,
    pub editor: Editor,
    /// Finished lines for the scrollback, in order.
    pub out: Vec<String>,
    pub session: Option<String>,
    pub(super) profile: Option<String>,
    /// The model in the status line and the turn lines.
    pub model: Option<String>,
    pub cost: f64,
    pub mode: String,
    pub live: Live,
    pub question: Option<ConfirmRequest>,
    pub offline: bool,
    /// Tasks this session started that are open, from the task changes seen
    /// since the chat began (cli-tasks brings the board's own count).
    pub tasks: u32,
    pub(super) task_open: std::collections::HashMap<String, bool>,
    pub(super) mine: HashSet<String>,
    pub(super) ended: HashSet<String>,
    pub(super) answered_text: HashSet<String>,
    pub(super) proposed: std::collections::HashMap<String, (String, Value)>,
    pub(super) unsent: Vec<String>,
    pub(super) typist: Typist,
    pub(super) banner: Banner,
    /// A line in the dock for a few seconds, and when it goes.
    pub notice: Option<(String, u64)>,
    quit_armed: bool,
    /// Esc was pressed once on an idle, empty line (a second opens the picker).
    pub(super) esc_armed: bool,
    /// The rewind picker, while it is open.
    pub(super) picker: Option<super::rewind::Picker>,
    /// The conversation a rewind left, named once under the fork's history.
    pub(super) rewound: Option<String>,
    pub menu: usize,
    pub(super) started_ms: u64,
}

/// The banner waits for the daemon's words: the model, and the newest
/// sessions.
#[derive(Default)]
pub(super) struct Banner {
    pub(super) printed: bool,
    pub(super) model: Option<String>,
    pub(super) last: Option<Option<theseus_protocol::SessionInfo>>,
}

impl Chat {
    pub fn new(cfg: Cfg, now_ms: u64) -> Self {
        let editor = Editor::with_history(cfg.history.clone());
        Self {
            cfg,
            width: 80,
            height: 24,
            now_ms,
            editor,
            out: Vec::new(),
            session: None,
            profile: None,
            model: None,
            cost: 0.0,
            mode: "default".to_string(),
            live: Live::default(),
            question: None,
            offline: false,
            tasks: 0,
            task_open: Default::default(),
            mine: HashSet::new(),
            ended: HashSet::new(),
            answered_text: HashSet::new(),
            proposed: Default::default(),
            unsent: Vec::new(),
            typist: Typist::default(),
            banner: Banner::default(),
            notice: None,
            quit_armed: false,
            esc_armed: false,
            picker: None,
            rewound: None,
            menu: 0,
            started_ms: now_ms,
        }
    }

    // ---------------------------------------------------------- writing

    /// A finished line, styled, to the scrollback.
    pub fn say(&mut self, line: &Line) {
        let w = usize::from(self.width);
        self.out.push(render(line, w, self.cfg.color));
    }

    pub fn say_plain(&mut self, sty: Sty, text: &str) {
        self.say(&one(sty, text));
    }

    /// Words of our own, wrapped to the width.
    pub fn say_words(&mut self, sty: Sty, text: &str) {
        for row in wrap_words(text, usize::from(self.width), 2) {
            self.say_plain(sty, &row);
        }
    }

    /// A blank line.
    pub fn gap(&mut self) {
        self.out.push(String::new());
    }

    /// A line in the dock, for a few seconds.
    pub fn flash(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), self.now_ms + NOTICE_MS));
    }

    /// The reply, through the seam, whole.
    pub(super) fn say_reply(&mut self, text: &str) {
        let rows = reply_lines(text, usize::from(self.width), self.cfg.color);
        self.out.extend(rows);
    }

    // ---------------------------------------------------------- the start

    /// A connection is up: what to ask. Again after a reconnect, when the
    /// session is known, it only watches it again.
    pub fn connected(&mut self, now_ms: u64) -> Vec<Effect> {
        self.now_ms = now_ms;
        let was_offline = std::mem::take(&mut self.offline);
        if let Some(sid) = self.session.clone() {
            if was_offline {
                self.flash("reconnected");
            }
            return vec![call(
                method::SESSION_WATCH,
                json!({ "session_id": sid }),
                Purpose::Watch,
            )];
        }
        self.started_ms = now_ms;
        let mut out = vec![call(method::HEALTH, Value::Null, Purpose::Health)];
        match self.cfg.resume.clone() {
            None => {
                out.push(call(
                    method::SESSION_OPEN,
                    json!({ "dir": self.cfg.dir }),
                    Purpose::Open,
                ));
                out.push(call(
                    method::SESSION_LIST,
                    json!({ "n": 100 }),
                    Purpose::ListLast,
                ));
            }
            Some(given) => out.push(call(
                method::SESSION_LIST,
                Value::Null,
                Purpose::ListResume(given),
            )),
        }
        out
    }

    pub fn disconnected(&mut self) {
        if !self.offline {
            self.offline = true;
            self.say_plain(Sty::Yellow, "connection to theseusd lost; reconnecting…");
        }
    }

    /// The soonest time the chat wants the clock for: the dock's timer while
    /// something runs, the banner's wait, a notice's end.
    pub fn deadline(&self) -> Option<u64> {
        let mut at = Vec::new();
        if self.live.active() {
            // The dock's clock: the spinner and the timer move together.
            at.push((self.now_ms / 100 + 1) * 100);
        }
        if !self.banner.printed {
            at.push(self.started_ms + BANNER_WAIT_MS);
        }
        if let Some((_, until)) = &self.notice {
            at.push(*until);
        }
        at.into_iter().min()
    }

    /// The clock moved: print the banner it was waiting for, drop a notice
    /// that ran out. True when the dock changed.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.now_ms = now_ms;
        if !self.banner.printed && now_ms >= self.started_ms + BANNER_WAIT_MS {
            self.print_banner();
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, until)| now_ms >= *until)
        {
            self.notice = None;
        }
        self.live.active() || self.notice.is_some()
    }

    pub(super) fn print_banner(&mut self) {
        if self.banner.printed {
            return;
        }
        self.banner.printed = true;
        let model = self
            .banner
            .model
            .clone()
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| "model unknown".to_string());
        if self.model.is_none() {
            self.model = self.banner.model.clone();
        }
        let last = self
            .banner
            .last
            .clone()
            .flatten()
            .filter(|s| self.session.as_deref() != Some(s.session_id.as_str()));
        let facts = banner::Facts {
            model: &model,
            shown_dir: &self.cfg.shown_dir,
            branch: self.cfg.branch.as_deref(),
            last: last.as_ref().map(|s| (s, self.now_ms)),
        };
        for l in banner::lines(&facts) {
            self.say(&l);
        }
    }

    pub(super) fn banner_ready(&mut self) {
        let model_known = self.banner.model.is_some();
        let list_known = self.banner.last.is_some() || self.cfg.resume.is_some();
        if model_known && list_known {
            self.print_banner();
        }
    }

    // ---------------------------------------------------------- keys

    pub fn resized(&mut self, w: u16, h: u16) {
        self.width = w.max(20);
        self.height = h.max(4);
    }

    /// A paste is one event, never keys: a pasted `q` or `y` is text.
    pub fn paste(&mut self, text: &str) -> Vec<Effect> {
        self.quit_armed = false;
        self.editor.paste(text);
        self.menu = 0;
        self.typing()
    }

    pub fn key(&mut self, k: KeyEvent) -> Vec<Effect> {
        let Some(action) = keys::map(k) else {
            return Vec::new();
        };
        if !matches!(action, Action::Interrupt) {
            self.quit_armed = false;
        }
        if !matches!(action, Action::Escape) {
            self.esc_armed = false;
        }
        if self.picker.is_some() {
            return self.picker_key(action);
        }
        // A question waits and the line is empty: one key answers it.
        if self.editor.is_empty() {
            if let Action::Insert(c) = action {
                if let Some(effects) = self.question_key(c) {
                    return effects;
                }
            }
        }
        self.act(action)
    }

    fn act(&mut self, action: Action) -> Vec<Effect> {
        let w = usize::from(self.width);
        match action {
            Action::Insert('?') if self.editor.is_empty() => {
                self.help();
                return Vec::new();
            }
            Action::Insert(c) => {
                self.editor.insert(c);
                self.menu = 0;
                return self.typing();
            }
            Action::Submit => {
                if self.editor.continue_line() {
                    return Vec::new();
                }
                if let Some(name) = self.completing().filter(|n| self.menu_open_choice(n)) {
                    // Enter on a half-typed command runs the one chosen.
                    self.editor.clear();
                    self.editor.insert_str(&format!("/{name}"));
                }
                return self.submit();
            }
            Action::Newline => self.editor.newline(),
            Action::Backspace => {
                self.editor.backspace();
                self.menu = 0;
            }
            Action::Delete => self.editor.delete(),
            Action::Left => self.editor.left(),
            Action::Right => self.editor.right(),
            Action::Home => self.editor.home(),
            Action::End => self.editor.end(),
            Action::WordLeft => self.editor.word_left(),
            Action::WordRight => self.editor.word_right(),
            Action::KillToStart => self.editor.kill_to_start(),
            Action::KillToEnd => self.editor.kill_to_end(),
            Action::KillWord => self.editor.kill_word(),
            Action::Up => {
                if !self.suggestions().is_empty() {
                    self.menu = self.menu.saturating_sub(1);
                } else if !self.editor.up(w) {
                    self.editor.history_prev();
                }
            }
            Action::Down => {
                let n = self.suggestions().len();
                if n > 0 {
                    self.menu = (self.menu + 1).min(n - 1);
                } else if !self.editor.down(w) {
                    self.editor.history_next();
                }
            }
            Action::Tab => {
                if let Some(name) = self.completing() {
                    self.editor.clear();
                    self.editor.insert_str(&format!("/{name} "));
                }
            }
            Action::DoubleEscape => {
                let mut effects = self.act(Action::Escape);
                effects.extend(self.act(Action::Escape));
                return effects;
            }
            Action::Escape => {
                if let Some(effects) = self.rewind_escape() {
                    return effects;
                }
                self.menu = 0;
                self.notice = None;
            }
            Action::ShowPaste => {
                if let Some(text) = self.editor.paste_at_cursor().map(str::to_string) {
                    self.say_plain(Sty::Dim, "pasted text:");
                    for l in text.lines() {
                        let row = format!("  {l}");
                        self.say_plain(Sty::Dim, &row);
                    }
                } else {
                    self.flash("nothing pasted here");
                }
            }
            Action::Interrupt => return self.interrupt(),
            Action::EndOfInput => {
                if self.editor.is_empty() {
                    return self.leave();
                }
                self.editor.delete();
            }
        }
        Vec::new()
    }

    /// Ctrl-C: clear the line; on an empty one, the second leaves.
    fn interrupt(&mut self) -> Vec<Effect> {
        if !self.editor.is_empty() {
            self.editor.clear();
            self.quit_armed = false;
            return Vec::new();
        }
        if self.quit_armed {
            return self.leave();
        }
        self.quit_armed = true;
        self.flash("ctrl+c again to leave · the conversation goes on");
        Vec::new()
    }

    /// Leave, saying the conversation goes on.
    pub fn leave(&mut self) -> Vec<Effect> {
        if let Some(sid) = self.session.clone() {
            let end: String = sid
                .chars()
                .rev()
                .take(6)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let busy = self.live.active();
            let text = if busy {
                format!("left · the conversation and its work go on · theseus chat {end}")
            } else {
                format!("left · the conversation goes on · theseus chat {end}")
            };
            self.say_plain(Sty::Dim, &text);
        }
        vec![Effect::Quit]
    }

    // ---------------------------------------------------------- commands

    /// The commands whose names begin as the line does, when the line is a
    /// single word after a slash.
    pub fn suggestions(&self) -> Vec<&'static commands::Command> {
        let text = self.editor.text();
        match text.strip_prefix('/') {
            Some(rest) if !rest.contains(char::is_whitespace) => commands::matching(rest),
            _ => Vec::new(),
        }
    }

    fn completing(&self) -> Option<&'static str> {
        let s = self.suggestions();
        s.get(self.menu.min(s.len().saturating_sub(1)))
            .map(|c| c.name)
    }

    /// Enter chooses a suggestion only while the line is not already a
    /// whole command's name.
    fn menu_open_choice(&self, name: &str) -> bool {
        self.editor.text().trim_start_matches('/') != name
    }

    fn help(&mut self) {
        self.gap();
        for l in commands::help_lines() {
            self.say_plain(Sty::Dim, &l);
        }
        self.gap();
    }

    // ---------------------------------------------------------- sending

    /// Enter: a command runs, a message goes as one `turn.submit`.
    pub fn submit(&mut self) -> Vec<Effect> {
        let taken = self.editor.take();
        let (text, trimmed) = (taken.text.clone(), taken.text.trim());
        if trimmed.is_empty() {
            return Vec::new();
        }
        self.menu = 0;
        if let Some(rest) = trimmed.strip_prefix('/') {
            let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            if let Some(effects) = commands::run(self, name, args.trim()) {
                return effects;
            }
            // Not ours: a path or a sentence that starts with a slash goes
            // to the model; a word alone is a mistyped command.
            if !rest.contains('/') && !rest.contains(char::is_whitespace) && rest.len() <= 24 {
                self.flash(format!("no command /{name} · / lists them"));
                return Vec::new();
            }
        }
        self.echo(&taken.shown);
        let mut effects = vec![Effect::SaveHistory(self.editor.history().to_vec())];
        match self.session.clone() {
            Some(sid) => effects.push(self.submit_call(&sid, text)),
            None => self.unsent.push(text),
        }
        effects
    }

    /// The message in the transcript: `›` and its lines.
    fn echo(&mut self, text: &str) {
        self.gap();
        let mut first = true;
        for raw in text.trim_end().lines() {
            let rows = wrap_words(raw, usize::from(self.width).saturating_sub(2), 0);
            for row in rows {
                let lead = if first { "› " } else { "  " };
                first = false;
                let line = vec![
                    span(
                        if lead == "› " {
                            Sty::Cyan
                        } else {
                            Sty::Plain
                        },
                        lead,
                    ),
                    span(Sty::Bold, row),
                ];
                self.say(&line);
            }
        }
    }

    pub(super) fn submit_call(&self, sid: &str, text: String) -> Effect {
        let mut params = json!({ "session_id": sid, "input": text, "author": AUTHOR });
        if let Some(p) = &self.profile {
            params["profile"] = json!(p);
            params["carried"] = json!(true);
        }
        call(method::TURN_SUBMIT, params, Purpose::Submit)
    }

    /// The first key of an idle spell: one cheap `session.typing`.
    fn typing(&mut self) -> Vec<Effect> {
        let Some(sid) = self.session.clone() else {
            return Vec::new();
        };
        if !self.typist.keystroke(&sid, self.now_ms) {
            return Vec::new();
        }
        vec![call(
            method::SESSION_TYPING,
            json!({ "session_id": sid, "author": AUTHOR }),
            Purpose::Typing,
        )]
    }

    // ---------------------------------------------------------- questions

    /// `y`, `n`, `t` on a question, with the line empty. None: not a key
    /// for it.
    fn question_key(&mut self, c: char) -> Option<Vec<Effect>> {
        let q = self.question.as_ref()?;
        let id = q.correlation_id.clone();
        let option = match c {
            'y' | 'Y' => q
                .yes_option()
                .map_or(
                    if q.budget.is_some() {
                        "reset"
                    } else {
                        "approve"
                    },
                    |o| o.id.as_str(),
                )
                .to_string(),
            'n' | 'N' => "decline".to_string(),
            't' | 'T' if q.external_text.is_some() => "approve_trust".to_string(),
            _ => return None,
        };
        Some(vec![call(
            method::WORK_ANSWER,
            json!({ "question_id": id, "option": option, "author": AUTHOR }),
            Purpose::Answer,
        )])
    }
}
