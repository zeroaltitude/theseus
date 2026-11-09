//! The question in the dock (theseus-wy6v.1): while one waits, the dock shows
//! it in place of the input, with the call in words, what it would do (an
//! edit's hunk, numbered, its changed words marked; a command as its shell
//! line with its directory), who asks and when it expires, and the keys that
//! answer it. The keys are `keys::ASK_KEYS`, one table; each works only while
//! the input line is empty and the question has the focus, so a message that
//! starts with "y" is a message.
//!
//! Esc goes back to typing (the question stays, one compact row, and Tab
//! gives it the focus again); `n` takes an optional note, and Enter sends the
//! decline. An answer is `action.confirm`, author `theseus chat`.
//!
//! The one model also holds the questions this conversation's tasks ask
//! (theseus-mnd9g): [`Asked`] is the conversation's own or a task's, drawn by
//! the same rows (the task's title says who asks) and answered by the same
//! keys, `1`..`6` choosing an `ask` question's options. A task's answer is
//! `work.answer`. With several waiting, the conversation's own comes first,
//! then the tasks' oldest first; Tab, with the question holding the focus,
//! moves it to the next.

use serde_json::json;
use theseus_client::render::diffview;
use theseus_protocol::{method, ConfirmRequest};

use super::dock::DockCtx;
use super::keys::{self, Ask};
use super::live::elapsed;
use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::{one, span, wrap_words, Line, Sty};
use super::tasks::TaskQ;
use theseus_protocol::work::QuestionKind;

/// The most rows of a call's preview the dock shows beyond what the terminal
/// has room for.
const PREVIEW_MAX: usize = 24;
const PREVIEW_MIN: usize = 4;

/// Where the person is in answering the question in the dock.
#[derive(Debug, Default)]
pub struct AskState {
    /// Esc went back to typing: the keys type until Tab gives the focus back.
    pub typing: bool,
    /// `n` was pressed: the line is the decline's note.
    pub note: bool,
    /// An answer is sent for this question and not yet heard back.
    pub sent: bool,
    /// Tab chose this question (its id) to hold the focus; none, the first.
    pub focus: Option<String>,
    /// The id of the question the fields above are about.
    pub seen: Option<String>,
}

/// A question that waits for you: this conversation's own, or a task's.
#[derive(Debug, Clone)]
pub(super) enum Asked {
    Own(Box<ConfirmRequest>),
    Task(Box<TaskQ>),
}

impl Asked {
    fn id(&self) -> &str {
        match self {
            Asked::Own(q) => &q.correlation_id,
            Asked::Task(t) => &t.id,
        }
    }
}

impl Chat {
    /// Every question that waits: the conversation's own, then its tasks'.
    pub(super) fn asked(&self) -> Vec<Asked> {
        let mut out: Vec<Asked> = self
            .question
            .iter()
            .map(|q| Asked::Own(Box::new(q.clone())))
            .collect();
        out.extend(
            self.task_questions()
                .into_iter()
                .map(|t| Asked::Task(Box::new(t))),
        );
        out
    }

    /// The question that has the keys: the one Tab chose, else the first.
    pub(super) fn current(&self) -> Option<Asked> {
        let all = self.asked();
        let at = self
            .ask
            .focus
            .as_deref()
            .and_then(|f| all.iter().position(|a| a.id() == f))
            .unwrap_or(0);
        all.into_iter().nth(at)
    }

    /// When the question that has the keys expires, if it does.
    pub(super) fn asked_expiry(&self) -> Option<u64> {
        match self.current()? {
            Asked::Own(q) => Some(crate::held::deadline_of(&q)).filter(|e| *e > 0),
            Asked::Task(t) => t.expires_at_ms,
        }
    }

    /// The question in the dock changed (one came, went or was answered):
    /// the state of answering starts over, keeping a Tab's choice that
    /// still waits.
    pub(super) fn ask_sync(&mut self) {
        let now = self.current().map(|a| a.id().to_string());
        if now != self.ask.seen {
            let waiting: Vec<String> = self.asked().iter().map(|a| a.id().to_string()).collect();
            let focus = self.ask.focus.take().filter(|f| waiting.contains(f));
            self.ask = AskState {
                focus,
                seen: now,
                ..AskState::default()
            };
        }
    }

