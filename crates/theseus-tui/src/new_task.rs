//! `N`: start a task from the board (theseus-7oq0w). A one-line prompt for the brief;
//! enter sends one `task.start` under the conversation the cursor is on (or your
//! Home, when it is on a task or on nothing), with no model turn, and esc
//! leaves. The daemon answers with the task's short id; the board shows the task
//! when its first turn starts. `P` holds the selected task at its next loop and
//! lets it go on (`task.pause`, `task.resume`; theseus-ypnxt.4).

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::json;
use theseus_client::render::Tag;
use theseus_protocol::task_pause::TaskPauseResult;
use theseus_protocol::task_start::TaskStartResult;
use theseus_protocol::{method, SessionKind};

use crate::app::{call, App, Effect, Mode, Purpose, AUTHOR};

/// `task.start`'s params for a brief typed by a person: the one shape the TUI's
/// `N` and the chat's `/task new` both send, under `home` (Home when none).
pub(crate) fn start_params(brief: &str, home: Option<&str>, author: &str) -> serde_json::Value {
    let mut params = json!({ "brief": brief, "author": author });
    if let Some(h) = home {
        params["home"] = json!(h);
    }
    params
}

impl App {
    /// `N` on the board: open the prompt for the brief, remembering the
    /// conversation the task will go under.
    pub(crate) fn new_task_prompt(&mut self) {
        let home = self.selected.clone().filter(|sid| {
            self.board
                .info(sid)
                .is_some_and(|i| i.kind != SessionKind::Task)
        });
        self.brief.clear();
        self.mode = Mode::NewTask(home);
    }

    /// The prompt's keys: type; enter starts the task; esc starts nothing.
    pub(crate) fn new_task_key(&mut self, k: KeyEvent, home: Option<String>) -> Vec<Effect> {
        match k.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.brief.clear();
                self.flash = Some((Tag::Dim, "no task started".to_string()));
            }
            KeyCode::Backspace => {
                self.brief.pop();
            }
            KeyCode::Char(c) => self.brief.push(c),
            KeyCode::Enter => {
                let brief = self.brief.trim().to_string();
                if brief.is_empty() {
                    return Vec::new();
                }
                self.brief.clear();
                self.mode = Mode::Normal;
                let params = start_params(&brief, home.as_deref(), AUTHOR);
                return vec![call(method::TASK_START, params, Purpose::NewTask)];
            }
            _ => {}
        }
        Vec::new()
    }

    /// The daemon's answer to a start.
    pub(crate) fn task_started(&mut self, v: serde_json::Value) {
        if let Ok(r) = serde_json::from_value::<TaskStartResult>(v) {
            self.flash = Some((
                Tag::Ok,
                format!(
                    "started task {} (${:.2}, no model turn): it appears on the board when it runs",
                    r.short, r.limit_usd
                ),
            ));
        }
    }

    /// `I` (the inspector's link) and `P` (hold or release) on the row under the cursor.
    pub(crate) fn row_key(&mut self, key: char) -> Vec<Effect> {
        match key {
            'P' => self.hold_key(),
            _ => self.inspect_key(),
        }
    }

    /// `P` on a task's row: hold it at its next loop, or let it go on when it
    /// is held already. One call, no model turn.
    pub(crate) fn hold_key(&mut self) -> Vec<Effect> {
        let task = self.selected.clone().filter(|sid| {
            self.board
                .info(sid)
                .is_some_and(|i| i.kind == SessionKind::Task)
        });
        let Some(task) = task else {
            self.flash = Some((
                Tag::Dim,
                "P pauses or resumes a task: select one".to_string(),
            ));
            return Vec::new();
        };
        let on = !self.board.work.of_session(&task).is_some_and(|w| w.paused);
        let m = if on {
            method::TASK_PAUSE
        } else {
            method::TASK_RESUME
        };
        vec![call(
            m,
            json!({ "task": task, "author": AUTHOR }),
            Purpose::Hold,
        )]
    }

    /// The daemon's answer to a pause or a resume.
    pub(crate) fn task_held(&mut self, v: serde_json::Value) {
        if let Ok(r) = serde_json::from_value::<TaskPauseResult>(v) {
            let (verb, was) = if r.paused {
                ("pausing", "paused")
            } else {
                ("resuming", "running")
            };
            let text = if r.changed {
                format!(
                    "{verb} task {}",
                    theseus_client::render::task_tree::short_id(&r.task_id)
                )
            } else {
                format!(
                    "task {} was {was} already",
                    theseus_client::render::task_tree::short_id(&r.task_id)
                )
            };
            self.flash = Some((Tag::Ok, text));
        }
    }
}
