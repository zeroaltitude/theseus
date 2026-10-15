//! Coming back to a conversation (theseus-6yka): `theseus -c` continues this
//! directory's last, `theseus -r` picks one in the dock, and either way the
//! last [`REPLAY_TURNS`] turns are replayed above the dock through the chat's
//! own transcript lines, the older turns folded into one line that `/history`
//! answers. No I/O here: each step returns the requests to send.

use serde_json::{json, Value};
use theseus_protocol::{
    method, SessionHistoryResult, SessionInfo, SessionKind, SessionListResult, SessionState,
};

use super::picker::{Outcome, Picker};
use super::state::{call, Chat, Effect, Purpose, Start};
use super::style::Sty;

/// Turns replayed on reopening; the rest are folded into one line.
pub const REPLAY_TURNS: usize = 30;
/// Nodes asked for to find them: a turn with tools is many nodes.
const REPLAY_NODES: usize = 500;
/// Sessions asked for when looking for the last here, and for the picker.
const CONTINUE_N: usize = 50;
const PICK_N: usize = 200;
const WIDE_N: usize = 1000;

/// Where the replay starts in `h.nodes`, and how many earlier turns it
/// leaves out: the page begins at the `keep`th user message from its end (or
/// at its first node), and the session's own count of turns says how many
/// remain above that.
pub fn window(h: &SessionHistoryResult, keep: usize) -> (usize, u64) {
    let starts: Vec<usize> = h
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind == "user_message")
        .map(|(i, _)| i)
        .collect();
    let from = if starts.len() > keep {
        starts[starts.len() - keep]
    } else {
        0
    };
    let shown = starts.len().min(keep) as u64;
    let folded = if from > 0 || h.older.is_some() || (h.nodes.len() >= REPLAY_NODES) {
        h.session.turns.saturating_sub(shown)
    } else {
        0
    };
    (from, folded)
}

/// A message longer than this many lines is shown folded: its first lines,
/// one line that counts the rest, and its last lines (the question that
/// follows a long paste is the last).
const FOLD_OVER: usize = 6;
const FOLD_KEEP: usize = 2;

/// A past message's lines for the transcript, a long one folded.
pub fn folded(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= FOLD_OVER {
        return lines.iter().map(|l| (*l).to_string()).collect();
    }
    let hidden = lines.len() - 2 * FOLD_KEEP;
    let mut out: Vec<String> = lines[..FOLD_KEEP]
        .iter()
        .map(|l| (*l).to_string())
        .collect();
    out.push(format!("… {hidden} more lines"));
    out.extend(
        lines[lines.len() - FOLD_KEEP..]
            .iter()
            .map(|l| (*l).to_string()),
    );
    out
}

/// This directory's last conversation to continue: a person's own (not a
/// task's, and not one that speaks to a place on Discord), with a turn, not
/// retired, the most recently active.
pub fn last_here(sessions: &[SessionInfo]) -> Option<&SessionInfo> {
    sessions
        .iter()
        .filter(|s| {
            s.kind == SessionKind::Conversation
                && s.started_on.as_deref() != Some("discord")
                && s.turns > 0
                && s.dir.is_some()
                && s.state != Some(SessionState::Retired)
        })
        .max_by_key(|s| s.last_active_ms)
}

impl Chat {
    /// The first questions of `-c` and `-r`: this directory's sessions.
    pub(super) fn start_listing(&mut self) -> Vec<Effect> {
        // There is no banner "last here" to wait for: the list is the start.
        self.banner.last = Some(None);
        let pick = self.cfg.start == Start::Pick;
        let mut params =
            json!({ "dir": self.cfg.dir, "n": if pick { PICK_N } else { CONTINUE_N } });
        if pick {
            self.picker = Some(Picker::default());
            params["with_text"] = json!(true);
        }
        let purpose = if pick {
            Purpose::ListPick
        } else {
            Purpose::ListContinue
        };
        vec![call(method::SESSION_LIST, params, purpose)]
    }

    /// A new conversation here: what bare `theseus` does at its start.
    pub(super) fn open_new(&mut self) -> Vec<Effect> {
        self.banner.last = Some(None);
        self.banner_ready();
        vec![call(
            method::SESSION_OPEN,
            json!({ "dir": self.cfg.dir }),
            Purpose::Open,
        )]
    }

