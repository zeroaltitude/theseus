//! The owner's notices place for the operator lane (theseus-nuqut): the
//! announcement place, `theseus-notify`, bound by `notify = true` in the
//! bindings file or `notify here` said in a private place. It takes the
//! announcements (`theseus_core::outbox::announces`) and every FYI note
//! (`owner_note::Kind::Fyi`) of a kind on [`INFORMING`]; everything that
//! needs the owner keeps his DM. With none bound, every post keeps the DM.
//!
//! The DM never waits on the notices place:
//! - an FYI whose write there Discord refuses for good (the channel is gone,
//!   or the bot may not post there) is written to the DM once instead, unless
//!   an earlier send of it may have landed, which is never sent again;
//! - an FYI whose write there meets an absent or busy Discord waits aside
//!   ([`State`]), with the FYIs for that place after it, and the lane goes on
//!   with the posts after them; they are tried again on their own backoff.

use std::time::Duration;

use serde_json::{json, Value};
use theseus_core::outbox::kind_of as post_kind;
use theseus_core::owner_note::{kind_of, Kind};
use theseus_kernel::Action;

use super::{Lane, SendErr, Write, BACKOFF_MAX};

/// The post kinds whose producers write an FYI that answers no conversation:
/// the ontology's mint, Jev's pause, a daemon notice, a restart, the disk
/// coming back, an MCP change, the self notes of a join, a note of an
/// act done unasked with its Undo (issue 57), and the injection flag's
/// notice under `notify`, which refuses nothing (issue 54). A post of any
/// other kind keeps the DM whatever its words say.
pub(super) const INFORMING: &[&str] = &[
    "onto_card",
    "jev_paused",
    "notice",
    "restarted",
    "disk",
    "mcp_changed",
    "mcp_prompt_changed",
    "self_join",
    "told",
    "content_flag",
];

/// Whether a post of `kind` whose finished words are `content` only informs
/// the owner: its kind is on [`INFORMING`] and its head says FYI.
pub(super) fn informs(kind: &str, content: &str) -> bool {
    INFORMING.contains(&kind) && kind_of(content) == Some(Kind::Fyi)
}

/// What the lane keeps about the notices place between posts.
#[derive(Default)]
pub(super) struct State {
    /// Set while a post is planned whose write goes to the notices place as
    /// an FYI: the DM it would have had.
    pub(super) dm: Option<(u64, String)>,
    /// The FYIs waiting aside, oldest first, by correlation id.
    parked: Vec<String>,
    /// When the waiting FYIs are tried again.
    until: Option<tokio::time::Instant>,
    attempt: u32,
}

impl State {
    /// When the lane wakes for the waiting FYIs.
    pub(super) fn wake(&self) -> Option<tokio::time::Instant> {
        self.until.filter(|_| !self.parked.is_empty())
    }

    /// Whether the notices place is waited out now: FYIs for it go aside
    /// untried.
    fn waits(&self) -> bool {
        self.until
            .is_some_and(|at| tokio::time::Instant::now() < at)
    }
}

impl Lane {
    /// The notices place, when one is bound, else `dm`: the owner's own word
    /// first (`notify here`), then the bindings file's `notify = true`. A
    /// `notify off` keeps `dm`, as does a daemon that binds none.
    pub(super) fn notices_place(&self, dm: (u64, String)) -> (u64, String) {
        if let Some(c) = self.shared.notify_said() {
            return (c, format!("discord:channel:{c}"));
        }
        if self.shared.notify_dm() {
            return dm;
        }
        self.shared.place_bits.notify_place().unwrap_or(dm)
    }

    /// Whether the notices place, when one is bound instead of the place `here`, is private by the
    /// rule a press is judged by: there, the owner's press counts. None bound is private enough:
    /// the post stays `here`.
    pub(super) fn notices_private(&self, here: u64, target: &str) -> bool {
        let to = self.notices_place((here, target.to_string()));
        // The place's own target, by its channel: the second half is a label for some sources.
        to.0 == here || self.shared.is_private(&format!("discord:channel:{}", to.0))
    }

    /// Where an operator post whose words are `content` goes: the notices
    /// place for an announcement, as [`Lane::announced`] says, and for an FYI
    /// that [`informs`]; `dm` for everything else. An FYI whose place is not
    /// one this daemon binds goes to `dm`. An FYI sent to the notices place
    /// leaves `dm` with the lane, for [`Lane::refused_to_dm`].
    pub(super) fn informed(
        &mut self,
        kind: &str,
        body: &Value,
        content: &str,
        dm: (u64, String),
    ) -> (u64, String) {
        if theseus_core::outbox::announces(body) || !informs(kind, content) {
            return self.announced(body, dm);
        }
        let to = self.notices_place(dm.clone());
        if to.0 == dm.0 {
            return to;
        }
        if self.shared.binds(&format!("discord:channel:{}", to.0)) {
            self.notices.dm = Some(dm);
            return to;
        }
        tracing::info!(
            kind,
            place = %to.1,
            "an FYI's notices place is not one this daemon binds: it goes to the owner's DM"
        );
        dm
    }

