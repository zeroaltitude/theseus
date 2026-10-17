//! Inspect, join and return (theseus-rrkb7): a task is its own session, and
//! the chat can look at it, talk to it, and come back to the conversation
//! that started it. All of it is the chat's view and none of it is stored.
//!
//! While a task is shown, `Chat::session` is the task's, so its history, its
//! turns and its tool calls come through the code the conversation uses; the
//! conversation's own state waits in the view's [`Stash`], and its events,
//! which the chat still hears, are only counted (`Away`). *Inspect* is
//! read-only (the keys `j`, `i`, `f` act on an empty line, since nothing can
//! be typed). *Join* sends the line to the task's session (`turn.submit`, at
//! once, never queued: the daemon steers it into the turn that runs). A line
//! for a task that has ended goes to the conversation, with the one redirect
//! line every surface says (`task_redirect_line`). Return (`Esc` on an empty
//! line, `/back`) restores the stash and reads the conversation again, then
//! says what happened while away. The shared viewer's `focused` is the task's
//! session while it is shown, so the policy does not ring for what is on screen.

use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use theseus_client::render::task_tree;
use theseus_protocol::modes::{PlanItem, SessionMode};
use theseus_protocol::work::{joinable, task_redirect_line, WorkKind, WorkState, WorkView};
use theseus_protocol::{
    method, ConfirmRequest, Event, RpcError, SessionInfo, SessionKind, SessionListResult,
};

use super::dock::DockCtx;
use super::editor::Taken;
use super::live::Live;
use super::reopen::REPLAY_NODES;
use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::{one, Line, Sty};

/// The task as the view knows it.
#[derive(Debug, Clone)]
pub(super) struct TaskRef {
    /// The task's execution (`exe_…`): what `task.retry` takes.
    pub id: String,
    pub session: String,
    pub title: String,
}

impl TaskRef {
    pub fn short(&self) -> String {
        task_tree::short_id(&self.session)
    }

    pub fn of(v: &WorkView) -> Option<Self> {
        Some(Self {
            id: v.id.clone(),
            session: v.session_id.clone()?,
            title: task_tree::name(v),
        })
    }
}

/// The conversation's state, put aside while a task is shown.
#[derive(Default)]
pub(super) struct Stash {
    session: Option<String>,
    execution: Option<String>,
    profile: Option<String>,
    model: Option<String>,
    cost: f64,
    mode: SessionMode,
    plan: Vec<PlanItem>,
    live: Live,
    question: Option<ConfirmRequest>,
    queued: Vec<Taken>,
    mine: HashSet<String>,
    answered_text: HashSet<String>,
    proposed: HashMap<String, (String, Value)>,
    earlier: Option<u64>,
    last_inspect: Option<String>,
}

/// What the conversation did while the task was shown.
#[derive(Default)]
struct Away {
    replies: u32,
    asked: u32,
}

pub(super) struct TaskView {
    pub task: TaskRef,
    pub join: bool,
    /// The task's last turn, for the inspector's link.
    pub last_turn: Option<String>,
    /// The end was said once.
    told_end: bool,
    /// `/stop` was said once: the second cancels.
    stop_armed: bool,
    main: Stash,
    away: Away,
}

/// What the replay of the conversation we came back to says after it.
pub(super) struct Back {
    line: String,
    /// Lines typed for an ended task, sent to the conversation.
    sent: Vec<String>,
}

const NOTE_SENT: &str = "  ↪ the task is over: sent to the conversation that started it";

impl Chat {
    // ------------------------------------------------------ the state

    /// The conversation's session: the chat's own, or the one a view put aside.
    pub(super) fn main_session(&self) -> Option<String> {
        match &self.view {
            Some(v) => v.main.session.clone(),
            None => self.session.clone(),
        }
    }

    /// The board's view of the task that is shown.
    pub(super) fn viewed_work(&self) -> Option<&WorkView> {
        let session = &self.view.as_ref()?.task.session;
        self.board
            .views
            .values()
            .find(|w| w.session_id.as_deref() == Some(session))
    }

    fn viewed_ended(&self) -> bool {
        self.viewed_work().is_some_and(|w| !joinable(w))
    }