    /// `-c`'s answer: continue the last one here, or say there is none and
    /// open a new one.
    pub(super) fn continue_here(&mut self, v: Value) -> Vec<Effect> {
        let sessions = serde_json::from_value::<SessionListResult>(v)
            .map(|l| l.sessions)
            .unwrap_or_default();
        match last_here(&sessions).cloned() {
            Some(info) => self.resume_info(info),
            None => {
                self.print_banner();
                self.say_plain(Sty::Dim, "no conversation here yet · starting a new one");
                self.open_new()
            }
        }
    }

    /// Go on with this conversation: watch it, and replay its last turns.
    pub(super) fn resume_info(&mut self, info: SessionInfo) -> Vec<Effect> {
        self.profile = info.profile.clone();
        self.model = self.model.take().or(info.model.clone());
        self.cost = info.cost_usd;
        self.execution = info.execution_id.clone();
        let sid = info.session_id;
        self.banner.last = Some(None);
        let mut out = self.opened(&sid);
        out.insert(
            0,
            call(
                method::SESSION_HISTORY,
                json!({ "session_id": sid, "n": REPLAY_NODES }),
                Purpose::History(sid.clone()),
            ),
        );
        self.banner_ready();
        out
    }

    /// The picker's list, this directory's or every directory's.
    pub(super) fn listed(&mut self, wide: bool, v: Value) -> Vec<Effect> {
        let sessions = serde_json::from_value::<SessionListResult>(v)
            .map(|l| l.sessions)
            .unwrap_or_default();
        if let Some(p) = self.picker.as_mut() {
            p.set(wide, sessions);
        }
        self.banner_ready();
        Vec::new()
    }

    /// A key while the picker is open.
    pub(super) fn picker_key(&mut self, action: super::keys::Action) -> Vec<Effect> {
        let Some(p) = self.picker.as_mut() else {
            return Vec::new();
        };
        match p.key(action) {
            Outcome::Idle => Vec::new(),
            Outcome::Pick(info) => {
                self.picker = None;
                self.resume_info(*info)
            }
            Outcome::Widen => vec![call(
                method::SESSION_LIST,
                json!({ "n": WIDE_N, "with_text": true }),
                Purpose::ListWide,
            )],
            Outcome::Leave => {
                self.picker = None;
                self.leave()
            }
            Outcome::Cancel => {
                self.picker = None;
                self.print_banner();
                self.open_new()
            }
        }
    }

    /// A paste into the picker's query: its first line.
    pub(super) fn picker_paste(&mut self, text: &str) -> Vec<Effect> {
        if let Some(p) = self.picker.as_mut() {
            for c in text.lines().next().unwrap_or("").chars() {
                p.key(super::keys::Action::Insert(c));
            }
        }
        Vec::new()
    }

    /// `/history`: print the turns the replay folded, one page back at a time.
    pub(super) fn earlier_cmd(&mut self, _args: &str) -> Vec<Effect> {
        let (Some(sid), Some(before)) = (self.session.clone(), self.earlier) else {
            self.flash("no earlier turns to show");
            return Vec::new();
        };
        vec![call(
            method::SESSION_HISTORY,
            json!({ "session_id": sid, "before": before, "n": REPLAY_NODES }),
            Purpose::Earlier(sid),
        )]
    }

    /// The turns `/history` asked for, from their first user message.
    pub(super) fn earlier_turns(&mut self, h: &SessionHistoryResult) {
        let from = h
            .nodes
            .iter()
            .position(|n| n.kind == "user_message")
            .unwrap_or(0);
        self.earlier = h.older.and_then(|_| h.nodes.get(from).map(|n| n.position));
        let turns = h.nodes[from..]
            .iter()
            .filter(|n| n.kind == "user_message")
            .count();
        self.gap();
        self.say_plain(
            Sty::Dim,
            &format!(
                "── {turns} earlier turn{} ──",
                if turns == 1 { "" } else { "s" }
            ),
        );
        for n in &h.nodes[from..] {
            self.node_line(n);
        }
        self.say_plain(
            Sty::Dim,
            &if self.earlier.is_some() {
                "── still earlier turns · /history again ──".to_string()
            } else {
                "── the start of the conversation ──".to_string()
            },
        );
        self.gap();
    }
}
