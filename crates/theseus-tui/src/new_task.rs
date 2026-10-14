//! `N`: start a task from the board (theseus-7oq0w). A one-line prompt for the brief;
//! enter sends one `task.start` under the conversation the cursor is on (or your
//! Home, when it is on a task or on nothing), with no model turn, and esc
//! leaves. The daemon answers with the task's short id; the board shows the task
//! when its first turn starts.

use crossterm::event::{KeyCode, KeyEvent};
use serde_json::json;
use theseus_client::render::Tag;
use theseus_protocol::task_start::TaskStartResult;
use theseus_protocol::{method, SessionKind};

use crate::app::{call, App, Effect, Mode, Purpose, AUTHOR};

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
                let mut params = json!({ "brief": brief, "author": AUTHOR });
                if let Some(h) = home {
                    params["home"] = json!(h);
                }
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
}