    fn stash_main(&mut self) -> Stash {
        Stash {
            session: self.session.take(),
            execution: self.execution.take(),
            profile: self.profile.take(),
            model: self.model.take(),
            cost: std::mem::take(&mut self.cost),
            mode: std::mem::take(&mut self.mode),
            plan: std::mem::take(&mut self.plan),
            live: std::mem::take(&mut self.live),
            question: self.question.take(),
            queued: std::mem::take(&mut self.queued),
            mine: std::mem::take(&mut self.mine),
            answered_text: std::mem::take(&mut self.answered_text),
            proposed: std::mem::take(&mut self.proposed),
            earlier: self.earlier.take(),
            last_inspect: self.last_inspect.take(),
        }
    }

    fn restore(&mut self, s: Stash) {
        self.session = s.session;
        self.execution = s.execution;
        self.profile = s.profile;
        self.model = s.model;
        self.cost = s.cost;
        self.mode = s.mode;
        self.plan = s.plan;
        self.live = s.live;
        self.question = s.question;
        self.queued = s.queued;
        self.mine = s.mine;
        self.answered_text = s.answered_text;
        self.proposed = s.proposed;
        self.earlier = s.earlier;
        self.last_inspect = s.last_inspect;
    }

    /// A task by the end of its id (four characters or more), among the
    /// board's tasks.
    pub(super) fn resolve_task(&self, given: &str) -> Result<TaskRef, String> {
        let g = theseus_client::resolve::trimmed(given);
        if g.chars().count() < 4 {
            return Err(format!(
                "`{given}` is too short to name a task: give at least four characters of its id"
            ));
        }
        let hits: Vec<&WorkView> = self
            .board
            .views
            .values()
            .filter(|v| v.kind == WorkKind::Task && v.session_id.is_some())
            .filter(|v| {
                v.id.ends_with(g) || v.session_id.as_deref().is_some_and(|s| s.ends_with(g))
            })
            .collect();
        match hits.as_slice() {
            [one] => TaskRef::of(one).ok_or_else(|| format!("`{given}` has no session")),
            [] => Err(format!("no task `{given}` · /tasks lists them")),
            many => Err(format!("`{given}` names {} tasks", many.len())),
        }
    }

    // ------------------------------------------------------ opening

    /// Show a task, read-only or joined: its history is read and its session
    /// watched (the conversation's watch goes on).
    pub(super) fn open_task(&mut self, t: TaskRef, join: bool) -> Vec<Effect> {
        let was = self.view.take().map(|v| v.main);
        let stash = self.stash_main();
        let main = was.unwrap_or(stash);
        let sid = t.session.clone();
        self.session = Some(sid.clone());
        self.ask = Default::default();
        self.released = false;
        self.task_picker = None;
        self.gap();
        self.view = Some(TaskView {
            task: t,
            join,
            last_turn: None,
            told_end: false,
            stop_armed: false,
            main,
            away: Away::default(),
        });
        self.ask_sync();
        vec![
            call(
                method::SESSION_HISTORY,
                json!({ "session_id": sid, "n": REPLAY_NODES }),
                Purpose::History(sid.clone()),
            ),
            call(
                method::SESSION_WATCH,
                json!({ "session_id": sid }),
                Purpose::Watch,
            ),
        ]
    }

    /// `chat --task ID`: the sessions are listed, the task and its parent
    /// found, and the task shown with the conversation behind it.
    pub(super) fn open_cold_task(&mut self, given: &str, join: bool, v: Value) -> Vec<Effect> {
        let sessions = serde_json::from_value::<SessionListResult>(v)
            .map(|l| l.sessions)
            .unwrap_or_default();
        let g = theseus_client::resolve::trimmed(given);
        let hits: Vec<&SessionInfo> = sessions
            .iter()
            .filter(|s| s.kind == SessionKind::Task)
            .filter(|s| {
                s.session_id == g
                    || (g.chars().count() >= 4
                        && (s.session_id.ends_with(g)
                            || s.execution_id.as_deref().is_some_and(|e| e.ends_with(g))))
            })
            .collect();
        let task = match hits.as_slice() {
            [one] => (*one).clone(),
            [] => return self.fatal(&format!("no task `{given}` · theseus tasks lists them")),
            many => {
                let ids: Vec<&str> = many.iter().map(|s| s.session_id.as_str()).collect();
                return self.fatal(&format!(
                    "`{given}` names {} tasks: {}",
                    many.len(),
                    ids.join(", ")
                ));
            }
        };
        let parent = task
            .parent_session_id
            .as_deref()
            .and_then(|p| sessions.iter().find(|s| s.session_id == p));
        let Some(parent) = parent.cloned() else {
            return self.fatal(&format!(
                "task `{given}` has no conversation to go back to · theseus chat opens one"
            ));
        };
        self.profile = parent.profile.clone();
        self.model = self.model.take().or(parent.model.clone());
        self.cost = parent.cost_usd;
        self.execution = parent.execution_id.clone();
        self.banner.last = Some(None);
        let mut out = self.opened(&parent.session_id);
        self.banner_ready();
        out.extend(
            self.open_task(
                TaskRef {
                    id: task.execution_id.clone().unwrap_or_default(),
                    title: task
                        .title
                        .clone()
                        .or(task.label.clone())
                        .unwrap_or_default(),
                    session: task.session_id,
                },
                join,
            ),
        );
        out
    }

