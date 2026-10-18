//! Starting, pausing and resuming a task from the chat (theseus-ypnxt.4).
//!
//! `/task new BRIEF` and the typed words `make that a task` call `task.start`
//! under this conversation, with no model turn: the owner's own words are the
//! brief, and nothing in a message is classified. `/pause [ID]` and
//! `/resume [ID]` call `task.pause` and `task.resume`; with no id they act on
//! the task shown, else on the only task they could change, else they list
//! the candidates and send nothing.

use serde_json::{json, Value};
use theseus_client::render::task_tree;
use theseus_protocol::task_pause::TaskPauseResult;
use theseus_protocol::task_start::TaskStartResult;
use theseus_protocol::work::{WorkKind, WorkView};
use theseus_protocol::{method, RpcError};

use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::Sty;

/// The words that make the owner's last message a task.
const MAKE_THAT: &str = "make that a task";

impl Chat {
    /// `/task [ID]` and `/task new [BRIEF]`.
    pub(super) fn task_command(&mut self, args: &str) -> Vec<Effect> {
        match args.split_once(char::is_whitespace).unwrap_or((args, "")) {
            ("new", brief) => self.new_task(brief.trim()),
            _ => self.task_cmd(args, false),
        }
    }

    /// Whether a whole message is the words that start a task from the last.
    pub(super) fn is_make_that(text: &str) -> bool {
        text.trim().eq_ignore_ascii_case(MAKE_THAT)
    }

    /// `task.start` for `brief`, or for the owner's last message when none is
    /// given: under this conversation, with no profile and no budget, so the
    /// daemon's defaults apply.
    pub(super) fn new_task(&mut self, brief: &str) -> Vec<Effect> {
        let brief = if brief.is_empty() {
            match self.last_said.clone() {
                Some(last) => last,
                None => {
                    self.flash("no message of yours here yet · /task new BRIEF");
                    return Vec::new();
                }
            }
        } else {
            brief.to_string()
        };
        let Some(home) = self.main_session() else {
            self.flash("the conversation is not open yet");
            return Vec::new();
        };
        let title = brief.lines().next().unwrap_or("").trim().to_string();
        vec![call(
            method::TASK_START,
            crate::new_task::start_params(&brief, Some(&home), AUTHOR),
            Purpose::TaskStart { title },
        )]
    }

    /// `/pause [ID]` (`on`) and `/resume [ID]`.
    pub(super) fn hold_cmd(&mut self, args: &str, on: bool) -> Vec<Effect> {
        let word = if on { "pause" } else { "resume" };
        let given = args.trim();
        let (task, short) = if !given.is_empty() {
            if given.chars().count() < 4 {
                self.flash(format!(
                    "/{word} ID · at least four characters of a task's id"
                ));
                return Vec::new();
            }
            (given.to_string(), given.to_string())
        } else if let Some(v) = &self.view {
            (v.task.session.clone(), v.task.short())
        } else {
            let mut cands: Vec<&WorkView> = self
                .board
                .views
                .values()
                .filter(|v| v.kind == WorkKind::Task && v.session_id.is_some())
                .filter(|v| !v.state.ended() && v.paused != on)
                .collect();
            cands.sort_by_key(|v| (v.started_at_ms, v.id.clone()));
            match cands.as_slice() {
                [one] => {
                    let sid = one.session_id.clone().unwrap_or_default();
                    (sid.clone(), task_tree::short_id(&sid))
                }
                [] => {
                    let what = if on { "running" } else { "paused" };
                    self.flash(format!("no {what} task to {word} · /{word} ID"));
                    return Vec::new();
                }
                many => {
                    let rows: Vec<String> = many
                        .iter()
                        .map(|v| {
                            let sid = v.session_id.as_deref().unwrap_or(&v.id);
                            format!("  {} {}", task_tree::short_id(sid), task_tree::name(v))
                        })
                        .collect();
                    self.say_plain(Sty::Dim, &format!("/{word} which one? /{word} ID:"));
                    for r in rows {
                        self.say_plain(Sty::Dim, &r);
                    }
                    return Vec::new();
                }
            }
        };
        let m = if on {
            method::TASK_PAUSE
        } else {
            method::TASK_RESUME
        };
        vec![call(
            m,
            json!({ "task": task, "author": AUTHOR }),
            Purpose::Hold { on, short },
        )]
    }

    /// The daemon's answers to a start and to a pause or a resume.
    pub(super) fn held_answer(&mut self, purpose: Purpose, v: Value) {
        match purpose {
            Purpose::TaskStart { title } => {
                if let Ok(r) = serde_json::from_value::<TaskStartResult>(v) {
                    let title = if title.is_empty() { "a task" } else { &title };
                    self.say_words(
                        Sty::Cyan,
                        &format!(
                            "◐ started {title} ({s}) · /task {s} to look, /join {s} to talk to it",
                            s = r.short
                        ),
                    );
                }
            }
            Purpose::Hold { on, short } => {
                let changed = serde_json::from_value::<TaskPauseResult>(v).is_ok_and(|r| r.changed);
                let line = match (on, changed) {
                    (true, true) => format!("Pausing the task {short}"),
                    (false, true) => format!("Resuming the task {short}"),
                    (true, false) => format!("The task {short} is paused already"),
                    (false, false) => format!("The task {short} is running already"),
                };
                self.say_plain(Sty::Dim, &format!("  {line}"));
            }
            _ => {}
        }
    }

    /// A start, a pause or a resume the daemon refused: its words.
    pub(super) fn held_refused(&mut self, e: &RpcError) {
        self.say_words(Sty::Red, &format!("✗ {}", e.message));
    }
}