    /// Whether a one-key answer applies now: a question waits, the person has
    /// not gone back to typing, and the line is empty.
    pub fn answering(&self) -> bool {
        self.current().is_some()
            && !self.ask.typing
            && !self.ask.note
            && !self.ask.sent
            && self.editor.is_empty()
    }

    /// A character with the line empty: an answer when it is one, else None
    /// and the character is text.
    pub(super) fn question_key(&mut self, c: char) -> Option<Vec<Effect>> {
        if self.ask.typing || self.ask.note {
            return None;
        }
        let cur = self.current()?;
        let (key, which) = keys::ask_key(c)?;
        if self.ask.sent {
            // Answered, and the daemon has not said so yet: a second key
            // would be a second answer.
            return Some(Vec::new());
        }
        match cur {
            Asked::Own(q) => self.own_key(&q, key, which),
            Asked::Task(t) => self.task_key(&t, which),
        }
    }

    fn own_key(&mut self, q: &ConfirmRequest, key: char, which: Ask) -> Option<Vec<Effect>> {
        match which {
            Ask::Yes => Some(self.confirm(q, true, None, false)),
            Ask::Grant if q.grant_offer.as_ref().is_some_and(|o| o.key() == key) => {
                Some(self.confirm(q, true, None, true))
            }
            Ask::No => {
                self.ask.note = true;
                Some(Vec::new())
            }
            Ask::Whole => {
                self.show_whole(q);
                Some(Vec::new())
            }
            Ask::Grant | Ask::Choice(_) => None,
        }
    }

    /// A task's question: `y` and `n` for an approval, `1`..`6` for an
    /// `ask`'s choices (the other kind's keys are text there), `v` for the
    /// call.
    fn task_key(&mut self, t: &TaskQ, which: Ask) -> Option<Vec<Effect>> {
        let ask = t.kind == QuestionKind::Ask;
        match which {
            Ask::Whole => {
                self.show_task_call(t);
                Some(Vec::new())
            }
            Ask::No if !ask => {
                self.ask.note = true;
                Some(Vec::new())
            }
            Ask::Yes if !ask => {
                let yes = t.options.first().map_or(
                    if t.kind == QuestionKind::Budget {
                        "reset"
                    } else {
                        "approve"
                    },
                    |o| o.id.as_str(),
                );
                Some(self.answer_task(t, yes, None))
            }
            Ask::Choice(n) if ask => {
                let o = t
                    .options
                    .iter()
                    .filter(|o| o.id != "choice:other")
                    .nth(usize::from(n))?;
                Some(self.answer_task(t, &o.id, None))
            }
            _ => None,
        }
    }

    /// Enter on the note: the decline goes with it (or without, when the line
    /// is empty).
    pub(super) fn send_note(&mut self) -> Vec<Effect> {
        let note = self.editor.take().text;
        self.ask.note = false;
        let note = note.trim();
        let note = (!note.is_empty()).then(|| note.to_string());
        match self.current() {
            Some(Asked::Own(q)) => self.confirm(&q, false, note, false),
            Some(Asked::Task(t)) => {
                let decline = t
                    .options
                    .iter()
                    .find(|o| o.id == "decline")
                    .map_or("decline", |o| o.id.as_str())
                    .to_string();
                self.answer_task(&t, &decline, note)
            }
            None => Vec::new(),
        }
    }

    /// One `action.confirm`.
    fn confirm(
        &mut self,
        q: &ConfirmRequest,
        approve: bool,
        note: Option<String>,
        grant: bool,
    ) -> Vec<Effect> {
        let mut params = json!({
            "correlation_id": q.correlation_id,
            "approve": approve,
            "author": AUTHOR,
        });
        if let Some(n) = note {
            params["note"] = json!(n);
        }
        if grant {
            if let Some(o) = &q.grant_offer {
                params["grant"] = json!({ "pattern": o.pattern });
            }
        }
        self.ask.sent = true;
        vec![call(method::ACTION_CONFIRM, params, Purpose::Answer)]
    }

    /// Esc, first: leaves a note, or goes back to typing. True when it did.
    pub(super) fn escape_question(&mut self) -> bool {
        if self.ask.note {
            self.ask.note = false;
            self.editor.clear();
            return true;
        }
        if self.current().is_some() && !self.ask.typing && self.editor.is_empty() {
            self.ask.typing = true;
            return true;
        }
        false
    }