    /// The header line the view's history opens with.
    pub(super) fn task_head(&self, sid: &str, turns: u64, title: &str) -> Option<String> {
        let v = self.view.as_ref().filter(|v| v.task.session == sid)?;
        let title = if title.is_empty() {
            &v.task.title
        } else {
            title
        };
        Some(format!(
            "task {} · {turns} turn{} · {title}",
            v.task.short(),
            if turns == 1 { "" } else { "s" }
        ))
    }

    /// The daemon's answers to what the view asked.
    pub(super) fn answered_view(&mut self, purpose: Purpose, v: Value) -> Vec<Effect> {
        match purpose {
            Purpose::ListTask { given, join } => return self.open_cold_task(&given, join, v),
            Purpose::TaskSubmit { .. } => {
                if let Ok(r) = serde_json::from_value::<theseus_protocol::TurnSubmitResult>(v) {
                    self.turn_ended(&r);
                }
            }
            Purpose::TaskInspect => self.inspected(&v),
            Purpose::TaskAnswer { who, option } => self.task_answered(&who, &option),
            Purpose::Cancel(id) => self.say_plain(Sty::Dim, &format!("  cancelled {id}")),
            Purpose::FollowUp => {
                let id = v.get("task_id").and_then(Value::as_str).unwrap_or("");
                let short = task_tree::short_id(id);
                self.say_plain(Sty::Dim, &format!("  follow-up task {short} started"));
            }
            _ => {}
        }
        Vec::new()
    }

    /// The task's last turn, from its replay.
    pub(super) fn note_last_turn(&mut self, sid: &str, turn: Option<String>) {
        if let Some(v) = self.view.as_mut().filter(|v| v.task.session == sid) {
            v.last_turn = turn.or(v.last_turn.take());
        }
    }

    // ------------------------------------------------------ returning

    /// Back to the conversation: its state restored, its history read again,
    /// and one line saying what happened while we were away.
    pub(super) fn return_main(&mut self) -> Vec<Effect> {
        let Some(v) = self.view.take() else {
            self.flash("this is the conversation already");
            return Vec::new();
        };
        let TaskView {
            task, main, away, ..
        } = v;
        let _ = self.stash_main();
        let line = self.back_line(&task, &away);
        self.restore(main);
        self.ask = Default::default();
        self.released = false;
        self.ask_sync();
        self.back = Some(Back {
            line,
            sent: Vec::new(),
        });
        let Some(sid) = self.session.clone() else {
            return Vec::new();
        };
        vec![call(
            method::SESSION_HISTORY,
            json!({ "session_id": sid, "n": REPLAY_NODES }),
            Purpose::History(sid),
        )]
    }

    /// `── back in main · a1b2c3 still running · main: 1 reply while you were away ──`
    fn back_line(&self, task: &TaskRef, away: &Away) -> String {
        let state = match self
            .board
            .views
            .values()
            .find(|w| w.session_id.as_deref() == Some(&task.session))
        {
            Some(w) if joinable(w) => "still running".to_string(),
            Some(w) => match w.state {
                WorkState::Done => "reported".to_string(),
                s => s.as_str().to_string(),
            },
            None => "left as it was".to_string(),
        };
        let mut main = match away.replies {
            0 => "nothing new in main".to_string(),
            1 => "main: 1 reply while you were away".to_string(),
            n => format!("main: {n} replies while you were away"),
        };
        if away.asked > 0 {
            main.push_str(&format!(" · {} waiting on you", away.asked));
        }
        format!("── back in main · {} {state} · {main} ──", task.short())
    }

