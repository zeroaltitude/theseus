//! Since you last looked (theseus-xock5): the chat reads the machine's seen
//! file, as the board and the CLI do. When it opens after an absence, or
//! comes back from a lost connection, it asks `work.digest` since the last
//! look and prints one line of what happened; it never asks on a first start,
//! and a daemon that does not know the method costs no word. The file also
//! says what the operator has already been shown, so a question seen
//! elsewhere does not ring here, and how long they have been away.

use serde_json::{json, Value};
use theseus_client::seen;
use theseus_protocol::notices::{Notice, Viewer};
use theseus_protocol::work::WorkKind;
use theseus_protocol::{method, WorkDigestResult};

use super::state::{call, Chat, Effect, Purpose};
use super::style::Sty;

/// An absence this long is told: a restart or a glance away is not.
pub const AWAY_MS: u64 = 60_000;

impl Chat {
    /// The digest for a chat that just opened: none on a first start (the file
    /// holds no look), nor when the last look was a moment ago.
    pub(super) fn welcome_call(&self) -> Option<Effect> {
        let looked = seen::looked(self.cfg.seen_path.as_deref()?);
        let away = self.now_ms.saturating_sub(looked.at_ms);
        (looked.position > 0 && away >= AWAY_MS).then(|| digest_call(looked.position))
    }

    /// The digest for a chat whose connection came back after `away` ms: what
    /// the board did since the last position this chat knew.
    pub(super) fn reconnect_call(&self, away: u64) -> Option<Effect> {
        (self.board.position > 0 && away >= AWAY_MS).then(|| digest_call(self.board.position))
    }

    /// The digest came: its one line goes in the transcript (after the
    /// banner, which prints once), and the look moves to its end.
    pub(super) fn welcome_said(&mut self, v: Value) -> Vec<Effect> {
        let Ok(r) = serde_json::from_value::<WorkDigestResult>(v) else {
            return Vec::new();
        };
        let since = self
            .cfg
            .seen_path
            .as_deref()
            .map_or(0, |p| seen::looked(p).at_ms);
        let hm = theseus_client::render::time::fmt_hm;
        if let Some(line) = theseus_protocol::digest_summary(&r.lines, since, &hm) {
            self.welcome = Some(format!("{line} · theseus digest"));
            if self.banner.printed {
                self.print_welcome();
            }
        }
        self.board.position = self.board.position.max(r.to);
        self.look(r.to)
    }

    /// The welcome line, once.
    pub(super) fn print_welcome(&mut self) {
        if let Some(line) = self.welcome.take() {
            self.say_plain(Sty::Green, &line);
        }
    }

    /// The operator is looking at the board now, at `position`: the file
    /// follows, off the loop's thread.
    pub(super) fn look(&self, position: u64) -> Vec<Effect> {
        match &self.cfg.seen_path {
            Some(path) if position > 0 => {
                vec![Effect::Look(path.clone(), position, self.now_ms)]
            }
            _ => Vec::new(),
        }
    }

    /// The board position this chat last applied.
    pub(super) fn board_position(&self) -> u64 {
        self.board.position
    }

    /// Who a notice would reach: what the operator was last shown of the
    /// work, from the file, and how long they have been away. Away is
    /// the longer of the quiet in this terminal and the time since any
    /// client's last look, which a recent look elsewhere shortens.
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
        let last = path
            .map_or(0, |p| seen::looked(p).at_ms)
            .max(self.last_input_ms);
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

fn digest_call(since: u64) -> Effect {
    call(
        method::WORK_DIGEST,
        json!({ "since": since }),
        Purpose::Welcome,
    )
}