    /// Tab with the line empty: back from typing, the question has the focus
    /// again; else, with more than one waiting, the next one has it.
    pub(super) fn focus_question(&mut self) -> bool {
        if !self.editor.is_empty() || self.ask.note {
            return false;
        }
        let all = self.asked();
        if all.is_empty() {
            return false;
        }
        if self.ask.typing {
            self.ask.typing = false;
            return true;
        }
        if all.len() < 2 {
            return false;
        }
        let at = self
            .current()
            .and_then(|c| all.iter().position(|a| a.id() == c.id()))
            .unwrap_or(0);
        let next = all[(at + 1) % all.len()].id().to_string();
        self.ask = AskState {
            focus: Some(next.clone()),
            seen: Some(next),
            ..AskState::default()
        };
        self.board_refresh();
        true
    }

    /// `v`: the whole call, written into the transcript.
    fn show_whole(&mut self, q: &ConfirmRequest) {
        self.gap();
        let words = self.tool_words(&q.tool, &q.input);
        self.say_plain(Sty::Bold, &format!("the whole call · {words}"));
        let width = usize::from(self.width);
        let preview = diffview::preview_lines(q);
        if preview.is_empty() {
            let json = serde_json::to_string_pretty(&q.input).unwrap_or_default();
            for l in json.lines() {
                self.say_plain(Sty::Dim, &format!("  {l}"));
            }
        } else {
            for l in super::diff::lines(&preview, PAD, width) {
                self.say(&l);
            }
        }
        if !q.reason.is_empty() {
            self.say_words(Sty::Dim, &format!("why: {}", q.reason));
        }
        self.gap();
    }

    /// The question's answer was refused: it is open again.
    pub(super) fn answer_refused(&mut self) {
        self.ask.sent = false;
    }

    /// The call in a few words for the title: `Edit tides.py · replace 1
    /// line`.
    pub(super) fn ask_title(&self, q: &ConfirmRequest) -> String {
        if q.budget.is_some() {
            return "the spend limit is reached".to_string();
        }
        // An approval in plain words (theseus-cudar): who wants to do what.
        let words = match &q.plain {
            Some(p) => format!("{} wants to {}", p.who, p.what),
            None => self.tool_words(&q.tool, &q.input),
        };
        match change_words(q) {
            Some(c) => format!("{words} · {c}"),
            None => words,
        }
    }
}

/// The indent of a preview's rows (`diffview::preview_lines`).
const PAD: usize = 6;

/// What an edit's preview changes, in words: `replace 1 line`, `add 3 lines`,
/// `remove 2 lines`, `change +4 −2`.
fn change_words(q: &ConfirmRequest) -> Option<String> {
    let p = q.preview.as_ref()?;
    if p.kind != theseus_protocol::ConfirmPreview::DIFF {
        return None;
    }
    let (added, removed) = theseus_protocol::diff::stat(&p.body);
    let lines = |n: u32| format!("{n} line{}", if n == 1 { "" } else { "s" });
    Some(match (added, removed) {
        (0, 0) => return None,
        (a, 0) => format!("add {}", lines(a)),
        (0, r) => format!("remove {}", lines(r)),
        (a, r) if a == r => format!("replace {}", lines(a)),
        (a, r) => format!("change +{a} −{r}"),
    })
}

/// How long until it expires, or that it did; for a question held past its
/// life, how long it has waited (`held`).
fn expiry(q: &ConfirmRequest, now_ms: u64) -> Option<String> {
    if let Some(waited) = crate::held::waited_label(q, now_ms) {
        return Some(waited);
    }
    (q.expires_at_ms > 0).then(|| {
        if q.expires_at_ms > now_ms {
            format!("expires in {}", elapsed(q.expires_at_ms - now_ms))
        } else {
            "expired".to_string()
        }
    })
}

