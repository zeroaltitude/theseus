//! The task picker (theseus-rrkb7): ctrl-t opens the conversation's tasks as
//! a list with a cursor, in the dock in place of the input. ↑↓ move, Enter
//! inspects the task, `j` joins it, `c` cancels it (twice: the second `c`
//! confirms), `a` gives the keys to its question, Esc closes. The rows are
//! the board's views, then this conversation's ended tasks that the board no
//! longer holds, read once with `task.list` as the picker opens (never per
//! key, never per turn). Typing searches them (theseus-ypnxt.4): every word
//! of the query in a row's title or its report's first line, case blind. While
//! the query is empty `j`, `c` and `a` keep their keys; `/` starts a query
//! that begins with one of them, and Esc clears the query before it closes.

use serde_json::json;
use theseus_client::render::task_tree;
use theseus_protocol::work::{joinable, WorkKind, WorkState, WorkView};
use theseus_protocol::{method, TaskInfo, TaskListResult};

use super::dock::DockCtx;
use super::keys::Action;
use super::state::{call, Chat, Effect, Purpose};
use super::style::{one, Line, Sty};
use super::task_view::TaskRef;

/// Rows the picker shows at once.
const SHOWN: usize = 8;

#[derive(Default)]
pub(super) struct Picker {
    cursor: usize,
    /// `c` was pressed on this task (its id): the next `c` cancels it.
    cancel: Option<String>,
    /// What is typed: the rows are the ones that hold every word of it.
    query: String,
    /// A query has begun (by `/` or by a letter that is no command): letters
    /// are text until it is cleared, even `j`, `c` and `a`.
    typing: bool,
    /// This conversation's ended tasks, from `task.list`, once it has answered.
    past: Vec<TaskInfo>,
    /// The board has no task, so the picker waits for `task.list`.
    loading: bool,
}

/// A row: a task on the board, or an ended one only `task.list` still has.
#[derive(Clone)]
enum Pick {
    Live(Box<WorkView>),
    Past(Box<TaskInfo>),
}

impl Pick {
    fn title(&self) -> String {
        match self {
            Pick::Live(v) => task_tree::name(v),
            Pick::Past(i) => i.title.clone().unwrap_or_else(|| "untitled".to_string()),
        }
    }

    /// What a query is matched against: the title and the report's first line.
    fn words(&self) -> String {
        let report = match self {
            Pick::Live(v) => v.report.as_ref().and_then(|r| r.first_line.as_deref()),
            Pick::Past(_) => None,
        };
        format!("{} {}", self.title(), report.unwrap_or("")).to_lowercase()
    }
}

fn cursor_query_len(chat: &Chat) -> usize {
    chat.task_picker
        .as_ref()
        .map_or(0, |p| p.query.chars().count())
}

/// Whether a `task.list` row has ended.
fn ended(i: &TaskInfo) -> bool {
    crate::board::ended_state(&i.state)
}