    /// The next post to deliver: the oldest open one, but past the FYIs
    /// waiting aside until their wait is over.
    pub(super) fn next_post(&mut self) -> Option<Action> {
        let outbox = &self.shared.core.outbox;
        if self.notices.parked.is_empty() {
            return outbox.next_for(&self.target);
        }
        if !self.notices.waits() {
            // Their turn again, in order: the oldest is tried first.
            self.notices.parked.clear();
            return outbox.next_for(&self.target);
        }
        let parked = &self.notices.parked;
        outbox
            .open_for(&self.target)
            .into_iter()
            .find(|a| !parked.contains(&a.correlation_id))
    }

    /// Take what planning `a` left: `Some(Some(dm))` when its write goes to
    /// the notices place as an FYI, `Some(None)` for any other post. While
    /// that place is waited out, an FYI for it goes aside untried and `None`
    /// comes back.
    pub(super) fn take_notices_dm(&mut self, a: &Action) -> Option<Option<(u64, String)>> {
        let dm = self.notices.dm.take();
        if dm.is_some() && self.notices.waits() {
            self.notices.parked.push(a.correlation_id.clone());
            return None;
        }
        Some(dm)
    }

    /// What a write `w` of post `a` came to, once the notices place is
    /// counted: `None` when an FYI's write there met an absent or busy
    /// Discord, and it waits aside ([`Lane::park`]); the DM's write when the
    /// place refused a new FYI for good and no earlier send of it may have
    /// landed (`notices` is then emptied, and `extra` names the DM);
    /// otherwise `wrote` itself.
    pub(super) async fn through_notices(
        &mut self,
        a: &Action,
        w: &Write,
        wrote: Result<Option<(u64, u64)>, SendErr>,
        (notices, maybe_landed, extra): (&mut Option<(u64, String)>, bool, &mut Value),
    ) -> Option<Result<Option<(u64, u64)>, SendErr>> {
        let (Err(e), Some(dm)) = (&wrote, notices.clone()) else {
            return Some(wrote);
        };
        if e.away {
            self.park(a, e);
            return None;
        }
        let create = w.message.is_none() && !self.msgs.contains_key(&w.key);
        if !create || maybe_landed {
            return Some(wrote);
        }
        let to_dm = self.refused_to_dm(a, w, &dm, e).await;
        extra["place"] = json!(dm.1);
        extra["notices_refused"] = json!(true);
        *notices = None;
        Some(to_dm)
    }

    /// An FYI's write to the notices place met an absent or busy Discord: it
    /// waits aside, dispatched, and is tried again after a backoff of its own,
    /// while the posts behind it go on.
    pub(super) fn park(&mut self, a: &Action, e: &SendErr) {
        let n = &mut self.notices;
        n.attempt += 1;
        let delay = Duration::from_secs(1 << n.attempt.min(6)).min(BACKOFF_MAX);
        n.until = Some(tokio::time::Instant::now() + delay);
        n.parked.push(a.correlation_id.clone());
        if e.unsure {
            self.unsure.insert(a.correlation_id.clone());
        }
        self.shared.core.outbox.error("discord", e.message.clone());
        if self.notices.attempt == 1 {
            tracing::warn!(
                kind = post_kind(a),
                waiting = self.notices.parked.len(),
                error = %e.message,
                "the notices place did not take an FYI: it waits, and the DM's posts go on"
            );
        }
    }

    /// An FYI's write reached the notices place.
    pub(super) fn notices_ok(&mut self) {
        self.notices.attempt = 0;
        self.notices.until = None;
    }

    /// The notices place refused a new FYI's write for good: the same write
    /// goes to the owner's DM, once. One log line says so.
    pub(super) async fn refused_to_dm(
        &mut self,
        a: &Action,
        w: &Write,
        dm: &(u64, String),
        e: &SendErr,
    ) -> Result<Option<(u64, u64)>, SendErr> {
        tracing::info!(
            kind = post_kind(a),
            error = %e.message,
            "the notices place refused an FYI: it goes to the owner's DM"
        );
        let mut to_dm = w.clone();
        to_dm.channel = dm.0;
        self.write(&to_dm).await
    }
}

#[cfg(test)]
mod tests {
    use theseus_core::owner_note::{note, Kind};

    use super::{informs, INFORMING};

    #[test]
    fn only_an_fyi_of_an_informing_kind_informs() {
        let fyi = note(Kind::Fyi, "Theseus started keeping track", "a word", "x");
        for k in INFORMING {
            assert!(informs(k, &fyi), "{k}");
        }
        // Never these, whatever their words say: each can ask the owner.
        for k in [
            "standing_card",
            "card_question",
            "retraction",
            "persona_soul",
            "hands_alert",
            "spend_ceiling",
            "self_keel",
            "self_narrow",
            "self_narrow_red",
            "self_halt",
            "seal_card",
            "link_card",
            "privacy_warning",
            "home_size",
            "needs_you",
            "jev_notice",
            "told_dm",
            "",
        ] {
            assert!(!informs(k, &fyi), "{k}");
        }
        for k in [Kind::Action, Kind::Decision, Kind::Alert, Kind::Reminder] {
            assert!(!informs("notice", &note(k, "T", "why", "")), "{k:?}");
        }
        assert!(!informs("notice", "plain words with no head"));
    }
}