/// The dock's rows for the question that has the keys.
pub fn rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(cur) = chat.current() else {
        return Vec::new();
    };
    let title = match &cur {
        Asked::Own(q) => chat.ask_title(q),
        Asked::Task(t) => format!("{} asks: {}?", t.who, t.words),
    };
    let mut out = Vec::new();
    for (i, row) in wrap_words(&title, ctx.width.saturating_sub(2), 2)
        .into_iter()
        .take(2)
        .enumerate()
    {
        out.push(vec![
            span(Sty::Yellow, if i == 0 { "? " } else { "  " }),
            span(Sty::Bold, row.trim_start().to_string()),
        ]);
    }
    let more = chat.asked().len().saturating_sub(1);
    if chat.ask.typing {
        // The question waits one row high while the person types.
        out.push(one(Sty::Dim, "  tab: answer it"));
        return out;
    }
    let q = match &cur {
        Asked::Own(q) => q,
        Asked::Task(t) => return task_rows(chat, t, out, more, ctx),
    };
    out.extend(preview(chat, q, ctx));
    // `by` names who is asked (the operator); who asks is the task, or Theseus.
    let mut who = Vec::new();
    match q.task.as_ref() {
        Some(t) => who.push(format!(
            "task {} asks",
            t.title.clone().unwrap_or_else(|| t.short.clone())
        )),
        None => who.push("Theseus asks".to_string()),
    }
    if let Some(e) = expiry(q, ctx.now_ms) {
        who.push(e);
    }
    if !who.is_empty() {
        out.push(one(Sty::Dim, format!("  {}", who.join(" · "))));
    }
    // The plain words say risk, the model's reason and what Yes and No do
    // (theseus-cudar); the gate's own reason stands in for a daemon without.
    let said = match &q.plain {
        Some(p) => p.lines().split_off(1).join(" · "),
        None if q.reason.is_empty() => String::new(),
        None => format!("why: {}", q.reason),
    };
    if !said.is_empty() {
        for row in wrap_words(&said, ctx.width.saturating_sub(2), 2) {
            out.push(one(Sty::Dim, format!("  {}", row.trim_start())));
        }
    }
    if let Some(o) = &q.grant_offer {
        for (i, row) in wrap_words(&o.words, ctx.width.saturating_sub(6), 0)
            .into_iter()
            .enumerate()
        {
            let lead = if i == 0 {
                format!("  {}  ", o.key())
            } else {
                "     ".to_string()
            };
            out.push(one(Sty::Dim, format!("{lead}{row}")));
        }
    }
    if chat.ask.note {
        out.push(one(
            Sty::Yellow,
            "  no · say what to do instead (enter sends, esc goes back)",
        ));
    } else if chat.ask.sent {
        out.push(one(Sty::Dim, "  answering…"));
    } else {
        out.push(one(Sty::Yellow, keys_line(q, more)));
    }
    out
}

/// A task's question under its title: its expiry, then the keys.
fn task_rows(chat: &Chat, t: &TaskQ, mut out: Vec<Line>, more: usize, ctx: &DockCtx) -> Vec<Line> {
    if let Some(at) = t.expires_at_ms {
        let left = at.saturating_sub(ctx.now_ms);
        let words = if at > ctx.now_ms {
            format!("expires in {}", elapsed(left))
        } else {
            "expired".to_string()
        };
        out.push(one(Sty::Dim, format!("  {words}")));
    }
    if chat.ask.note {
        out.push(one(
            Sty::Yellow,
            "  no · say what to do instead (enter sends, esc goes back)",
        ));
    } else if chat.ask.sent {
        out.push(one(Sty::Dim, "  answering…"));
    } else {
        let mut line = format!("  {}", super::tasks::keys_line(t));
        line.push_str(&more_words(more));
        line.push_str(" · esc type");
        out.push(one(Sty::Yellow, line));
    }
    out
}

/// ` · tab next (2 more)`, when another question waits.
fn more_words(more: usize) -> String {
    if more == 0 {
        String::new()
    } else {
        format!(" · tab next ({more} more)")
    }
}

/// `y yes · n no · v whole call · esc type`, with the question's extra keys.
fn keys_line(q: &ConfirmRequest, more: usize) -> String {
    let yes = if q.budget.is_some() { "reset" } else { "yes" };
    let mut line = format!("  y {yes} · n no, say what instead · v whole call");
    line.push_str(&more_words(more));
    line.push_str(" · esc type");
    line
}

/// The preview's rows: as many as the terminal has room for, the rest said.
fn preview(chat: &Chat, q: &ConfirmRequest, ctx: &DockCtx) -> Vec<Line> {
    let rows = diffview::preview_lines(q);
    if rows.is_empty() {
        return Vec::new();
    }
    let room = usize::from(chat.height)
        .saturating_sub(12)
        .clamp(PREVIEW_MIN, PREVIEW_MAX);
    let mut lines = super::diff::lines(&rows, PAD, ctx.width);
    if lines.len() > room {
        let more = lines.len() - (room - 1);
        lines.truncate(room - 1);
        lines.push(one(
            Sty::Dim,
            format!("      … {more} more rows · v shows the whole call"),
        ));
    }
    lines
}
