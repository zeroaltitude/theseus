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

use serde_json::json;
use theseus_client::render::diffview;
use theseus_protocol::{method, ConfirmRequest};

use super::dock::DockCtx;
use super::keys::{self, Ask};
use super::live::elapsed;
use super::state::{call, Chat, Effect, Purpose, AUTHOR};
use super::style::{one, span, wrap_words, Line, Sty};

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
}

impl Chat {
    /// A question is new or gone: the focus is on it, or on nothing.
    pub(super) fn ask_reset(&mut self) {
        self.ask = AskState::default();
    }

    /// Whether a one-key answer applies now: a question waits, the person has
    /// not gone back to typing, and the line is empty.
    pub fn answering(&self) -> bool {
        self.question.is_some()
            && !self.ask.typing
            && !self.ask.note
            && !self.ask.sent
            && self.editor.is_empty()
    }

    /// A character with the line empty: an answer when it is one, else None
    /// and the character is text.
    pub(super) fn question_key(&mut self, c: char) -> Option<Vec<Effect>> {
        if self.question.is_none() || self.ask.typing || self.ask.note {
            return None;
        }
        let (key, which) = keys::ask_key(c)?;
        if self.ask.sent {
            // Answered, and the daemon has not said so yet: a second key
            // would be a second answer.
            return Some(Vec::new());
        }
        let q = self.question.clone()?;
        match which {
            Ask::Yes => Some(self.confirm(&q, true, None, false, false)),
            Ask::Trust if q.external_text.is_some() => {
                Some(self.confirm(&q, true, None, true, false))
            }
            Ask::Grant if q.grant_offer.as_ref().is_some_and(|o| o.key() == key) => {
                Some(self.confirm(&q, true, None, false, true))
            }
            Ask::No => {
                self.ask.note = true;
                Some(Vec::new())
            }
            Ask::Whole => {
                self.show_whole(&q);
                Some(Vec::new())
            }
            Ask::Trust | Ask::Grant => None,
        }
    }

    /// Enter on the note: the decline goes with it (or without, when the line
    /// is empty).
    pub(super) fn send_note(&mut self) -> Vec<Effect> {
        let note = self.editor.take().text;
        self.ask.note = false;
        let note = note.trim();
        let Some(q) = self.question.clone() else {
            return Vec::new();
        };
        let note = (!note.is_empty()).then(|| note.to_string());
        self.confirm(&q, false, note, false, false)
    }

    /// One `action.confirm`.
    fn confirm(
        &mut self,
        q: &ConfirmRequest,
        approve: bool,
        note: Option<String>,
        trust: bool,
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
        if trust {
            params["trust"] = json!(true);
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
        if self.question.is_some() && !self.ask.typing && self.editor.is_empty() {
            self.ask.typing = true;
            return true;
        }
        false
    }

    /// Tab with the line empty: the question has the focus again.
    pub(super) fn focus_question(&mut self) -> bool {
        if self.question.is_some() && self.ask.typing && self.editor.is_empty() {
            self.ask.typing = false;
            return true;
        }
        false
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
        let words = self.tool_words(&q.tool, &q.input);
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

/// How long until it expires, or that it did.
fn expiry(q: &ConfirmRequest, now_ms: u64) -> Option<String> {
    (q.expires_at_ms > 0).then(|| {
        if q.expires_at_ms > now_ms {
            format!("expires in {}", elapsed(q.expires_at_ms - now_ms))
        } else {
            "expired".to_string()
        }
    })
}

/// The dock's rows for the question.
pub fn rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(q) = &chat.question else {
        return Vec::new();
    };
    let title = chat.ask_title(q);
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
    if chat.ask.typing {
        // The question waits one row high while the person types.
        out.push(one(Sty::Dim, "  tab: answer it"));
        return out;
    }
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
    if !q.reason.is_empty() {
        for row in wrap_words(
            &format!("why: {}", q.reason),
            ctx.width.saturating_sub(2),
            2,
        ) {
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
        out.push(one(Sty::Yellow, keys_line(q)));
    }
    out
}

/// `y yes · n no · v whole call · esc type`, with the question's extra keys.
fn keys_line(q: &ConfirmRequest) -> String {
    let yes = if q.budget.is_some() { "reset" } else { "yes" };
    let mut line = format!("  y {yes} · n no, say what instead · v whole call");
    if q.external_text.is_some() {
        line.push_str(" · t yes and trust");
    }
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
