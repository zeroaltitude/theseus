//! The picker (theseus-6yka): `theseus -r` opens it in the chat's dock. This
//! directory's conversations from every surface, newest active first,
//! searched as you type over each one's title, labels, directory and the
//! words of its first and last message (the daemon's `session.list` with
//! `with_text`: a read of the store, no model). Tab widens it to every
//! directory, Enter continues the chosen one here, Esc starts a new one.
//!
//! The list is read once; a keystroke filters it in memory, with no request.
//! No I/O here: `Chat` sends the reads and acts on the [`Outcome`].

use theseus_protocol::{SessionInfo, SessionKind, SessionState};

use super::banner::ago;
use super::dock::{Dock, DockCtx};
use super::keys::Action;
use super::state::Chat;
use super::style::{one, span, Line, Sty};

/// What a key did that the chat must act on.
#[derive(Debug)]
pub enum Outcome {
    /// Nothing to do outside the picker.
    Idle,
    /// Continue this conversation.
    Pick(Box<SessionInfo>),
    /// Tab asked for the list of every directory, which was not read yet.
    Widen,
    /// Leave the picker; a new conversation opens.
    Cancel,
    /// Ctrl-D: leave the chat, opening nothing.
    Leave,
}

pub struct Item {
    pub info: SessionInfo,
    /// What a search looks in, lower-cased once.
    hay: String,
}

impl Item {
    fn new(info: SessionInfo) -> Self {
        let mut hay = String::new();
        let mut add = |t: Option<&str>| {
            if let Some(t) = t.filter(|t| !t.is_empty()) {
                hay.push_str(&t.to_lowercase());
                hay.push('\n');
            }
        };
        add(info.title.as_deref());
        add(info.label.as_deref());
        for t in info.title_was.iter().flatten() {
            add(Some(t));
        }
        add(info.dir.as_deref());
        add(info.first_text.as_deref());
        add(info.last_text.as_deref());
        add(info.prompts.as_deref());
        Self { info, hay }
    }
}

/// The picker's state: two lists (this directory's, and every directory's,
/// read when Tab first asks), the query and the chosen row.
#[derive(Default)]
pub struct Picker {
    here: Option<Vec<Item>>,
    wide: Option<Vec<Item>>,
    /// Tab pressed: every directory's list is shown (or being read).
    pub is_wide: bool,
    pub query: String,
    pub sel: usize,
    /// Why the list could not be read.
    pub failed: Option<String>,
}

/// A conversation worth picking: a person's, with a turn, not retired.
fn pickable(s: &SessionInfo) -> bool {
    s.kind == SessionKind::Conversation && s.turns > 0 && s.state != Some(SessionState::Retired)
}

impl Picker {
    /// The list `session.list` answered, for this directory's or for every
    /// directory's.
    pub fn set(&mut self, wide: bool, sessions: Vec<SessionInfo>) {
        let items: Vec<Item> = sessions
            .into_iter()
            .filter(pickable)
            .map(Item::new)
            .collect();
        if wide {
            self.wide = Some(items);
        } else {
            self.here = Some(items);
        }
        self.sel = 0;
    }

    fn list(&self) -> Option<&Vec<Item>> {
        if self.is_wide {
            self.wide.as_ref()
        } else {
            self.here.as_ref()
        }
    }

    pub fn loading(&self) -> bool {
        self.failed.is_none() && self.list().is_none()
    }

