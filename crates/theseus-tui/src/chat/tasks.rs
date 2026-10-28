//! The chat's tasks (theseus-mnd9g): the work board of this conversation,
//! kept from one `work.watch` (`interactive`, so the daemon knows a person is
//! looking), the lines it prints as tasks start, ask, report and fail, the
//! tasks' questions, and the commands `/tasks` and `/cancel`.
//!
//! Nothing here runs a timer. The board changes when the daemon pushes, and
//! the footer's clocks turn over on the minute, so the chat asks the runner
//! for a deadline only while a row has one (`repaint_at`).
//!
//! A task's question is answered in the dock like the conversation's own
//! (`ask.rs` owns the one question model and `keys::ASK_KEYS` the one key
//! table): this module keeps the board, lists the tasks' questions as
//! [`TaskQ`] and sends their answers through `work.answer`.

use std::collections::{HashMap, HashSet};

use serde_json::json;
use theseus_client::render::task_tree::{self, Event as TaskEvent};
use theseus_client::render::Tag;
use theseus_protocol::notices::{self, Notice, Urgency, Why};
use theseus_protocol::work::{AnswerOption, QuestionKind, WorkKind, WorkView};
use theseus_protocol::{method, WorkChanged, WorkWatchResult};

use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::{one, span, Line, Sty};

/// The footer shows this many tasks, then `+N more · ctrl-t`.
pub const FOOTER_ROWS: usize = 5;

/// A question a task asks, as the dock shows it.
#[derive(Debug, Clone)]
pub(super) struct TaskQ {
    pub id: String,
    /// The task that asks.
    pub who: String,
    pub words: String,
    pub kind: QuestionKind,
    pub options: Vec<AnswerOption>,
    pub expires_at_ms: Option<u64>,
    /// The call in full, for `v`.
    pub detail: Vec<String>,
}

/// The state of the board in the chat.
#[derive(Default)]
pub(super) struct Board {
    pub views: HashMap<String, WorkView>,
    /// A snapshot came: the board is the daemon's and the footer is drawn.
    pub live: bool,
    /// Questions already announced in the transcript.
    pub told: HashSet<String>,
    /// When the footer next changes by itself.
    pub repaint_at: Option<u64>,
    /// The last board position this chat applied: the digest after a lost
    /// connection starts after it.
    pub position: u64,
}

impl Board {
    fn list(&self) -> Vec<&WorkView> {
        self.views.values().collect()
    }
}

/// The tag of a task line as the chat styles it.
pub(super) fn sty_of(tag: Tag) -> Sty {
    use theseus_protocol::Level;
    match tag {
        Tag::Ask | Tag::Warn | Tag::Level(Level::NeedsYou) => Sty::Yellow,
        Tag::Ok => Sty::Green,
        Tag::Bad => Sty::Red,
        Tag::Level(Level::Working) => Sty::Cyan,
        Tag::Dim | Tag::Level(Level::Idle | Level::Ready) => Sty::Dim,
        _ => Sty::Plain,
    }
}

impl Chat {
    // ------------------------------------------------------ the watch

    /// `work.watch` for this conversation, as a screen a person looks at.
    pub(super) fn watch_work(sid: &str) -> Effect {
        Self::watch_with(sid, &[])
    }

    /// `session.watch` of one session: its events reach this connection.
    pub(super) fn watch_session(sid: &str) -> Effect {
        call(
            method::SESSION_WATCH,
            json!({ "session_id": sid }),
            Purpose::Watch,
        )
    }

    /// `session.unwatch` of one session: its events stop.
    pub(super) fn unwatch_session(sid: &str) -> Effect {
        call(
            method::SESSION_UNWATCH,
            json!({ "session_id": sid }),
            Purpose::Watch,
        )
    }