    /// The replay of the conversation is on the screen: say the line, and
    /// echo what was sent for an ended task.
    pub(super) fn say_back(&mut self) {
        let Some(b) = self.back.take() else { return };
        self.say_plain(Sty::Cyan, &b.line);
        for text in b.sent {
            self.echo(&text);
            self.say_plain(Sty::Dim, NOTE_SENT);
        }
    }

    // ------------------------------------------------------ keys and commands

    /// A key with the line empty, in a task only looked at: `j` joins, `i`
    /// the inspector's link, `f` a follow-up. Joined, the line is for typing.
    pub(super) fn view_key(&mut self, c: char) -> Option<Vec<Effect>> {
        let v = self.view.as_ref().filter(|v| !v.join)?;
        let task = v.task.clone();
        Some(match c.to_ascii_lowercase() {
            'j' => self.join_task(task),
            'i' => self.inspect_cmd(""),
            'f' => self.follow_up(),
            _ => return None,
        })
    }

    fn join_task(&mut self, task: TaskRef) -> Vec<Effect> {
        let can = self
            .board
            .views
            .values()
            .find(|w| w.session_id.as_deref() == Some(&task.session))
            .is_none_or(joinable);
        if !can {
            self.flash(format!(
                "{} has ended · what you type goes to main · esc goes back",
                task.short()
            ));
            return Vec::new();
        }
        if let Some(v) = self.view.as_mut() {
            v.join = true;
        }
        self.say_plain(
            Sty::Dim,
            &format!("  joined {} · esc goes back to main", task.short()),
        );
        Vec::new()
    }

    /// `/task [ID]` and `/join ID`.
    pub(super) fn task_cmd(&mut self, args: &str, join: bool) -> Vec<Effect> {
        if args.trim().is_empty() {
            if join {
                self.flash("/join ID · at least four characters of a task's id");
                return Vec::new();
            }
            return self.open_task_picker();
        }
        match self.resolve_task(args) {
            Ok(t) if join => {
                let can = self
                    .board
                    .views
                    .values()
                    .any(|w| w.session_id.as_deref() == Some(&t.session) && joinable(w));
                if !can {
                    self.flash(format!(
                        "{} has ended · /task {} shows it",
                        t.short(),
                        t.short()
                    ));
                    return Vec::new();
                }
                self.open_task(t, true)
            }
            Ok(t) => self.open_task(t, false),
            Err(e) => {
                self.flash(e);
                Vec::new()
            }
        }
    }

    /// `/inspect [ID]`, and `i`: with a task's id, that task read-only;
    /// else the last turn's link, read from the daemon for a replayed turn.
    pub(super) fn inspect_cmd(&mut self, args: &str) -> Vec<Effect> {
        if !args.trim().is_empty() {
            return self.task_cmd(args, false);
        }
        if let Some(u) = &self.last_inspect {
            let line = format!("  inspect {u}");
            self.say_plain(Sty::Dim, &line);
            return Vec::new();
        }
        let asked = self
            .view
            .as_ref()
            .and_then(|v| v.last_turn.clone().map(|t| (v.task.session.clone(), t)));
        match asked {
            Some((session_id, turn_id)) => vec![call(
                method::TURN_INSPECT,
                json!({ "session_id": session_id, "turn_id": turn_id }),
                Purpose::TaskInspect,
            )],
            None => {
                self.say_plain(Sty::Dim, "  no turn has ended here yet");
                Vec::new()
            }
        }
    }

    /// The inspector's link, read.
    pub(super) fn inspected(&mut self, v: &Value) {
        if let Some(u) = v.get("url").and_then(Value::as_str) {
            self.say_plain(Sty::Dim, &format!("  inspect {u}"));
            self.last_inspect = Some(u.to_string());
        }
    }

