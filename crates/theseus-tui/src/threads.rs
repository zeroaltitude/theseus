//! Threads in the TUI (theseus-j0yh): the current thread, the status line that
//! names it, the Ctrl-T picker, and `/t` in the input line. The state and its
//! keys are `App`'s (no I/O: they return the requests to send); the picker's
//! drawing is here too, so the whole feature reads in one file.
//!
//! The thread list is read when the picker opens, when `/t` is typed, at a
//! connect when a thread is current, and again after a reply to a message sent
//! in a thread (the status line's context size). Typing in the picker, moving
//! in it and closing it send nothing; `p`, `r` and `d` each send one request.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use serde_json::{json, Value};
use theseus_client::current::{self, Pick};
use theseus_client::render::thread as words;
use theseus_client::render::Tag;
use theseus_protocol::method;
use theseus_protocol::threads::{ThreadListResult, ThreadRow, ThreadState};

use crate::app::{call, App, Call, Effect, Mode, Purpose, AUTHOR};
use crate::ui::dim;

/// What `d` asks for twice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picker {
    pub filter: String,
    /// `/` was pressed: the letters that are commands type instead.
    pub typing: bool,
    pub cursor: usize,
    /// `a`: paused and done threads too.
    pub all: bool,
    /// `d` once, for this thread: `d` again finishes it.
    pub confirm: Option<String>,
}

impl Picker {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            typing: false,
            cursor: 0,
            all: false,
            confirm: None,
        }
    }
}

impl Default for Picker {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    /// The thread's row, by slug.
    pub fn thread_row(&self, slug: &str) -> Option<&ThreadRow> {
        self.threads.iter().find(|t| t.thread.slug == slug)
    }

    /// The thread whose session this is.
    pub fn thread_of(&self, sid: &str) -> Option<&ThreadRow> {
        self.threads
            .iter()
            .find(|t| t.thread.session_id.as_deref() == Some(sid))
    }

    /// The status line: `bh-ai pr · active · 41K ctx · warm`, or `home`.
    /// Nothing for a TUI that has never met a thread, so the header of one
    /// who uses none is as it was.
    pub fn thread_status(&self, width: usize) -> Option<String> {
        if self.thread.is_none() && self.threads.is_empty() {
            return None;
        }
        let row = self.thread.as_deref().and_then(|s| self.thread_row(s));
        Some(match (&self.thread, row) {
            (Some(slug), None) => slug.clone(),
            (_, row) => words::status_line(row, self.now_ms, width),
        })
    }

    /// The rows the picker shows, in the order they are numbered: active
    /// threads, else all with `a`, those the filter keeps.
    pub fn picker_rows(&self) -> Vec<&ThreadRow> {
        let p = self.picker.clone().unwrap_or_default();
        let f = p.filter.to_lowercase();
        self.threads
            .iter()
            .filter(|t| p.all || t.thread.state == ThreadState::Active)
            .filter(|t| {
                f.is_empty()
                    || t.thread.title.to_lowercase().contains(&f)
                    || t.thread.slug.contains(&f)
                    || t.thread
                        .persona
                        .as_deref()
                        .is_some_and(|x| x.to_lowercase().contains(&f))
            })
            .collect()
    }

    fn list_call(state: Option<&str>) -> Effect {
        let params = state.map_or(Value::Null, |s| json!({ "state": s }));
        call(method::THREAD_LIST, params, Purpose::Threads)
    }

    /// Ctrl-T: open the picker, and read the list.
    pub(crate) fn open_picker(&mut self) -> Vec<Effect> {
        self.mode = Mode::Threads;
        self.picker = Some(Picker::new());
        vec![Self::list_call(None)]
    }

    /// A `thread.list` answer.
    pub(crate) fn threads_listed(&mut self, v: Value) -> Vec<Effect> {
        let Ok(list) = serde_json::from_value::<ThreadListResult>(v) else {
            return Vec::new();
        };
        // The done ones, asked for by `a`, join the live ones; a list of the
        // live ones keeps the done ones it already had.
        let asked_done = list
            .threads
            .iter()
            .any(|t| t.thread.state == ThreadState::Done);
        let mut rows = list.threads;
        if !asked_done {
            rows.extend(
                self.threads
                    .iter()
                    .filter(|t| t.thread.state == ThreadState::Done)
                    .cloned(),
            );
        } else {
            rows.extend(
                self.threads
                    .iter()
                    .filter(|t| t.thread.state != ThreadState::Done)
                    .cloned(),
            );
            rows.dedup_by(|a, b| a.thread.slug == b.thread.slug);
        }
        rows.sort_by(|a, b| {
            (b.thread.last_active_ms, &a.thread.slug)
                .cmp(&(a.thread.last_active_ms, &b.thread.slug))
        });
        self.threads = rows;
        // A thread remembered from the last run, with a session: its pane,
        // when none is open.
        if self.detail.is_none() && !self.threads_focused {
            self.threads_focused = true;
            if let Some(sid) = self
                .thread
                .as_deref()
                .and_then(|s| self.thread_row(s))
                .and_then(|t| t.thread.session_id.clone())
            {
                return self.focus(&sid);
            }
        }
        Vec::new()
    }

