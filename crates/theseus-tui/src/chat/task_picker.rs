//! The task picker (theseus-rrkb7): ctrl-t opens the conversation's tasks as
//! a list with a cursor, in the dock in place of the input. ↑↓ move, Enter
//! inspects the task, `j` joins it, `c` cancels it (twice: the second `c`
//! confirms), `a` gives the keys to its question, Esc closes. The rows are
//! the board's views, so the picker asks the daemon nothing.

use theseus_client::render::task_tree;
use theseus_protocol::work::{joinable, WorkKind, WorkState, WorkView};

use super::dock::DockCtx;
use super::keys::Action;
use super::state::{Chat, Effect};
use super::style::{one, Line, Sty};
use super::task_view::TaskRef;

/// Rows the picker shows at once.
const SHOWN: usize = 8;

#[derive(Default)]
pub(super) struct Picker {
    cursor: usize,
    /// `c` was pressed on this task (its id): the next `c` cancels it.
    cancel: Option<String>,
}

impl Chat {
    /// The tasks of the board, what needs you first, then the longest waiting.
    fn picker_rows(&self) -> Vec<&WorkView> {
        let mut rows: Vec<&WorkView> = self
            .board
            .views
            .values()
            .filter(|v| v.kind == WorkKind::Task && v.session_id.is_some())
            .collect();
        rows.sort_by_key(|v| (v.attention.level.rank(), v.state_since_ms, v.id.clone()));
        rows
    }

    /// ctrl-t, and `/task` alone.
    pub(super) fn open_task_picker(&mut self) -> Vec<Effect> {
        let rows = self.picker_rows();
        if rows.is_empty() {
            self.show_tasks();
            return Vec::new();
        }
        let shown = self.view.as_ref().map(|v| v.task.session.as_str());
        let cursor = rows
            .iter()
            .position(|v| v.session_id.as_deref() == shown)
            .unwrap_or(0);
        self.task_picker = Some(Picker {
            cursor,
            cancel: None,
        });
        Vec::new()
    }

    /// A key while the picker is open.
    pub(super) fn task_picker_key(&mut self, action: Action) -> Vec<Effect> {
        let Some(cursor) = self.task_picker.as_ref().map(|p| p.cursor) else {
            return Vec::new();
        };
        let rows = self.picker_rows();
        let n = rows.len();
        let chosen = rows
            .get(cursor.min(n.saturating_sub(1)))
            .map(|v| (*v).clone());
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
            Action::Escape | Action::Tasks | Action::Interrupt => self.task_picker = None,
            Action::Submit => return self.picked(chosen, false),
            Action::Insert('j') => return self.picked(chosen, true),
            Action::Insert('c') => {
                let Some(v) = chosen.filter(|v| !v.state.ended()) else {
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
            Action::Insert('a') => {
                let q = chosen.and_then(|v| v.question.map(|q| q.id));
                let Some(id) = q else {
                    self.flash("that task asks nothing");
                    return Vec::new();
                };
                self.task_picker = None;
                self.ask.focus = Some(id);
                self.ask.typing = false;
            }
            _ => {}
        }
        Vec::new()
    }

    fn picked(&mut self, chosen: Option<WorkView>, join: bool) -> Vec<Effect> {
        let Some(v) = chosen else { return Vec::new() };
        let Some(t) = TaskRef::of(&v) else {
            return Vec::new();
        };
        if join && !joinable(&v) {
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
    let mut out = vec![one(Sty::Cyan, "tasks")];
    for (i, v) in views.iter().enumerate().skip(first).take(SHOWN) {
        let tag = if i == at { "▸" } else { " " };
        let short = task_tree::short_id(v.session_id.as_deref().unwrap_or(&v.id));
        let end = v.ended_at_ms.unwrap_or(ctx.now_ms);
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
            _ if i == at => Sty::Cyan,
            _ if v.question.is_some() => Sty::Yellow,
            WorkState::Failed => Sty::Red,
            WorkState::Done => Sty::Green,
            s if s.working() => Sty::Plain,
            _ => Sty::Dim,
        };
        out.push(one(sty, task_tree::cut(&text, ctx.width)));
    }
    out.push(one(
        Sty::Dim,
        "↑↓ move · enter inspect · j join · c cancel · a answer · esc close",
    ));
    out
}
