//! Inspect, join and return for a task's pane (theseus-mu0e6). A task is its own session, so the pane
//! already shows it and `i` already types to it; this adds the way back and the words around it.
//! `p` or Backspace focuses the conversation that started the task, `I` asks for the inspector link of
//! the session's newest turn, the input line says the task reads a message at its next step, and a
//! message for a task that has reported goes to its parent with the shared redirect line. A client's
//! view only: nothing here is stored.

use serde_json::json;
use theseus_client::render::Tag;
use theseus_protocol::inspect::TurnInspectResult;
use theseus_protocol::work::{joinable, task_redirect_line, WorkView};
use theseus_protocol::{error_code, method, RpcError, SessionKind};

use crate::app::{call, App, Effect, Purpose, AUTHOR};
use crate::board::short;

/// What the input line says while it types to a task that works.
const STEER: &str = "the task reads this at its next step";

/// Whether a refusal says the task has ended (`execution_complete` and its kin), as Discord reads it.
fn task_is_over(e: &RpcError) -> bool {
    let class = e.data.get("class").and_then(serde_json::Value::as_str);
    e.code == error_code::NOT_FOUND || class.is_some_and(|c| c.starts_with("execution_"))
}

impl App {
    /// The conversation that started a task, when `sid` is a task whose view says.
    pub(crate) fn task_parent(&self, sid: &str) -> Option<String> {
        let v = self
            .board
            .view(sid)
            .filter(|v| v.kind == SessionKind::Task)?;
        v.parent_session_id.clone()
    }

    /// Whether a task takes input now. A task the board does not hold is assumed to.
    fn takes_input(&self, sid: &str) -> bool {
        self.board
            .view(sid)
            .is_none_or(|v| joinable(&WorkView::from_execution(v)))
    }

    /// The session on screen is a task that has reported and has a parent to write to instead.
    pub(crate) fn ended_task(&self, sid: &str) -> Option<String> {
        self.task_parent(sid).filter(|_| !self.takes_input(sid))
    }

    /// `p` or Backspace: the conversation that started the task on screen. `None` when the session on
    /// screen is no task, so `p` keeps its other meaning.
    pub(crate) fn parent_key(&mut self) -> Option<Vec<Effect>> {
        let task = self.shown()?.to_string();
        let parent = self.task_parent(&task)?;
        let state = self
            .board
            .view(&task)
            .map_or("", |v| v.state.as_str())
            .to_string();
        let line = format!(
            "back in {} · task {} {state}",
            self.board.name(&parent),
            short(&task)
        );
        let out = self.focus(&parent);
        self.flash = Some((Tag::Dim, line));
        Some(out)
    }

    /// The breadcrumb that heads a task's pane (`task a1b2c3 of <parent> · p parent · I inspector`).
    pub(crate) fn breadcrumb(&self, sid: &str) -> Option<String> {
        let parent = self.task_parent(sid)?;
        Some(format!(
            "task {} of {} · p parent · I inspector",
            short(sid),
            self.board.name(&parent)
        ))
    }

    /// What the input line's prompt says: where the message goes.
    pub(crate) fn input_prompt(&self) -> String {
        let Some(sid) = self.detail.as_ref().map(|d| d.session_id.as_str()) else {
            return "> ".into();
        };
        if self.ended_task(sid).is_some() {
            return format!("task {} reported; to main> ", short(sid));
        }
        match self.task_parent(sid) {
            Some(_) => format!("task {} reads it at its next step> ", short(sid)),
            None => "> ".into(),
        }
    }

    /// A message the owner typed to a task that works: the steer's words, under it.
    pub(crate) fn steer_said(&mut self, sid: &str) {
        if self.task_parent(sid).is_some() {
            if let Some(d) = self.detail.as_mut() {
                d.note(Tag::Dim, &format!("↪ {STEER}"));
            }
        }
    }

    /// A message for a task that has reported goes to the conversation that started it, with the shared
    /// redirect line, and the pane follows it there.
    pub(crate) fn send_to_main(&mut self, task: &str, text: &str, why: &str) -> Vec<Effect> {
        let Some(parent) = self.task_parent(task) else {
            return Vec::new();
        };
        let line = task_redirect_line(&self.board.name(task), &short(task), why);
        self.input.clear();
        self.mode = crate::app::Mode::Normal;
        let mut out = self.focus(&parent);
        if let Some(d) = self.detail.as_mut().filter(|d| d.session_id == parent) {
            d.sent(text);
            d.note(Tag::Dim, &format!("↪ {why}: sent here"));
            d.scroll = 0;
        }
        let params = json!({
            "session_id": parent, "input": format!("{line}{text}"), "author": AUTHOR, "via": "tui"
        });
        out.push(call(method::TURN_SUBMIT, params, Purpose::Submit(parent)));
        out
    }

    /// A request that failed. A `turn.submit` to a task that ended as the message left sends the message
    /// to main instead of losing it; anything else is said where the operator looks.
    pub(crate) fn request_failed(&mut self, purpose: &Purpose, e: &RpcError) -> Vec<Effect> {
        if let Purpose::SubmitTask { task, text } = purpose {
            if task_is_over(e) {
                return self.send_to_main(task, text, "the task ended first");
            }
        }
        self.failed(purpose, e);
        Vec::new()
    }

    /// `I`: the inspector link of the newest turn in the session on screen (`turn.inspect`).
    pub(crate) fn inspect_key(&mut self) -> Vec<Effect> {
        let Some(sid) = self.on_screen() else {
            return Vec::new();
        };
        let turn = self.detail.as_ref().and_then(|d| d.turn.clone());
        let Some(turn) = turn else {
            self.flash = Some((Tag::Dim, "no turn to inspect yet".to_string()));
            return Vec::new();
        };
        let params = json!({ "session_id": sid, "turn_id": turn });
        vec![call(method::TURN_INSPECT, params, Purpose::Inspect(sid))]
    }

    /// `turn.inspect`'s answer: the link, in the pane and the footer.
    pub(crate) fn inspected(&mut self, sid: &str, v: serde_json::Value) {
        let Ok(r) = serde_json::from_value::<TurnInspectResult>(v) else {
            return;
        };
        let line = match r.url.is_empty() {
            true => "the daemon has no cockpit link to give ([cockpit] public_url)".to_string(),
            false => format!("inspect {}", r.url),
        };
        self.note(sid, Tag::Dim, &line);
        self.flash = Some((Tag::Dim, line));
    }
}