    /// The current thread when it has no session yet: its first message
    /// opens one.
    pub(crate) fn sessionless_thread(&self) -> Option<String> {
        let slug = self.thread.as_deref()?;
        let t = self.thread_row(slug)?;
        (t.thread.session_id.is_none() && t.thread.state == ThreadState::Active)
            .then(|| slug.to_string())
    }

    /// Enter in the input line with no session open: the message goes to the
    /// current thread, which opens its session.
    pub(crate) fn first_thread_message(&mut self, text: &str) -> Vec<Effect> {
        self.mode = Mode::Normal;
        let Some(slug) = self.sessionless_thread() else {
            return Vec::new();
        };
        if text.is_empty() {
            self.mode = Mode::Input;
            return Vec::new();
        }
        self.input.clear();
        let params = json!({ "thread": slug, "input": text, "author": AUTHOR });
        vec![call(
            method::TURN_SUBMIT,
            params,
            Purpose::SubmitThread(slug),
        )]
    }

    /// Make `slug` the current thread: remembered for this terminal, and its
    /// session in focus when it has one.
    fn switch_to(&mut self, slug: Option<String>) -> Vec<Effect> {
        let mut out = Vec::new();
        self.thread = slug.clone();
        if let Some((path, terminal)) = self.thread_file.clone() {
            out.push(Effect::Remember(path, terminal, slug.clone()));
        }
        match slug.as_deref().and_then(|s| self.thread_row(s)).cloned() {
            Some(t) => match t.thread.session_id {
                Some(sid) => out.extend(self.focus(&sid)),
                None => {
                    self.flash = Some((
                        Tag::Dim,
                        format!(
                            "{}: no session yet; i types the first message",
                            t.thread.title
                        ),
                    ));
                }
            },
            None if slug.is_none() => {
                self.flash = Some((Tag::Dim, "home".to_string()));
            }
            None => {}
        }
        out
    }

    /// `/t` and `/t X`, typed in the input line.
    pub(crate) fn thread_line(&mut self, arg: &str) -> Vec<Effect> {
        self.input.clear();
        self.mode = Mode::Normal;
        if arg.is_empty() {
            return self.open_picker();
        }
        // The rows as the picker would number them.
        let live: Vec<ThreadRow> = self
            .threads
            .iter()
            .filter(|t| t.thread.state == ThreadState::Active)
            .cloned()
            .collect();
        match current::pick(arg, &live) {
            Ok(Pick::Home) => self.switch_to(None),
            Ok(Pick::Thread(r)) => {
                let slug = r.thread.slug.clone();
                self.switch_to(Some(slug))
            }
            Err(e) => {
                self.flash = Some((Tag::Bad, e.to_string()));
                Vec::new()
            }
        }
    }

