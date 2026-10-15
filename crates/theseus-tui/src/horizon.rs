//! The Horizon section (theseus-nuhsz; horizon HZ8): the future under the
//! task tree. Reminders, routines, watches and deferred tasks, in the
//! sections `theseus agenda` shows (`theseus_client::render::horizon` words
//! both), kept from `work.watch`'s views and `work.changed` by the position
//! rule. `H` focuses it; there `j`/`k` move, `z` snoozes a reminder, `p`
//! pauses or resumes a reminder or a routine, `r` runs a routine now, `x`
//! cancels a reminder, a watch or a deferred task, and enter opens a
//! routine's conversation. Quiet by construction: it draws on events and sets
//! no timer of its own; the local midnight moves a reminder from This week to
//! Today at the next draw, because the day is this client's.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use serde_json::{json, Value};
use theseus_client::render::horizon::{build, is_future, lines_from_views, local_day, Row};
use theseus_client::render::Tag;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{method, WorkChanged, WorkWatchResult};

use crate::app::{call, App, Effect, Mode, Purpose};
use crate::ui::{dim, tag_style};

/// What the section holds: the future views, and the cursor's row.
#[derive(Debug, Default)]
pub struct Section {
    views: BTreeMap<String, WorkView>,
    /// The cursor's index among the items (headings are not items).
    sel: usize,
}

/// One drawn row of the section.
#[derive(Debug, Clone, PartialEq)]
pub enum Drawn {
    Heading(String),
    Item {
        text: String,
        tag: Tag,
        selected: bool,
    },
}

/// What a key asks of the app.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// Nothing, with a line to say why.
    Say(Tag, String),
    /// One request: its method and params, and the footer's words for it.
    Call(&'static str, Value, String),
    /// Open the conversation of this session.
    Open(String),
    None,
}

impl Section {
    /// The snapshot: the future views, and only those.
    pub fn load(&mut self, snap: WorkWatchResult) {
        self.views = snap
            .views
            .into_iter()
            .filter(is_future)
            .map(|v| (v.id.clone(), v))
            .collect();
    }

    /// A frame's changes, by the position rule.
    pub fn apply(&mut self, c: &WorkChanged) {
        for id in &c.gone {
            self.views.remove(id);
        }
        for v in &c.views {
            if !is_future(v) {
                self.views.remove(&v.id);
            } else if self
                .views
                .get(&v.id)
                .is_none_or(|o| v.position > o.position)
            {
                self.views.insert(v.id.clone(), v.clone());
            }
        }
    }

    /// The agenda as of `now_ms`: its sections, from the views alone.
    fn agenda(&self, now_ms: u64) -> theseus_client::render::horizon::Agenda {
        let views: Vec<WorkView> = self.views.values().cloned().collect();
        build(
            &views,
            &lines_from_views(&views),
            now_ms,
            &local_day(now_ms),
        )
    }

    /// The items, in order, headings left out.
    pub fn items(&self, now_ms: u64) -> Vec<Row> {
        self.agenda(now_ms)
            .sections
            .into_iter()
            .flat_map(|s| s.rows)
            .collect()
    }

    /// How many items there are.
    pub fn len(&self, now_ms: u64) -> usize {
        self.items(now_ms).len()
    }

    /// The section as drawn: a heading, then its rows.
    pub fn drawn(&self, now_ms: u64, focused: bool) -> Vec<Drawn> {
        let a = self.agenda(now_ms);
        let sel = self.sel.min(
            a.sections
                .iter()
                .map(|s| s.rows.len())
                .sum::<usize>()
                .saturating_sub(1),
        );
        let mut out = Vec::new();
        let mut at = 0;
        for s in a.sections {
            out.push(Drawn::Heading(theseus_client::render::horizon::heading(
                s.horizon,
            )));
            for r in s.rows {
                out.push(Drawn::Item {
                    text: r.compact,
                    tag: r.tag,
                    selected: focused && at == sel,
                });
                at += 1;
            }
        }
        out
    }

    /// Move the cursor, staying inside the items.
    pub fn step(&mut self, by: i32, now_ms: u64) {
        let n = self.len(now_ms);
        if n == 0 {
            self.sel = 0;
            return;
        }
        let to = (self.sel as i64 + i64::from(by)).clamp(0, n as i64 - 1);
        self.sel = to as usize;
    }

    /// The row under the cursor.
    pub fn selected(&self, now_ms: u64) -> Option<Row> {
        let items = self.items(now_ms);
        let at = self.sel.min(items.len().checked_sub(1)?);
        items.into_iter().nth(at)
    }