    /// The same watch with the sessions of turns the router placed in
    /// threads (theseus-umb8a). A connection holds one `work.watch`, and a
    /// new one replaces the roots of the last, so this names them all.
    pub(super) fn watch_with(sid: &str, also: &[String]) -> Effect {
        let roots: Vec<&str> = std::iter::once(sid)
            .chain(also.iter().map(String::as_str))
            .collect();
        call(
            method::WORK_WATCH,
            json!({ "roots": roots, "limit": 0, "interactive": true }),
            Purpose::WorkWatch,
        )
    }

    /// The snapshot: the board as it stands. No line is printed for what
    /// already runs; a question that already waits is told once.
    pub(super) fn work_snapshot(&mut self, r: WorkWatchResult) -> Vec<Effect> {
        self.board.live = true;
        self.board.position = r.position;
        self.board.views = r.views.into_iter().map(|v| (v.id.clone(), v)).collect();
        let waiting: Vec<TaskQ> = self.task_questions();
        for p in waiting {
            self.tell_question(&p);
        }
        self.board_changed();
        self.viewed_changed();
        // A first start has no look to tell from: this board is it.
        let first = self
            .cfg
            .seen_path
            .as_deref()
            .is_some_and(|p| theseus_client::seen::looked(p).position == 0);
        if first {
            self.look(r.position)
        } else {
            Vec::new()
        }
    }

    /// A frame's changes, by (position, time): the daemon's sample of a running job's last line
    /// keeps its frame's position and takes a later time (theseus-hwv3t).
    pub(super) fn work_changed(&mut self, c: WorkChanged) {
        self.board.position = self.board.position.max(c.position);
        for v in c.views {
            let prev = self.board.views.get(&v.id);
            if prev.is_some_and(|p| (p.position, p.at_ms) >= (v.position, v.at_ms)) {
                continue;
            }
            // A question is told by its event's line, once: the snapshot's
            // `tell_question` skips what an event already said.
            let events: Vec<(Line, Option<String>)> = task_tree::events(prev, &v)
                .iter()
                .filter(|e| task_event_is_ours(e))
                .map(|e| {
                    let asked = match e {
                        TaskEvent::Asked(_, q) => Some(q.id.clone()),
                        _ => None,
                    };
                    (words(e), asked)
                })
                .collect();
            for (line, asked) in events {
                self.say(&line);
                self.board.told.extend(asked);
            }
            self.board.views.insert(v.id.clone(), v);
        }
        for id in c.gone {
            self.board.views.remove(&id);
        }
        self.board_changed();
        self.viewed_changed();
    }

    /// A notice from the daemon's policy: a ping rings once. The words of a
    /// task's own events were printed from its views; the rest are printed
    /// here, as the notice says them.
    pub(super) fn work_notice(&mut self, n: &Notice) -> Vec<Effect> {
        let delivery = notices::deliver(n, &self.viewer(n));
        if delivery == notices::Delivery::Nothing {
            return Vec::new();
        }
        if !matches!(
            n.why,
            Why::Asked | Why::Failed | Why::Blocked | Why::Reported | Why::Answered | Why::Finished
        ) {
            let sty = if n.urgency == Urgency::Interrupt {
                Sty::Yellow
            } else {
                Sty::Dim
            };
            self.say_words(sty, &n.line);
        }
        match delivery {
            notices::Delivery::Ping { .. } => vec![Effect::Bell],
            _ => Vec::new(),
        }
    }

    /// A task of this session changed (the session's own events, for a
    /// daemon without the board): the status line's count follows it, until
    /// the board is live and counts for itself.
    pub(super) fn task_changed(&mut self, t: &theseus_protocol::tasks::TaskChanged) {
        use theseus_protocol::tasks::TaskState::{Abandoned, Done, Failed, Proposed};
        let open = !matches!(t.task.state, Done | Failed | Abandoned | Proposed);
        self.task_open.insert(t.task.id.clone(), open);
        if !self.board.live {
            self.tasks = self.task_open.values().filter(|o| **o).count() as u32;
        }
    }

