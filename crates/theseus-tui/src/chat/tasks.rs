//! The chat's tasks (theseus-mnd9g): the work board of this conversation,
//! kept from one `work.watch` (`interactive`, so the daemon knows a person is
//! looking), the lines it prints as tasks start, ask, report and fail, the
//! questions that wait for you and their one-key answers, and the commands
//! `/tasks`, `/cancel` and `/stop`.
//!
//! Nothing here runs a timer. The board changes when the daemon pushes, and
//! the footer's clocks turn over on the minute, so the chat asks the runner
//! for a deadline only while a row has one (`repaint_at`).
//!
//! A question waits for a key while the input line is empty and `esc` has not
//! been pressed: `y` / `n` / `t`, `1`..`6` for an `ask`'s choices, `v` for
//! the call in full, `tab` for the next question. `esc` is writing: the
//! question stays in the footer and every key is text, so a message that
//! starts with "y" is a message. A message sent ends writing.

use std::collections::{HashMap, HashSet};

use serde_json::json;
use theseus_client::render::task_tree::{self, Event as TaskEvent};
use theseus_client::render::Tag;
use theseus_protocol::notices::{self, Notice, Urgency, Viewer, Why};
use theseus_protocol::work::{AnswerOption, QuestionKind, WorkKind, WorkView};
use theseus_protocol::{method, WorkChanged, WorkWatchResult};

use super::state::{call, Chat, Effect, Purpose};
use super::style::{one, span, wrap_words, Line, Sty};

/// What the chat's answers to a question carry as their author.
pub const ANSWER_AUTHOR: &str = "theseus chat";

/// The footer shows this many tasks, then `+N more · ctrl-t`.
pub const FOOTER_ROWS: usize = 5;

/// A question that waits for you: this conversation's own (a call its turn
/// asks) or a task's.
#[derive(Debug, Clone)]
pub(super) struct Pending {
    pub id: String,
    /// The task that asks; none for the conversation's own.
    pub who: Option<String>,
    pub words: String,
    pub kind: QuestionKind,
    pub options: Vec<AnswerOption>,
    pub external_text: bool,
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
    /// The question the keys answer; the first when none, or when it left.
    pub focus: Option<String>,
    /// `esc`: keys are text until a message is sent.
    pub writing: bool,
    /// `n` was pressed on this question: the line is its note.
    pub note: Option<String>,
    /// Questions already announced in the transcript.
    pub told: HashSet<String>,
    /// When the footer next changes by itself.
    pub repaint_at: Option<u64>,
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
    pub(super) fn watch_work(&self, sid: &str) -> Effect {
        call(
            method::WORK_WATCH,
            json!({ "roots": [sid], "limit": 0, "interactive": true }),
            Purpose::WorkWatch,
        )
    }

    /// The snapshot: the board as it stands. No line is printed for what
    /// already runs; a question that already waits is told once.
    pub(super) fn work_snapshot(&mut self, r: WorkWatchResult) {
        self.board.live = true;
        self.board.views = r.views.into_iter().map(|v| (v.id.clone(), v)).collect();
        let waiting: Vec<Pending> = self.task_questions();
        for p in waiting {
            self.tell_question(&p);
        }
        self.board_changed();
    }

    /// A frame's changes, by the position rule.
    pub(super) fn work_changed(&mut self, c: WorkChanged) {
        for v in c.views {
            let prev = self.board.views.get(&v.id);
            if prev.is_some_and(|p| p.position >= v.position) {
                continue;
            }
            let events: Vec<(Line, bool)> = task_tree::events(prev, &v)
                .iter()
                .filter(|e| task_event_is_ours(e))
                .map(|e| (words(e), matches!(e, TaskEvent::Asked(..))))
                .collect();
            let asked = events.iter().any(|(_, a)| *a);
            for (line, _) in events {
                self.say(&line);
            }
            self.board.views.insert(v.id.clone(), v);
            if asked {
                if let Some(p) = self.task_questions().into_iter().last() {
                    self.tell_question(&p);
                }
            }
        }
        for id in c.gone {
            self.board.views.remove(&id);
        }
        self.board_changed();
    }