    /// A key in the section, other than moving.
    pub fn act(&self, key: char, now_ms: u64) -> Act {
        let Some(r) = self.selected(now_ms) else {
            return Act::Say(Tag::Dim, "nothing here".into());
        };
        let reminder = r.kind == WorkKind::Wake;
        let author = crate::app::AUTHOR;
        match (key, r.kind) {
            ('z', _) if reminder => Act::Call(
                method::WAKE_SNOOZE,
                json!({"wake": r.id, "author": author}),
                "snoozed 10 min".into(),
            ),
            ('z', _) => Act::Say(Tag::Dim, "only a reminder snoozes".into()),
            ('p', WorkKind::Routine) => {
                let m = if r.paused {
                    method::ROUTINE_RESUME
                } else {
                    method::ROUTINE_PAUSE
                };
                match r.session_id {
                    Some(id) => Act::Call(
                        m,
                        json!({"id": id, "author": author}),
                        if r.paused { "resumed" } else { "paused" }.into(),
                    ),
                    None => Act::Say(Tag::Warn, "this routine has no session yet".into()),
                }
            }
            ('p', _) if reminder => {
                let m = if r.paused {
                    method::WAKE_RESUME
                } else {
                    method::WAKE_PAUSE
                };
                Act::Call(
                    m,
                    json!({"wake": r.id, "author": author}),
                    if r.paused { "resumed" } else { "paused" }.into(),
                )
            }
            ('p', _) => Act::Say(Tag::Dim, "only a reminder or a routine pauses".into()),
            ('r', WorkKind::Routine) => match r.session_id {
                Some(id) => Act::Call(
                    method::ROUTINE_RUN_NOW,
                    json!({"id": id, "author": author}),
                    "running it now".into(),
                ),
                None => Act::Say(Tag::Warn, "this routine has no session yet".into()),
            },
            ('r', _) => Act::Say(Tag::Dim, "only a routine runs now".into()),
            ('x', WorkKind::Wake | WorkKind::Watch) => Act::Call(
                method::WAKE_CANCEL,
                json!({"wake": r.id, "author": author}),
                "cancelled".into(),
            ),
            ('x', WorkKind::Task) => match r.session_id {
                Some(id) => Act::Call(
                    method::TASK_CANCEL,
                    json!({"task": id, "author": author}),
                    "cancelled the task".into(),
                ),
                None => Act::None,
            },
            ('x', WorkKind::Routine) => Act::Say(
                Tag::Dim,
                "p pauses a routine; `theseus routine rm` ends it".into(),
            ),
            ('x', _) => Act::None,
            ('\n', WorkKind::Routine) => match r.session_id {
                Some(id) => Act::Open(id),
                None => Act::None,
            },
            ('\n', _) => Act::Say(Tag::Dim, "nothing to open here".into()),
            _ => Act::None,
        }
    }

    /// Whether the section has any item to draw.
    pub fn shown(&self, now_ms: u64) -> bool {
        !self.agenda(now_ms).sections.is_empty()
    }
}

/// The most rows the section takes of the sidebar: a third of its height.
fn room(height: u16) -> u16 {
    (height / 3).max(4)
}

impl App {
    /// `H`: give the section the keys, when it has anything to show.
    pub(crate) fn focus_horizon(&mut self) {
        if self.horizon.shown(self.now_ms) {
            self.mode = Mode::Horizon;
        } else {
            self.flash = Some((Tag::Dim, "nothing is ahead".to_string()));
        }
    }

    /// A key while the section has them.
    pub(crate) fn horizon_key(&mut self, k: KeyEvent) -> Vec<Effect> {
        let now = self.now_ms;
        match k.code {
            KeyCode::Esc | KeyCode::Char('H' | 'q') | KeyCode::Tab => self.mode = Mode::Normal,
            KeyCode::Down | KeyCode::Char('j') => self.horizon.step(1, now),
            KeyCode::Up | KeyCode::Char('k') => self.horizon.step(-1, now),
            KeyCode::Enter => return self.horizon_act('\n'),
            KeyCode::Char(c @ ('z' | 'p' | 'r' | 'x')) => return self.horizon_act(c),
            _ => {}
        }
        Vec::new()
    }

    /// The answer to the section's own requests: the views, or a key's done.
    pub(crate) fn horizon_answered(&mut self, purpose: Purpose) {
        if let Purpose::HorizonAct(done) = purpose {
            self.flash = Some((Tag::Ok, format!("✓ {done}")));
        }
    }

    fn horizon_act(&mut self, key: char) -> Vec<Effect> {
        match self.horizon.act(key, self.now_ms) {
            Act::Call(m, params, done) => vec![call(m, params, Purpose::HorizonAct(done))],
            Act::Say(tag, text) => {
                self.flash = Some((tag, text));
                Vec::new()
            }
            Act::Open(sid) => {
                self.mode = Mode::Normal;
                self.focus(&sid)
            }
            Act::None => Vec::new(),
        }
    }
}

/// Draw the section into `area`: a heading to each horizon, a row to each
/// item, the cursor's row reversed while the section has the keys. Rows that
/// do not fit are left off the bottom, the cursor's always kept.
pub fn draw(app: &App, area: Rect, buf: &mut Buffer) {
    let focused = app.mode == Mode::Horizon;
    let drawn = app.horizon.drawn(app.now_ms, focused);
    let height = area.height as usize;
    let at = drawn
        .iter()
        .position(|d| matches!(d, Drawn::Item { selected: true, .. }))
        .unwrap_or(0);
    let top = if at >= height { at + 1 - height } else { 0 };
    for (i, d) in drawn.iter().enumerate().skip(top).take(height) {
        let y = area.y + (i - top) as u16;
        let width = area.width as usize;
        match d {
            Drawn::Heading(h) => {
                buf.set_stringn(
                    area.x,
                    y,
                    format!(" {h}"),
                    width,
                    Style::default().add_modifier(Modifier::BOLD),
                );
            }
            Drawn::Item {
                text,
                tag,
                selected,
            } => {
                let base = if *selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    tag_style(*tag)
                };
                buf.set_style(Rect::new(area.x, y, area.width, 1), base);
                buf.set_stringn(area.x, y, format!("  {text}"), width, base);
            }
        }
    }
}

/// How many rows of the sidebar the section takes at this height: none while
/// it has nothing to show.
pub fn rows_for(app: &App, height: u16) -> u16 {
    let n = app.horizon.drawn(app.now_ms, false).len() as u16;
    if n == 0 {
        0
    } else {
        (n + 1).min(room(height))
    }
}

/// A rule above the section.
pub fn rule(area: Rect, buf: &mut Buffer) {
    let line = "─".repeat(area.width as usize);
    buf.set_stringn(area.x, area.y, line, area.width as usize, dim());
}