    /// The board moved: the status line's count, the footer's next clock.
    fn board_changed(&mut self) {
        self.ask_sync();
        let c = self.footer_counts();
        self.tasks = c.working + c.needs_you;
        self.board_refresh();
    }

    fn footer_counts(&self) -> task_tree::Counts {
        let views: Vec<WorkView> = self.board.views.values().cloned().collect();
        task_tree::footer(&views, self.now_ms, usize::from(self.width), 0).counts
    }

    /// Where the footer's next clock turns over, if a row has one.
    pub(super) fn board_refresh(&mut self) {
        let views: Vec<WorkView> = self.board.views.values().cloned().collect();
        let f = task_tree::footer(&views, self.now_ms, usize::from(self.width), FOOTER_ROWS);
        let countdown = self
            .asked_expiry()
            .filter(|e| *e > self.now_ms)
            .map(|_| (self.now_ms / 1000 + 1) * 1000);
        self.board.repaint_at = [f.next_ms, countdown].into_iter().flatten().min();
    }

    /// The clock reached the footer's deadline: it changes, and finds the next.
    pub(super) fn board_tick(&mut self) -> bool {
        match self.board.repaint_at {
            Some(at) if self.now_ms >= at => {
                self.board_refresh();
                true
            }
            _ => false,
        }
    }

    // ------------------------------------------------------ questions

    pub(super) fn task_questions(&self) -> Vec<TaskQ> {
        let views = self.board.list();
        let titles: HashMap<&str, String> = views
            .iter()
            .flat_map(|v| {
                let name = task_tree::name(v);
                let mut keys = vec![(v.id.as_str(), name.clone())];
                if let Some(s) = v.session_id.as_deref() {
                    keys.push((s, name));
                }
                keys
            })
            .collect();
        let mut seen = HashSet::new();
        let mut out: Vec<(u64, TaskQ)> = Vec::new();
        for v in &views {
            if v.state.ended() || v.kind == WorkKind::Conversation {
                continue;
            }
            let Some(q) = v.question.as_ref() else {
                continue;
            };
            if !seen.insert(q.id.clone()) {
                continue;
            }
            let who = if v.kind == WorkKind::Question {
                v.parent
                    .as_deref()
                    .and_then(|p| titles.get(p).cloned())
                    .unwrap_or_else(|| theseus_protocol::notices::name_of(&q.asked_by))
            } else {
                task_tree::name(v)
            };
            let mut options = q.options.clone();
            if options.is_empty() {
                options = theseus_protocol::work::answer_options(q);
            }
            let mut detail = vec![q.prompt.clone()];
            detail.extend(q.detail.iter().cloned());
            if let Some(i) = &q.input {
                detail.push(serde_json::to_string_pretty(i).unwrap_or_default());
            }
            out.push((
                q.asked_at_ms,
                TaskQ {
                    id: q.id.clone(),
                    who,
                    words: q.prompt.lines().next().unwrap_or("").trim().to_string(),
                    kind: q.kind,
                    options,
                    expires_at_ms: q.expires_at_ms,
                    detail,
                },
            ));
        }
        out.sort_by_key(|(at, p)| (*at, p.id.clone()));
        out.into_iter().map(|(_, p)| p).collect()
    }

    /// A question that already waits when the chat connects, announced in
    /// the transcript once. Its keys are the dock's.
    fn tell_question(&mut self, p: &TaskQ) {
        if !self.board.told.insert(p.id.clone()) {
            return;
        }
        let line = Line::from([
            span(Sty::Yellow, "● "),
            span(Sty::Bold, p.who.clone()),
            span(Sty::Plain, format!(" asks: {}?", p.words)),
        ]);
        self.say(&line);
    }