    /// A notice from the daemon's policy: a ping rings once. The words of a
    /// task's own events were printed from its views; the rest are printed
    /// here, as the notice says them.
    pub(super) fn work_notice(&mut self, n: &Notice) -> Vec<Effect> {
        let viewer = Viewer {
            focused: self.session.clone(),
            seen: 0,
            away_ms: 0,
            quiet: false,
            sound: true,
        };
        let delivery = notices::deliver(n, &viewer);
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

    /// The board moved: the status line's count, the footer's next clock.
    fn board_changed(&mut self) {
        if self.pending().is_empty() {
            self.board.writing = false;
            self.board.note = None;
        }
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
            .focused()
            .and_then(|p| p.expires_at_ms)
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

    /// Every question that waits for you: the conversation's own first, then
    /// the tasks', the oldest asked first.
    pub(super) fn pending(&self) -> Vec<Pending> {
        let mut out = Vec::new();
        if let Some(q) = &self.question {
            let words = if q.budget.is_some() {
                "the spend limit is reached".to_string()
            } else {
                self.tool_words(&q.tool, &q.input)
            };
            out.push(Pending {
                id: q.correlation_id.clone(),
                who: None,
                words,
                kind: if q.budget.is_some() {
                    QuestionKind::Budget
                } else {
                    QuestionKind::Approval
                },
                options: q.options.clone(),
                external_text: q.external_text.is_some(),
                expires_at_ms: (q.expires_at_ms > 0).then_some(q.expires_at_ms),
                detail: vec![
                    self.tool_words(&q.tool, &q.input),
                    serde_json::to_string_pretty(&q.input).unwrap_or_default(),
                ],
            });
        }
        out.extend(self.task_questions());
        out
    }

    fn task_questions(&self) -> Vec<Pending> {
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
        let mut out: Vec<(u64, Pending)> = Vec::new();
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
                Pending {
                    id: q.id.clone(),
                    who: Some(who),
                    words: q.prompt.lines().next().unwrap_or("").trim().to_string(),
                    kind: q.kind,
                    options,
                    external_text: q.external_text,
                    expires_at_ms: q.expires_at_ms,
                    detail,
                },
            ));
        }
        out.sort_by_key(|(at, p)| (*at, p.id.clone()));
        out.into_iter().map(|(_, p)| p).collect()
    }

    /// The question the keys answer.
    pub(super) fn focused(&self) -> Option<Pending> {
        let all = self.pending();
        match &self.board.focus {
            Some(id) => all
                .iter()
                .find(|p| &p.id == id)
                .cloned()
                .or_else(|| all.first().cloned()),
            None => all.first().cloned(),
        }
    }

    /// A question announced in the transcript, once. Its keys are the dock's.
    fn tell_question(&mut self, p: &Pending) {
        if !self.board.told.insert(p.id.clone()) {
            return;
        }
        if let Some(who) = &p.who {
            // The line the task's event printed already names the question
            // when it just came; a snapshot's has none yet.
            let line = Line::from([
                span(Sty::Yellow, "● "),
                span(Sty::Bold, who.clone()),
                span(Sty::Plain, format!(" asks: {}?", p.words)),
            ]);
            if !self.out_has_ask(who, &p.words) {
                self.say(&line);
            }
        }
    }

    fn out_has_ask(&self, who: &str, words: &str) -> bool {
        let needle = format!("{who} asks: {words}?");
        self.out.iter().rev().take(3).any(|l| l.contains(&needle))
    }

    /// A key with the line empty and a question waiting. None: not a key
    /// for it (it is text).
    pub(super) fn question_key(&mut self, c: char) -> Option<Vec<Effect>> {
        if self.board.writing {
            return None;
        }
        let p = self.focused()?;
        let ask = p.kind == QuestionKind::Ask;
        let option = match c {
            'v' | 'V' => {
                self.show_call(&p);
                return Some(Vec::new());
            }
            'n' | 'N' if !ask => {
                // The decline takes a note: the line is for it.
                self.board.note = Some(p.id.clone());
                return Some(Vec::new());
            }
            'y' | 'Y' if !ask => {
                let o = p.options.first().map_or(
                    if p.kind == QuestionKind::Budget {
                        "reset"
                    } else {
                        "approve"
                    },
                    |o| o.id.as_str(),
                );
                o.to_string()
            }
            't' | 'T' if p.external_text => "approve_trust".to_string(),
            '1'..='6' if ask => {
                let n = c as usize - '1' as usize;
                p.options
                    .iter()
                    .filter(|o| o.id != "choice:other")
                    .nth(n)?
                    .id
                    .clone()
            }
            _ => return None,
        };
        Some(self.answer(&p, &option, None))
    }

    /// `work.answer`, unless this is a job's shell.
    pub(super) fn answer(
        &mut self,
        p: &Pending,
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
            "question_id": p.id, "option": option, "author": ANSWER_AUTHOR,
        });
        if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
            params["note"] = json!(n.trim());
        }
        let purpose = match &p.who {
            Some(who) => Purpose::TaskAnswer {
                who: who.clone(),
                option: option.to_string(),
            },
            None => Purpose::Answer,
        };
        vec![call(method::WORK_ANSWER, params, purpose)]
    }

    /// `v`: the call in full, above the dock.
    fn show_call(&mut self, p: &Pending) {
        let who = p.who.as_deref().unwrap_or("this conversation");
        self.say_plain(Sty::Dim, &format!("  {who} asks:"));
        for part in &p.detail {
            for l in part.lines().take(40) {
                let row = format!("    {l}");
                self.say_plain(Sty::Dim, &row);
            }
        }
    }

    /// `tab`: the next question gets the keys.
    pub(super) fn next_question(&mut self) -> bool {
        let all = self.pending();
        if all.len() < 2 {
            return false;
        }
        let at = self
            .focused()
            .and_then(|f| all.iter().position(|p| p.id == f.id))
            .unwrap_or(0);
        self.board.focus = Some(all[(at + 1) % all.len()].id.clone());
        self.board_refresh();
        true
    }

    /// Enter in note mode: the decline, with the line as its note.
    pub(super) fn send_note(&mut self, with_note: bool) -> Vec<Effect> {
        let Some(id) = self.board.note.take() else {
            return Vec::new();
        };
        let taken = self.editor.take().text;
        let Some(p) = self.pending().into_iter().find(|p| p.id == id) else {
            return Vec::new();
        };
        let note = with_note.then_some(taken);
        let decline = p
            .options
            .iter()
            .find(|o| o.id == "decline")
            .map_or("decline", |o| o.id.as_str())
            .to_string();
        self.answer(&p, &decline, note)
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
            json!({ "task": id, "author": ANSWER_AUTHOR }),
            Purpose::Cancel(id.to_string()),
        )]
    }

    /// `/stop`, `/stop all`: the conversation's turn, and with `all` its tasks.
    pub fn stop(&mut self, args: &str) -> Vec<Effect> {
        let all = args.trim() == "all";
        if all {
            if let Err(e) =
                theseus_client::client::refuse_stop_all_in_a_job(self.job_session.as_deref())
            {
                self.say_words(Sty::Red, &format!("✗ {e}"));
                return Vec::new();
            }
        }
        let Some(exe) = self
            .board
            .views
            .values()
            .find(|v| {
                v.kind == WorkKind::Conversation
                    && v.session_id.as_deref() == self.session.as_deref()
            })
            .map(|v| v.id.clone())
        else {
            self.flash("the conversation is not on the board yet");
            return Vec::new();
        };
        vec![call(
            method::EXECUTION_STOP,
            json!({ "execution_id": exe, "tasks": all, "author": ANSWER_AUTHOR }),
            Purpose::Stop,
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
        | TaskEvent::Cancelled(v) => v.kind == WorkKind::Task,
    }
}

/// An event as a styled line.
fn words(e: &TaskEvent) -> Line {
    let l = task_tree::event_line(e);
    one(sty_of(l.tag), l.text)
}

/// The keys a question answers to, in one line.
pub(super) fn keys_line(p: &Pending) -> String {
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
        if p.external_text {
            parts.push("[t] approve and trust".to_string());
        }
    }
    parts.push("[v] view".to_string());
    parts.join(" · ")
}

/// Words wrapped for the dock.
pub(super) fn wrapped(text: &str, width: usize) -> Vec<String> {
    wrap_words(text, width, 2)
}