impl Chat {
    /// The tasks of the board, what needs you first, then the longest waiting;
    /// then the ended ones `task.list` added, newest first; the ones that hold
    /// every word of the query.
    fn picker_rows(&self) -> Vec<Pick> {
        let mut live: Vec<&WorkView> = self
            .board
            .views
            .values()
            .filter(|v| v.kind == WorkKind::Task && v.session_id.is_some())
            .collect();
        live.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.clone()));
        let mut rows: Vec<Pick> = live
            .iter()
            .map(|v| Pick::Live(Box::new((*v).clone())))
            .collect();
        if let Some(p) = &self.task_picker {
            rows.extend(
                p.past
                    .iter()
                    .filter(|i| {
                        !live
                            .iter()
                            .any(|v| v.session_id.as_deref() == Some(i.task_id.as_str()))
                    })
                    .map(|i| Pick::Past(Box::new(i.clone()))),
            );
            let query = p.query.to_lowercase();
            rows.retain(|r| query.split_whitespace().all(|w| r.words().contains(w)));
        }
        rows
    }

    /// ctrl-t, and `/task` alone: the picker opens on the board's tasks and
    /// reads the ended ones once.
    pub(super) fn open_task_picker(&mut self) -> Vec<Effect> {
        let read = self.main_session().map(|sid| {
            call(
                method::TASK_LIST,
                json!({ "session_id": sid }),
                Purpose::PickerTasks,
            )
        });
        let live = self.picker_rows();
        if live.is_empty() && read.is_none() {
            self.show_tasks();
            return Vec::new();
        }
        let shown = self.view.as_ref().map(|v| v.task.session.as_str());
        let cursor = live
            .iter()
            .position(|r| matches!(r, Pick::Live(v) if v.session_id.as_deref() == shown))
            .unwrap_or(0);
        self.task_picker = Some(Picker {
            cursor,
            loading: live.is_empty(),
            ..Picker::default()
        });
        read.into_iter().collect()
    }

    /// The ended tasks of this conversation, as `task.list` answers. A picker
    /// that waited for them and has none to show gives way to the tree.
    pub(super) fn picker_tasks(&mut self, v: serde_json::Value) -> Vec<Effect> {
        let tasks = serde_json::from_value::<TaskListResult>(v)
            .map(|l| l.tasks)
            .unwrap_or_default();
        let Some(p) = self.task_picker.as_mut() else {
            return Vec::new();
        };
        p.past = tasks.into_iter().filter(ended).collect();
        p.loading = false;
        if self.picker_rows().is_empty() {
            self.task_picker = None;
            self.show_tasks();
        }
        Vec::new()
    }

    /// An older daemon, or a refusal: the board's tasks are all there is.
    pub(super) fn picker_tasks_failed(&mut self) -> Vec<Effect> {
        if let Some(p) = self.task_picker.as_mut() {
            p.loading = false;
        }
        if self.task_picker.is_some() && self.picker_rows().is_empty() {
            self.task_picker = None;
            self.show_tasks();
        }
        Vec::new()
    }

    /// A key while the picker is open.
    pub(super) fn task_picker_key(&mut self, action: Action) -> Vec<Effect> {
        let Some(cursor) = self.task_picker.as_ref().map(|p| p.cursor) else {
            return Vec::new();
        };
        let rows = self.picker_rows();
        let n = rows.len();
        let chosen = rows.get(cursor.min(n.saturating_sub(1))).cloned();
        let live = match chosen.clone() {
            Some(Pick::Live(v)) => Some(*v),
            _ => None,
        };
        let searching = self.task_picker.as_ref().is_some_and(|p| p.typing);
        let was_armed = self.task_picker.as_mut().and_then(|p| p.cancel.take());
        match action {
            Action::Up | Action::Down => {
                let to = if action == Action::Up {
                    cursor.saturating_sub(1)
                } else {
                    (cursor + 1).min(n.saturating_sub(1))
                };
                if let Some(p) = self.task_picker.as_mut() {
                    p.cursor = to;
                }
            }
            Action::Escape if searching => self.search(|q| q.clear(), false),
            Action::Escape | Action::Tasks | Action::Interrupt => self.task_picker = None,
            Action::Backspace if searching => {
                let last = cursor_query_len(self) <= 1;
                self.search(
                    |q| {
                        q.pop();
                    },
                    !last,
                )
            }
            Action::Submit => return self.picked(chosen, false),
            Action::Insert('j') if !searching => return self.picked(chosen, true),
            Action::Insert('c') if !searching => {
                let Some(v) = live.filter(|v| !v.state.ended()) else {
                    self.flash("that task has ended");
                    return Vec::new();
                };
                let short = task_tree::short_id(v.session_id.as_deref().unwrap_or(&v.id));
                if was_armed.as_deref() == Some(v.id.as_str()) {
                    self.task_picker = None;
                    return self.cancel_task(&short);
                }
                if let Some(p) = self.task_picker.as_mut() {
                    p.cancel = Some(v.id);
                }
                self.flash(format!("c again cancels {short} · any other key keeps it"));
            }
            Action::Insert('a') if !searching => {
                let q = live.and_then(|v| v.question.map(|q| q.id));
                let Some(id) = q else {
                    self.flash("that task asks nothing");
                    return Vec::new();
                };
                self.task_picker = None;
                self.ask.focus = Some(id);
                self.ask.typing = false;
            }
            // `/` starts a query that begins with a key of its own.
            Action::Insert('/') if !searching => self.search(|_| {}, true),
            Action::Insert(c) if !c.is_control() => self.search(|q| q.push(c), true),
            _ => {}
        }
        Vec::new()
    }

    /// Change the query; the cursor goes back to the first row it leaves.
    fn search(&mut self, edit: impl FnOnce(&mut String), typing: bool) {
        if let Some(p) = self.task_picker.as_mut() {
            edit(&mut p.query);
            p.typing = typing;
            p.cursor = 0;
            p.cancel = None;
        }
    }

    fn picked(&mut self, chosen: Option<Pick>, join: bool) -> Vec<Effect> {
        let (t, joins) = match chosen {
            None => return Vec::new(),
            Some(Pick::Live(v)) => {
                let Some(t) = TaskRef::of(&v) else {
                    return Vec::new();
                };
                (t, joinable(&v))
            }
            Some(Pick::Past(i)) => (
                TaskRef {
                    id: i.execution_id.clone(),
                    session: i.task_id.clone(),
                    title: i.title.clone().unwrap_or_default(),
                },
                false,
            ),
        };
        if join && !joins {
            self.flash(format!("{} has ended · enter shows it", t.short()));
            return Vec::new();
        }
        self.open_task(t, join)
    }
}