    /// `/follow`, and `f` on an ended task: one `task.retry`, a linked task
    /// that tries again. Never automatic; the daemon refuses what did not fail.
    pub(super) fn follow_up(&mut self) -> Vec<Effect> {
        let Some(id) = self.view.as_ref().map(|v| v.task.id.clone()) else {
            self.flash("/follow works in a task's view");
            return Vec::new();
        };
        if !self.viewed_ended() {
            self.flash("the task has not ended");
            return Vec::new();
        }
        if let Err(e) =
            theseus_client::client::refuse_in_a_job(method::TASK_RETRY, self.job_session.as_deref())
        {
            self.say_words(Sty::Red, &format!("✗ {e}"));
            return Vec::new();
        }
        vec![call(
            method::TASK_RETRY,
            json!({ "execution_id": id, "note": "a follow-up the owner asked for" }),
            Purpose::FollowUp,
        )]
    }

    /// Esc: with the line empty, back to the conversation. Nothing stops.
    pub(super) fn view_escape(&mut self) -> Vec<Effect> {
        if self.editor.is_empty() {
            return self.return_main();
        }
        Vec::new()
    }

    /// `/stop` in a task's view cancels the task, after asking once.
    pub(super) fn view_stop(&mut self, args: &str) -> Option<Vec<Effect>> {
        let short = self.view.as_ref()?.task.short();
        if !args.trim().is_empty() {
            self.flash("/stop cancels the task shown · /back goes to main");
            return Some(Vec::new());
        }
        if self.viewed_ended() {
            self.flash(format!("{short} has ended"));
            return Some(Vec::new());
        }
        let armed = self
            .view
            .as_mut()
            .is_some_and(|v| std::mem::replace(&mut v.stop_armed, true));
        if !armed {
            self.say_plain(
                Sty::Yellow,
                &format!("  cancel task {short}? /stop again confirms · anything else keeps it"),
            );
            return Some(Vec::new());
        }
        if let Some(v) = self.view.as_mut() {
            v.stop_armed = false;
        }
        Some(self.cancel_task(&short))
    }

    /// A line was entered: anything but `/stop` takes back its question.
    pub(super) fn view_line(&mut self, line: &str) {
        if let Some(v) = self.view.as_mut().filter(|_| line != "/stop") {
            v.stop_armed = false;
        }
    }

    // ------------------------------------------------------ sending

    /// A line that is not a command, while a task is shown. None: no task.
    pub(super) fn view_submit(&mut self, taken: &Taken) -> Option<Vec<Effect>> {
        let (join, task) = self.view.as_ref().map(|v| (v.join, v.task.clone()))?;
        let mut effects = Vec::new();
        if !join {
            self.editor.insert_str(&taken.text);
            self.flash(format!(
                "{} is read-only here · j joins it · esc goes back to main",
                task.short()
            ));
            return Some(effects);
        }
        effects.push(Effect::SaveHistory(self.editor.history().to_vec()));
        if self.viewed_ended() {
            effects.extend(self.redirect(&taken.text, &taken.shown));
            return Some(effects);
        }
        self.echo(&taken.shown);
        self.say_plain(Sty::Dim, "  ↪ the task reads this at its next step");
        effects.push(call(
            method::TURN_SUBMIT,
            json!({ "session_id": task.session, "input": taken.text, "author": AUTHOR }),
            Purpose::TaskSubmit {
                text: taken.text.clone(),
            },
        ));
        Some(effects)
    }

    /// The task is over: back to the conversation, and the line goes there
    /// with the redirect line every surface says.
    fn redirect(&mut self, text: &str, shown: &str) -> Vec<Effect> {
        let Some(task) = self.view.as_ref().map(|v| v.task.clone()) else {
            return Vec::new();
        };
        let said = format!(
            "{}{text}",
            task_redirect_line(&task.title, &task.short(), "the task is over")
        );
        let mut effects = self.return_main();
        if let Some(b) = self.back.as_mut() {
            b.sent.push(shown.to_string());
        }
        if let Some(sid) = self.session.clone() {
            effects.push(self.submit_call(&sid, said));
        }
        effects
    }

    /// The task's session refused a line: ended after the view last heard
    /// (`execution_complete`), so the line goes to the conversation; any
    /// other refusal is said.
    pub(super) fn task_submit_refused(&mut self, text: &str, e: &RpcError) -> Vec<Effect> {
        let class = e.data.get("class").and_then(Value::as_str);
        let ended = e.code == theseus_protocol::error_code::NOT_FOUND
            || class.is_some_and(|c| c.starts_with("execution_"));
        if ended && self.view.is_some() {
            return self.redirect(text, text);
        }
        self.say_words(Sty::Red, &format!("✗ not sent: {}", e.message));
        Vec::new()
    }