    /// `work.answer` for a task's question, unless this is a job's shell.
    pub(super) fn answer_task(
        &mut self,
        p: &TaskQ,
        option: &str,
        note: Option<String>,
    ) -> Vec<Effect> {
        if let Err(e) = theseus_client::client::refuse_in_a_job(
            method::WORK_ANSWER,
            self.job_session.as_deref(),
        ) {
            self.say_words(Sty::Red, &format!("✗ {e}"));
            return Vec::new();
        }
        let mut params = json!({
            "question_id": p.id, "option": option, "author": AUTHOR,
        });
        if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
            params["note"] = json!(n.trim());
        }
        self.ask.sent = true;
        vec![call(
            method::WORK_ANSWER,
            params,
            Purpose::TaskAnswer {
                who: p.who.clone(),
                option: option.to_string(),
            },
        )]
    }

    /// `v` on a task's question: the call in full, above the dock.
    pub(super) fn show_task_call(&mut self, p: &TaskQ) {
        self.say_plain(Sty::Dim, &format!("  {} asks:", p.who));
        for part in &p.detail {
            for l in part.lines().take(40) {
                let row = format!("    {l}");
                self.say_plain(Sty::Dim, &row);
            }
        }
    }

    /// An answer to a task's question came back.
    pub(super) fn task_answered(&mut self, who: &str, option: &str) {
        let word = if notices_approves(option) {
            "approved"
        } else {
            "declined"
        };
        self.say_plain(Sty::Dim, &format!("  {who}: {word} ({option})"));
    }

    // ------------------------------------------------------ commands

    /// `/tasks`, `ctrl-t`: the tree, in place.
    pub fn show_tasks(&mut self) {
        let views: Vec<WorkView> = self.board.views.values().cloned().collect();
        let lines = task_tree::tree(&views, self.now_ms, usize::from(self.width), 0);
        self.gap();
        for l in lines {
            self.say_plain(sty_of(l.tag), &l.text);
        }
        self.gap();
    }

    /// `/cancel ID`: one `task.cancel`.
    pub fn cancel_task(&mut self, id: &str) -> Vec<Effect> {
        let id = id.trim();
        if id.chars().count() < 4 {
            self.flash("/cancel ID · at least four characters of a task's id");
            return Vec::new();
        }
        vec![call(
            method::TASK_CANCEL,
            json!({ "task": id, "author": AUTHOR }),
            Purpose::Cancel(id.to_string()),
        )]
    }
}

fn notices_approves(option: &str) -> bool {
    theseus_protocol::answer::approves(option)
}

/// Only a task's own events are the chat's to print: the conversation's
/// turns and its own questions come by its own session.
fn task_event_is_ours(e: &TaskEvent) -> bool {
    match e {
        TaskEvent::Started(v)
        | TaskEvent::Asked(v, _)
        | TaskEvent::Reported(v)
        | TaskEvent::Failed(v)
        | TaskEvent::Cancelled(v)
        | TaskEvent::Step(v)
        | TaskEvent::PastUsual(v)
        | TaskEvent::Paused(v)
        | TaskEvent::Resumed(v) => v.kind == WorkKind::Task,
    }
}

/// An event as a styled line.
fn words(e: &TaskEvent) -> Line {
    let l = task_tree::event_line(e);
    one(sty_of(l.tag), l.text)
}

/// A task question's keys, in one line.
pub(super) fn keys_line(p: &TaskQ) -> String {
    let mut parts = Vec::new();
    if p.kind == QuestionKind::Ask {
        for (i, o) in p
            .options
            .iter()
            .filter(|o| o.id != "choice:other")
            .take(6)
            .enumerate()
        {
            parts.push(format!("[{}] {}", i + 1, o.label.to_lowercase()));
        }
    } else {
        let yes = p
            .options
            .first()
            .map_or("approve", |o| o.label.as_str())
            .to_lowercase();
        parts.push(format!("[y] {yes}"));
        parts.push("[n] decline".to_string());
    }
    parts.push("[v] view".to_string());
    parts.join(" · ")
}
