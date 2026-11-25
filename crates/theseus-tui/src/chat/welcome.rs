//! Since you last looked (theseus-xock5): the chat asks `work.digest` since the
//! store's look (issue 46), as the board and the CLI do. When it opens, and
//! when it comes back from a long-lost connection, it asks; it prints one line
//! of what happened when the look was an absence ago, never on a first start,
//! and a daemon that does not know the method costs no word. Then the store's
//! look moves to the digest's end (`work.look`), and again on leaving and at
//! most once a minute while the board moves. The machine's seen file says what
//! the operator has already been shown, so a question seen elsewhere does not
//! ring here.

use serde_json::{json, Value};
use theseus_client::seen;
use theseus_protocol::notices::{Notice, Viewer};
use theseus_protocol::work::WorkKind;
use theseus_protocol::{method, WorkDigestResult, WorkLookResult};

use super::state::{call, Chat, Effect, Purpose};
use super::style::Sty;

/// An absence this long is told: a restart or a glance away is not.
pub const AWAY_MS: u64 = 60_000;

/// While the board moves, a look is sent at most this often: every look is a
/// record in the store.
pub const LOOK_EVERY_MS: u64 = 60_000;

impl Chat {
    /// The digest for a chat that just opened: a chat with no seen file is no
    /// terminal of the owner's, and asks none.
    pub(super) fn welcome_call(&self) -> Option<Effect> {
        self.cfg.seen_path.as_ref()?;
        Some(self.digest_call())
    }

    /// The digest for a chat whose connection came back after `away` ms: what
    /// the board did since the store's look.
    pub(super) fn reconnect_call(&self, away: u64) -> Option<Effect> {
        (self.board.position > 0 && away >= AWAY_MS).then(|| self.digest_call())
    }

    /// The digest came: its one line goes in the transcript (after the
    /// banner, which prints once) when the look was an absence ago, and the
    /// store's look moves to its end.
    pub(super) fn welcome_said(&mut self, v: Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<WorkDigestResult>(v) else {
            return Vec::new();
        };
        let old = self.old_look();
        let since = r.look.map(|l| l.at_ms).or(old).unwrap_or(0);
        self.looked_ms = self.looked_ms.max(since);
        let hm = theseus_client::render::time::fmt_hm;
        let away = self.now_ms.saturating_sub(since);
        let told = (since > 0 && away >= AWAY_MS)
            .then(|| theseus_protocol::digest_summary(&r.lines, since, &hm))
            .flatten();
        if let Some(line) = told {
            self.welcome = Some(format!("{line} · theseus digest"));
            if self.banner.printed {
                self.print_welcome();
            }
        }
        self.board.position = self.board.position.max(r.to);
        let mut out: Vec<Effect> = self.look(r.to).into_iter().collect();
        if let (Some(path), Some(_)) = (&self.cfg.seen_path, old) {
            out.push(Effect::ForgetOldLook(path.clone()));
        }
        out
    }

    /// The welcome line, once.
    pub(super) fn print_welcome(&mut self) {
        if let Some(line) = self.welcome.take() {
            self.say_plain(Sty::Green, &line);
        }
    }

    /// `work.look` at `position`, unless it was sent already: the operator is
    /// looking at the board. None for a chat with no seen file.
    pub(super) fn look(&mut self, position: u64) -> Option<Effect> {
        self.cfg.seen_path.as_ref()?;
        if position == 0 || position <= self.look_sent.0 {
            return None;
        }
        self.look_sent = (position, self.now_ms);
        Some(call(
            method::WORK_LOOK,
            json!({ "position": position }),
            Purpose::Look,
        ))
    }

    /// A look due: the board moved, and the last was a minute ago.
    pub(super) fn look_due(&mut self) -> Option<Effect> {
        if self.now_ms < self.look_sent.1 + LOOK_EVERY_MS {
            return None;
        }
        self.look(self.board.position)
    }

    /// The store's look as `work.look` left it, which the seen file keeps.
    pub(super) fn look_said(&mut self, v: &Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<WorkLookResult>(v.clone()) else {
            return Vec::new();
        };
        self.looked_ms = self.looked_ms.max(r.look.at_ms);
        self.cfg
            .seen_path
            .clone()
            .map(|p| Effect::StoreLook(p, r.look))
            .into_iter()
            .collect()
    }

    /// The board position this chat last applied.
    pub(super) fn board_position(&self) -> u64 {
        self.board.position
    }

    /// The time of the look an older build kept in the seen file, carried over
    /// once (issue 46).
    fn old_look(&self) -> Option<u64> {
        seen::old_look_time(self.cfg.seen_path.as_deref()?)
    }

    fn digest_call(&self) -> Effect {
        call(
            method::WORK_DIGEST,
            json!({ "since_look": true, "since_ms": self.old_look().unwrap_or(self.now_ms) }),
            Purpose::Welcome,
        )
    }

    /// Who a notice would reach: what the operator was last shown of the
    /// work, from the file, and how long they have been away. Away is
    /// the longer of the quiet in this terminal and the time since the
    /// store's look as this chat last heard it.
    pub(super) fn viewer(&self, n: &Notice) -> Viewer {
        let path = self.cfg.seen_path.as_deref();
        let shown = path.map(seen::displayed).unwrap_or_default();
        let execution = self
            .board
            .views
            .get(&n.work.id)
            .map(|v| match (v.kind, v.parent.as_deref()) {
                (WorkKind::Question, Some(p)) => p.to_string(),
                _ => v.id.clone(),
            })
            .unwrap_or_else(|| n.work.id.clone());
        let last = self.looked_ms.max(self.last_input_ms);
        let quiet = self.now_ms.saturating_sub(last);
        Viewer {
            focused: self.session.clone(),
            seen: shown.get(&execution).copied().unwrap_or(0),
            away_ms: if quiet >= AWAY_MS { quiet } else { 0 },
            quiet: false,
            sound: true,
        }
    }
}