    // ------------------------------------------------------ the conversation's events

    /// The conversation's own events while a task is shown are counted, for
    /// the line on return.
    pub(super) fn note_away(&mut self, e: &Event) {
        let Some(v) = self.view.as_mut() else { return };
        let main = v.main.session.as_deref();
        match e {
            Event::TurnEnded(r)
                if Some(r.session_id.as_str()) == main && !r.output.trim().is_empty() =>
            {
                v.away.replies += 1;
            }
            Event::ConfirmRequested(c) if Some(c.session_id.as_str()) == main => {
                v.away.asked += 1;
            }
            _ => {}
        }
    }

    /// The board moved: a task that ended while shown says so once, and the
    /// words under the header change (`viewed_ended`).
    pub(super) fn viewed_changed(&mut self) {
        if !self.viewed_ended() || self.view.as_ref().is_none_or(|v| v.told_end) {
            return;
        }
        let word = self.viewed_work().map_or("reported", |w| match w.state {
            WorkState::Done => "reported",
            WorkState::Failed => "failed",
            WorkState::Cancelled => "cancelled",
            _ => "ended",
        });
        let Some(v) = self.view.as_mut() else { return };
        v.told_end = true;
        let line = format!(
            "🏁 {} {word} · what you type now goes to main · esc goes back",
            v.task.short()
        );
        self.say_plain(Sty::Dim, &line);
    }

    // ------------------------------------------------------ the dock

    /// When the header's clock next turns over (the minute), while a task
    /// that works is shown.
    pub(super) fn view_repaint_at(&self) -> Option<u64> {
        let w = self.viewed_work().filter(|w| joinable(w))?;
        let run = self.now_ms.saturating_sub(w.started_at_ms);
        Some(w.started_at_ms + (run / 60_000 + 1) * 60_000)
    }

    /// The prompt: the task's short id while it is joined and works, `main`
    /// once it has ended (the line goes there), `›` otherwise.
    pub(super) fn prompt(&self) -> String {
        match &self.view {
            None => "› ".to_string(),
            Some(v) if v.join && self.viewed_ended() => "main › ".to_string(),
            Some(v) if v.join => format!("{} › ", v.task.short()),
            Some(v) => format!("{} (read-only) › ", v.task.short()),
        }
    }

    /// The columns the input has, the prompt taken off.
    pub(super) fn input_width(&self) -> usize {
        let extra = self.prompt().chars().count().saturating_sub(2);
        usize::from(self.width).saturating_sub(extra)
    }
}

/// `── ⚙ task a1b2c3 · Sound the north channel · running 4m · $0.31 ── j join …`
pub fn header_rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(v) = chat.view.as_ref() else {
        return Vec::new();
    };
    let ended = chat.viewed_ended();
    let mut facts = Vec::new();
    if let Some(w) = chat.viewed_work() {
        let state = match w.state {
            WorkState::Done => "reported",
            WorkState::NeedsYou => "needs you",
            s => s.as_str(),
        };
        let end = w.ended_at_ms.unwrap_or(ctx.now_ms);
        facts.push(format!(
            "{state} {}",
            task_tree::coarse(end.saturating_sub(w.started_at_ms))
        ));
        if let Some(c) = &w.cost {
            facts.push(theseus_protocol::usd(c.spent_usd));
        }
    }
    let keys = match (v.join, ended) {
        (false, false) => "j join · i inspector · Esc back to main",
        (false, true) => "f follow up · i inspector · Esc back to main",
        (true, false) => "joined · /inspect · Esc back to main",
        (true, true) => "type to tell main · /follow · Esc back to main",
    };
    let tail = if facts.is_empty() {
        String::new()
    } else {
        format!(" · {}", facts.join(" · "))
    };
    let fixed = format!("── ⚙ task {} · ", v.task.short()).chars().count()
        + tail.chars().count()
        + " ── ".chars().count()
        + keys.chars().count();
    let room = ctx.width.saturating_sub(fixed).max(8);
    let title = task_tree::cut(&v.task.title, room);
    vec![one(
        Sty::Cyan,
        format!("── ⚙ task {} · {title}{tail} ── {keys}", v.task.short()),
    )]
}