/// The picker's rows, with the cursor, and its keys under them.
pub fn rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(p) = chat.task_picker.as_ref() else {
        return Vec::new();
    };
    let views = chat.picker_rows();
    let at = p.cursor.min(views.len().saturating_sub(1));
    let first = at
        .saturating_sub(SHOWN - 1)
        .min(views.len().saturating_sub(SHOWN));
    let head = if p.query.is_empty() {
        "tasks · type to search".to_string()
    } else {
        format!("tasks · search: {}", p.query)
    };
    let mut out = vec![one(Sty::Cyan, head)];
    if views.is_empty() {
        let note = if p.loading {
            "  reading the tasks…"
        } else {
            "  no task matches"
        };
        out.push(one(Sty::Dim, note));
    }
    for (i, r) in views.iter().enumerate().skip(first).take(SHOWN) {
        let tag = if i == at { "▸" } else { " " };
        let (text, sty) = match r {
            Pick::Live(v) => live_row(v, tag, ctx.now_ms, i == at),
            Pick::Past(t) => past_row(t, tag, i == at),
        };
        out.push(one(
            sty,
            theseus_protocol::term::words::clip(&text, ctx.width),
        ));
    }
    out.push(one(
        Sty::Dim,
        "↑↓ move · enter inspect · j join · c cancel · a answer · esc close",
    ));
    out
}

fn live_row(v: &WorkView, tag: &str, now_ms: u64, at: bool) -> (String, Sty) {
    let short = task_tree::short_id(v.session_id.as_deref().unwrap_or(&v.id));
    let end = v.ended_at_ms.unwrap_or(now_ms);
    let mut text = format!(
        "{tag} {} {short} {} · {} {}",
        task_tree::mark(v, v.question.is_some()),
        task_tree::name(v),
        v.state.as_str(),
        task_tree::coarse(end.saturating_sub(v.started_at_ms)),
    );
    if let Some(c) = &v.cost {
        text.push_str(&format!(" · {}", theseus_protocol::usd(c.spent_usd)));
    }
    let sty = match v.state {
        _ if at => Sty::Cyan,
        _ if v.question.is_some() => Sty::Yellow,
        WorkState::Failed => Sty::Red,
        WorkState::Done => Sty::Green,
        s if s.working() => Sty::Plain,
        _ => Sty::Dim,
    };
    (text, sty)
}

/// An ended task that only `task.list` still has: its title, how it ended and
/// what it cost.
fn past_row(t: &TaskInfo, tag: &str, at: bool) -> (String, Sty) {
    let (mark, word, sty) = match t.state.as_str() {
        "complete" => ("✓", "done", Sty::Green),
        "cancelled" => ("–", "cancelled", Sty::Dim),
        _ => ("✗", "failed", Sty::Red),
    };
    let text = format!(
        "{tag} {mark} {} {} · {word} {} · {}",
        t.short,
        t.title.as_deref().unwrap_or("untitled"),
        task_tree::coarse(t.updated_at_ms.saturating_sub(t.created_at_ms)),
        theseus_protocol::usd(t.spent_usd),
    );
    (text, if at { Sty::Cyan } else { sty })
}