    /// The conversations the query matches, in the list's order: every word
    /// of the query is somewhere in its text.
    pub fn visible(&self) -> Vec<&Item> {
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        self.list()
            .map(|l| {
                l.iter()
                    .filter(|i| words.iter().all(|w| i.hay.contains(w.as_str())))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn key(&mut self, a: Action) -> Outcome {
        match a {
            Action::Insert(c) => {
                self.query.push(c);
                self.sel = 0;
            }
            Action::Backspace => {
                self.query.pop();
                self.sel = 0;
            }
            Action::KillToStart | Action::KillToEnd => {
                self.query.clear();
                self.sel = 0;
            }
            Action::KillWord => {
                let t = self.query.trim_end().rfind(' ').map_or(0, |i| i + 1);
                self.query.truncate(t);
                self.sel = 0;
            }
            Action::Up => self.sel = self.sel.saturating_sub(1),
            Action::Down => {
                let n = self.visible().len();
                self.sel = (self.sel + 1).min(n.saturating_sub(1));
            }
            Action::Tab => {
                self.is_wide = !self.is_wide;
                self.sel = 0;
                if self.is_wide && self.wide.is_none() {
                    return Outcome::Widen;
                }
            }
            Action::Submit => {
                if let Some(i) = self.visible().get(self.sel) {
                    return Outcome::Pick(Box::new(i.info.clone()));
                }
            }
            Action::Escape if !self.query.is_empty() => {
                self.query.clear();
                self.sel = 0;
            }
            Action::Escape | Action::Interrupt => return Outcome::Cancel,
            Action::EndOfInput if self.query.is_empty() => return Outcome::Leave,
            _ => {}
        }
        Outcome::Idle
    }
}

/// A few words of the text around the first of the query's words that it
/// holds, for the chosen row: why this conversation matched.
fn snippet(i: &Item, words: &[String], room: usize) -> Option<String> {
    let texts = [
        i.info.prompts.as_deref(),
        i.info.first_text.as_deref(),
        i.info.last_text.as_deref(),
    ];
    for t in texts.into_iter().flatten() {
        let lower = t.to_lowercase();
        // Only where lower-casing kept every character in place.
        if lower.chars().count() != t.chars().count() {
            continue;
        }
        let Some(at) = words
            .iter()
            .filter_map(|w| lower.find(w.as_str()))
            .min()
            .map(|b| lower[..b].chars().count())
        else {
            continue;
        };
        let chars: Vec<char> = t.chars().collect();
        let from = at.saturating_sub(room / 4);
        let to = (from + room).min(chars.len());
        let mut s: String = chars[from..to]
            .iter()
            .collect::<String>()
            .replace('\n', " ");
        if from > 0 {
            s.insert(0, '…');
        }
        if to < chars.len() {
            s.push('…');
        }
        return Some(s);
    }
    None
}

/// How many results show at once.
fn room(chat: &Chat) -> usize {
    usize::from(chat.height).saturating_sub(6).clamp(3, 8)
}

/// One result's line: `▸ title · 3 h ago · 12 turns · $0.42 · cli`.
fn row(i: &Item, chosen: bool, now_ms: u64, width: usize, wide: bool) -> Line {
    let s = &i.info;
    let facts = format!(
        "{} · {} turn{} · {}{}",
        ago(now_ms, s.last_active_ms),
        s.turns,
        if s.turns == 1 { "" } else { "s" },
        theseus_protocol::usd(s.cost_usd),
        s.started_on
            .as_deref()
            .map_or(String::new(), |w| format!(" · {w}")),
    );
    let mut title = s
        .title
        .clone()
        .or_else(|| s.label.clone())
        .unwrap_or_else(|| "untitled".to_string())
        .replace('\n', " ");
    if wide {
        if let Some(d) = s.dir.as_deref().and_then(|d| d.rsplit('/').next()) {
            if !d.is_empty() {
                title = format!("{title}  [{d}]");
            }
        }
    }
    let room = width.saturating_sub(facts.chars().count() + 6).max(8);
    if title.chars().count() > room {
        title = title.chars().take(room - 1).collect::<String>() + "…";
    }
    let pad = " ".repeat(room.saturating_sub(title.chars().count()));
    vec![
        span(
            if chosen { Sty::Cyan } else { Sty::Plain },
            if chosen { "▸ " } else { "  " },
        ),
        span(if chosen { Sty::Bold } else { Sty::Plain }, title),
        span(Sty::Dim, format!("{pad}  {facts}")),
    ]
}

/// The dock while the picker is open: its heading, the query, the results,
/// and the status line.
pub fn dock(chat: &Chat, p: &Picker, ctx: &DockCtx) -> Dock {
    let width = ctx.width;
    let mut rows: Vec<Line> = Vec::new();
    for provider in super::dock::ABOVE_INPUT {
        rows.extend(provider(chat, ctx));
    }
    let scope = if p.is_wide {
        "every directory".to_string()
    } else {
        chat.cfg.shown_dir.clone()
    };
    rows.push(vec![
        span(Sty::Bold, "resume a conversation"),
        span(
            Sty::Dim,
            format!(
                " · {scope} · tab: {} · esc: a new one · ctrl+d: leave",
                if p.is_wide {
                    "this directory"
                } else {
                    "every directory"
                }
            ),
        ),
    ]);
    let query_row = rows.len();
    rows.push(vec![
        span(Sty::Cyan, "› "),
        span(Sty::Plain, p.query.clone()),
    ]);
    let vis = p.visible();
    if let Some(why) = &p.failed {
        rows.push(one(
            Sty::Red,
            format!("  could not read the conversations: {why}"),
        ));
    } else if p.loading() {
        rows.push(one(Sty::Dim, "  reading the conversations…"));
    } else if vis.is_empty() {
        let words = if p.query.is_empty() && !p.is_wide {
            "  no conversation here yet · tab shows every directory"
        } else if p.query.is_empty() {
            "  no conversations"
        } else {
            "  no conversation matches"
        };
        rows.push(one(Sty::Dim, words));
    }
    let take = room(chat);
    let first = p
        .sel
        .saturating_sub(take - 1)
        .min(vis.len().saturating_sub(take));
    for (n, i) in vis.iter().enumerate().skip(first).take(take) {
        rows.push(row(i, n == p.sel, ctx.now_ms, width, p.is_wide));
    }
    if let Some(i) = vis.get(p.sel) {
        let words: Vec<String> = p.query.split_whitespace().map(str::to_lowercase).collect();
        let room = width.saturating_sub(8);
        let shown = snippet(i, &words, room).or_else(|| {
            let last = i
                .info
                .last_text
                .as_deref()
                .or(i.info.first_text.as_deref())?;
            let line = last.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            Some(line.chars().take(room).collect())
        });
        if let Some(text) = shown {
            rows.push(one(Sty::Dim, format!("    “{}”", text.trim())));
        }
    }
    rows.push(super::dock::status_line(chat, width));
    Dock {
        rows,
        cursor: (query_row, 2 + p.query.chars().count()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn info(sid: &str, title: &str, first: &str, last: &str) -> SessionInfo {
        serde_json::from_value(json!({
            "session_id": sid, "kind": "conversation", "label": null,
            "title": title, "created_at_unix_ms": 1, "turns": 3, "last_active_ms": 9,
            "first_text": first, "last_text": last, "dir": "/home/ada/tides",
        }))
        .unwrap()
    }

    fn picker() -> Picker {
        let mut p = Picker::default();
        p.set(
            false,
            vec![
                info(
                    "ses_a",
                    "survey the log lines",
                    "survey the log lines",
                    "found 3",
                ),
                info(
                    "ses_b",
                    "fix the parser",
                    "the parser drops commas",
                    "done, tests pass",
                ),
                info(
                    "ses_c",
                    "plan the release",
                    "what ships on friday",
                    "ok, tagged",
                ),
            ],
        );
        p
    }

    fn type_in(p: &mut Picker, text: &str) {
        for c in text.chars() {
            p.key(Action::Insert(c));
        }
    }

    #[test]
    fn a_word_of_a_message_finds_its_conversation() {
        let mut p = picker();
        assert_eq!(p.visible().len(), 3);
        type_in(&mut p, "commas");
        let v = p.visible();
        assert_eq!(v.len(), 1, "a word of a first message");
        assert_eq!(v[0].info.session_id, "ses_b");
        p.key(Action::KillToStart);
        type_in(&mut p, "TAGGED");
        assert_eq!(
            p.visible()[0].info.session_id,
            "ses_c",
            "a last message, any case"
        );
        p.key(Action::KillToStart);
        type_in(&mut p, "parser pass");
        assert_eq!(p.visible().len(), 1, "every word, in any of the text");
        type_in(&mut p, " zzz");
        assert!(p.visible().is_empty());
    }

    #[test]
    fn enter_picks_the_chosen_row_and_esc_clears_before_it_cancels() {
        let mut p = picker();
        p.key(Action::Down);
        match p.key(Action::Submit) {
            Outcome::Pick(i) => assert_eq!(i.session_id, "ses_b"),
            o => panic!("{o:?}"),
        }
        type_in(&mut p, "zzz");
        assert!(
            matches!(p.key(Action::Submit), Outcome::Idle),
            "nothing matches"
        );
        assert!(matches!(p.key(Action::Escape), Outcome::Idle));
        assert!(p.query.is_empty());
        assert!(matches!(p.key(Action::Escape), Outcome::Cancel));
    }

    #[test]
    fn tab_widens_once_and_toggles_back() {
        let mut p = picker();
        assert!(matches!(p.key(Action::Tab), Outcome::Widen));
        assert!(p.loading(), "the wide list is being read");
        p.set(true, vec![info("ses_z", "elsewhere", "x", "y")]);
        assert_eq!(p.visible().len(), 1);
        assert!(matches!(p.key(Action::Tab), Outcome::Idle));
        assert_eq!(p.visible().len(), 3, "this directory's list is kept");
        assert!(
            matches!(p.key(Action::Tab), Outcome::Idle),
            "the wide list is kept"
        );
        assert_eq!(p.visible()[0].info.session_id, "ses_z");
    }

    #[test]
    fn tasks_empty_and_retired_conversations_are_not_offered() {
        let mut p = Picker::default();
        let mut task = info("ses_t", "a task", "a", "b");
        task.kind = SessionKind::Task;
        let mut empty = info("ses_e", "empty", "a", "b");
        empty.turns = 0;
        let mut gone = info("ses_g", "gone", "a", "b");
        gone.state = Some(SessionState::Retired);
        p.set(
            false,
            vec![task, empty, gone, info("ses_ok", "kept", "a", "b")],
        );
        assert_eq!(p.visible().len(), 1);
    }

    #[test]
    fn a_match_deep_in_the_prompts_shows_its_words() {
        let mut p = Picker::default();
        let mut i = info(
            "ses_s",
            "what does this project do?",
            "what does this project do?",
            "ok",
        );
        i.prompts = Some("run the tests\nwhat do these log lines say went wrong?\nfix it".into());
        p.set(false, vec![i]);
        type_in(&mut p, "log lines");
        let v = p.visible();
        assert_eq!(v.len(), 1, "a prompt in the middle is searched");
        let words = vec!["log".to_string(), "lines".to_string()];
        let s = snippet(v[0], &words, 60).unwrap();
        assert!(s.contains("these log lines say"), "{s}");
    }

    #[test]
    fn a_thousand_sessions_filter_well_inside_a_frame() {
        let mut p = Picker::default();
        let many: Vec<SessionInfo> = (0..1000)
            .map(|n| {
                info(
                    &format!("ses_{n}"),
                    &format!("conversation number {n} about tides"),
                    &format!("first message {n} with some words in it for the search to read"),
                    &format!("the last reply {n} was long enough to have a few more words"),
                )
            })
            .collect();
        p.set(false, many);
        let t = std::time::Instant::now();
        for c in "reply 99 was".chars() {
            p.key(Action::Insert(c));
            assert!(!p.visible().is_empty() || c == 's');
        }
        let each = t.elapsed() / 12;
        assert!(each.as_millis() < 16, "{each:?} a keystroke");
    }
}
