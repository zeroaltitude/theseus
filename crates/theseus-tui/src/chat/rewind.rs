//! Rewind (theseus-23rk, R20): Esc Esc on an empty line while no turn runs
//! opens a picker in the dock, one row for each of your messages with its time
//! and the file changes since it. Enter chooses a message; then `f` puts the
//! files and the conversation back (`files.restore`, then `session.fork` at
//! the node before the message, and the chat goes on in the fork with the
//! message back in the input to edit), `c` forks only, `o` restores the files
//! only, Esc never mind. A single Esc during a turn is the stop (chat-ask's),
//! so it is not bound here. Files that programs changed are listed, never
//! restored, and the picker says so.

use serde_json::{json, Value};
use theseus_client::render::rewind::changes_words;
use theseus_protocol::{
    method, FilesChangesResult, FilesRestoreResult, RewindPoint, SessionForkResult,
};

use super::dock::DockCtx;
use super::keys::Action;
use super::state::{call, Chat, Effect, Purpose};
use super::style::{one, span, Line, Sty};

/// The messages the picker shows at once.
const ROWS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stage {
    Choose,
    Mode,
}

pub(super) struct Picker {
    points: Vec<RewindPoint>,
    sel: usize,
    stage: Stage,
}

/// What a rewind in flight is doing, carried by its requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewinding {
    /// The message, to put back in the input.
    pub text: String,
    /// The node a fork ends at; none to restore the files alone.
    pub fork_at: Option<String>,
}

impl Chat {
    /// Escape on an idle, empty line: the first arms, the second opens the
    /// picker. None: not ours (a turn runs, or the line holds text).
    pub(super) fn rewind_escape(&mut self) -> Option<Vec<Effect>> {
        if self.live.active() || !self.editor.is_empty() || self.question.is_some() {
            return None;
        }
        let sid = self.session.clone()?;
        if !self.esc_armed {
            self.esc_armed = true;
            self.flash("esc again to rewind");
            return Some(Vec::new());
        }
        self.esc_armed = false;
        self.notice = None;
        Some(vec![call(
            method::FILES_CHANGES,
            json!({ "session_id": sid }),
            Purpose::RewindPoints,
        )])
    }

    /// The picker's points arrived.
    pub(super) fn rewind_points(&mut self, v: Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<FilesChangesResult>(v) else {
            self.flash("the daemon's answer about your messages could not be read");
            return Vec::new();
        };
        if r.points.is_empty() {
            self.flash("nothing to rewind to yet");
            return Vec::new();
        }
        self.rewind_picker = Some(Picker {
            points: r.points,
            sel: 0,
            stage: Stage::Choose,
        });
        Vec::new()
    }

    /// A key while the picker is open. It takes every key.
    pub(super) fn rewind_key(&mut self, action: Action) -> Vec<Effect> {
        let Some(p) = self.rewind_picker.as_mut() else {
            return Vec::new();
        };
        match (p.stage, action) {
            (_, Action::Escape | Action::DoubleEscape | Action::Interrupt) => {
                self.rewind_picker = None
            }
            (Stage::Choose, Action::Up) => p.sel = p.sel.saturating_sub(1),
            (Stage::Choose, Action::Down) => p.sel = (p.sel + 1).min(p.points.len() - 1),
            (Stage::Choose, Action::Submit) => p.stage = Stage::Mode,
            (Stage::Mode, Action::Insert(c)) => return self.rewind_mode(c),
            _ => {}
        }
        Vec::new()
    }

    /// `f`, `c` or `o` on the chosen message.
    fn rewind_mode(&mut self, c: char) -> Vec<Effect> {
        let Some(sid) = self.session.clone() else {
            return Vec::new();
        };
        let Some(p) = self.rewind_picker.as_ref() else {
            return Vec::new();
        };
        let point = p.points[p.sel].clone();
        let (files, conversation) = match c.to_ascii_lowercase() {
            'f' => (true, true),
            'c' => (false, true),
            'o' => (true, false),
            _ => return Vec::new(),
        };
        if conversation && point.fork_at.is_none() {
            self.flash(
                "the first message has nothing before it to go back to · o puts the files back",
            );
            return Vec::new();
        }
        self.rewind_picker = None;
        let going = Rewinding {
            text: point.message.clone(),
            fork_at: conversation.then(|| point.fork_at.clone()).flatten(),
        };
        if files {
            return vec![call(
                method::FILES_RESTORE,
                json!({ "session_id": sid, "since_node": point.node_id }),
                Purpose::RewindRestore(going),
            )];
        }
        vec![Self::fork_call(&sid, going)]
    }