    /// A key in the picker.
    pub(crate) fn picker_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let mut p = self.picker.clone().unwrap_or_default();
        let rows: Vec<ThreadRow> = self.picker_rows().into_iter().cloned().collect();
        let under = rows.get(p.cursor).map(|t| t.thread.slug.clone());
        let mut out = Vec::new();
        // `d` asked once: only `d` finishes it; any other key withdraws it.
        let confirming = p.confirm.take();
        match k.code {
            KeyCode::Char('t') if ctrl => self.close_picker(),
            KeyCode::Esc => {
                if confirming.is_none() {
                    self.close_picker();
                    return Vec::new();
                }
            }
            KeyCode::Down => p.cursor = (p.cursor + 1).min(rows.len().saturating_sub(1)),
            KeyCode::Up => p.cursor = p.cursor.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(slug) = under {
                    self.close_picker();
                    return self.switch_to(Some(slug));
                }
            }
            KeyCode::Backspace => {
                p.filter.pop();
                p.cursor = 0;
            }
            KeyCode::Char('/') if p.filter.is_empty() && !p.typing => p.typing = true,
            // The commands, while nothing is typed; once something is, letters type.
            KeyCode::Char(c) if p.filter.is_empty() && !p.typing && !ctrl && "prda".contains(c) => {
                match c {
                    'a' => {
                        p.all = !p.all;
                        p.cursor = 0;
                        if p.all {
                            out.push(Self::list_call(Some("done")));
                        }
                    }
                    'd' => match (&under, &confirming) {
                        (Some(s), Some(c)) if s == c => {
                            out.push(Self::act(method::THREAD_DONE, s));
                        }
                        (Some(s), _) => {
                            p.confirm = Some(s.clone());
                            self.flash = Some((
                                Tag::Warn,
                                format!("finish {s}? d again to confirm, any other key cancels"),
                            ));
                        }
                        _ => {}
                    },
                    'p' | 'r' => {
                        if let Some(s) = &under {
                            let m = if c == 'p' {
                                method::THREAD_PAUSE
                            } else {
                                method::THREAD_RESUME
                            };
                            out.push(Self::act(m, s));
                        }
                    }
                    _ => {}
                }
            }
            KeyCode::Char(c) if !ctrl => {
                p.filter.push(c);
                p.cursor = 0;
            }
            _ => {}
        }
        if self.mode == Mode::Threads {
            self.picker = Some(p);
        }
        out
    }

    fn close_picker(&mut self) {
        self.mode = Mode::Normal;
        self.picker = None;
        self.flash = None;
    }

    fn act(m: &'static str, slug: &str) -> Effect {
        Effect::Call(Call {
            method: m,
            params: json!({ "slug": slug, "author": AUTHOR }),
            purpose: Purpose::ThreadAct(slug.to_string()),
        })
    }

    /// A reply in a thread: the list again, for the status line's context size.
    pub(crate) fn thread_replied(&self, sid: &str) -> Vec<Effect> {
        if self.thread_of(sid).is_some() {
            return vec![call(method::THREAD_LIST, Value::Null, Purpose::Threads)];
        }
        Vec::new()
    }

    /// The turn that opened a thread's session answered: watch that session.
    pub(crate) fn thread_submitted(&mut self, v: Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<theseus_protocol::TurnSubmitResult>(v) else {
            return Vec::new();
        };
        let mut out = vec![call(method::THREAD_LIST, Value::Null, Purpose::Threads)];
        out.extend(self.focus(&r.session_id));
        out
    }

    /// A thread act answered: the list again, so the picker shows it.
    pub(crate) fn thread_acted(&mut self, v: Value) -> Vec<Effect> {
        if let Ok(r) = serde_json::from_value::<ThreadRow>(v) {
            self.flash = Some((
                Tag::Ok,
                format!("{} is {}", r.thread.title, r.thread.state.as_str()),
            ));
        }
        let mut out = vec![Self::list_call(None)];
        if self.picker.as_ref().is_some_and(|p| p.all) {
            out.push(Self::list_call(Some("done")));
        }
        out
    }
}

/// The picker, over the body.
pub fn draw_picker(app: &App, area: Rect, buf: &mut Buffer) {
    let Some(p) = &app.picker else { return };
    let rows = app.picker_rows();
    let w = area.width.saturating_sub(4).clamp(20, 110);
    let h = (rows.len() as u16 + 4)
        .clamp(5, area.height.max(5))
        .min(area.height);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let y = area.y + area.height.saturating_sub(h) / 2;
    let rect = Rect::new(x, y, w.min(area.width), h);
    buf.set_style(rect, Style::default().bg(Color::Black));
    let line = |buf: &mut Buffer, i: u16, text: &str, style: Style| {
        buf.set_stringn(
            rect.x,
            rect.y + i,
            " ".repeat(rect.width as usize),
            rect.width as usize,
            style,
        );
        buf.set_stringn(rect.x, rect.y + i, text, rect.width as usize, style);
    };
    let title = format!(
        " threads{}  ·  {} shown",
        if p.all { " (all)" } else { "" },
        rows.len()
    );
    line(
        buf,
        0,
        &title,
        Style::default().add_modifier(Modifier::BOLD),
    );
    let filter = format!(" /{}", p.filter);
    line(buf, 1, &filter, Style::default());
    let room = (rect.height as usize).saturating_sub(3);
    let top = p.cursor.saturating_sub(room.saturating_sub(1));
    if rows.is_empty() {
        line(buf, 2, " no threads match", dim());
    }
    for (i, t) in rows.iter().enumerate().skip(top).take(room) {
        let current = app.thread.as_deref() == Some(t.thread.slug.as_str());
        let when = (app.hm)(t.thread.last_active_ms);
        let text = words::picker_line(i + 1, t, &when, app.now_ms, current);
        let style = if i == p.cursor {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        line(buf, 2 + (i - top) as u16, &text, style);
    }
    let hint = match &p.confirm {
        Some(s) => format!(" finish {s}? d again · any other key cancels"),
        None => {
            " enter switch · p pause · r resume · d done · a all · / filter · esc close".to_string()
        }
    };
    let last = rect.height - 1;
    let shown: String = hint.chars().take(rect.width as usize).collect();
    line(buf, last, &shown, dim());
}