    fn fork_call(sid: &str, going: Rewinding) -> Effect {
        let at = going.fork_at.clone().unwrap_or_default();
        call(
            method::SESSION_FORK,
            json!({ "session": sid, "at": at }),
            Purpose::RewindFork(going),
        )
    }

    /// `files.restore` answered: say what became of each file, then fork
    /// when the conversation goes back too.
    pub(super) fn rewind_restored(&mut self, going: Rewinding, v: Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<FilesRestoreResult>(v) else {
            self.flash("the daemon's answer about the files could not be read");
            return Vec::new();
        };
        self.gap();
        for f in &r.files {
            let (sty, tail) = match f.outcome.as_str() {
                "skipped" => (Sty::Yellow, format!(" — {}", f.why)),
                _ => (Sty::Green, String::new()),
            };
            self.say_words(sty, &format!("{} {}{tail}", f.outcome, f.path));
        }
        self.say_words(Sty::Dim, &r.words);
        if r.files.iter().any(|f| f.outcome != "skipped") {
            self.say_words(
                Sty::Dim,
                &format!(
                    "undo it with: theseus rewind {} --to @{} --files-only",
                    self.session.as_deref().unwrap_or(""),
                    r.undo_since
                ),
            );
        }
        match (&going.fork_at, self.session.clone()) {
            (Some(_), Some(sid)) => vec![Self::fork_call(&sid, going)],
            // Files only: the conversation stays where it is.
            _ => Vec::new(),
        }
    }

    /// `session.fork` answered: the chat goes on in the fork, with the
    /// message back in the input.
    pub(super) fn rewind_forked(&mut self, going: Rewinding, v: Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<SessionForkResult>(v) else {
            self.flash("the daemon's answer about the fork could not be read");
            return Vec::new();
        };
        let from = self.session.clone().unwrap_or_default();
        // The conversation starts again on a clean screen: the fork's own
        // history is read and shown, and the message is back in the input.
        self.out.push(super::screen::CLEAR.to_string());
        self.session = Some(r.session_id.clone());
        self.live = Default::default();
        self.cost = 0.0;
        self.rewound = Some(from);
        self.editor.insert_str(&going.text);
        vec![
            call(
                method::SESSION_HISTORY,
                json!({ "session_id": r.session_id, "n": 40 }),
                Purpose::History(r.session_id.clone()),
            ),
            call(
                method::SESSION_WATCH,
                json!({ "session_id": r.session_id }),
                Purpose::Watch,
            ),
        ]
    }

    /// A rewind's request was refused.
    pub(super) fn rewind_failed(&mut self, what: &str, message: &str) {
        self.say_words(Sty::Red, &format!("✗ {what}: {message}"));
    }
}

/// The picker's rows in the dock.
pub(super) fn rows(chat: &Chat, ctx: &DockCtx) -> Vec<Line> {
    let Some(p) = &chat.rewind_picker else {
        return Vec::new();
    };
    let mut out = Vec::new();
    match p.stage {
        Stage::Choose => out.push(one(
            Sty::Bold,
            "Rewind to before which message?  ↑/↓ choose · enter select · esc never mind",
        )),
        Stage::Mode => out.push(one(
            Sty::Bold,
            "f files and conversation · c conversation only · o files only · esc never mind",
        )),
    }
    let first = p
        .sel
        .saturating_sub(ROWS - 1)
        .min(p.points.len().saturating_sub(ROWS));
    for (i, pt) in p.points.iter().enumerate().skip(first).take(ROWS) {
        let mark = if i == p.sel { "› " } else { "  " };
        let when = theseus_client::render::time::fmt_hm(pt.at_ms);
        let changes = changes_words(pt);
        let room = ctx
            .width
            .saturating_sub(when.len() + changes.chars().count() + 8);
        let mut text: String = pt.text.chars().take(room.max(8)).collect();
        if pt.text.chars().count() > text.chars().count() {
            text.pop();
            text.push('…');
        }
        let sty = if i == p.sel { Sty::Cyan } else { Sty::Plain };
        out.push(vec![
            span(sty, format!("{mark}{when}  {text}")),
            span(Sty::Dim, format!("  {changes}")),
        ]);
    }
    if p.points.iter().any(|pt| pt.commands > 0) {
        out.push(one(
            Sty::Dim,
            "  files that programs changed are listed, never restored",
        ));
    }
    out
}
