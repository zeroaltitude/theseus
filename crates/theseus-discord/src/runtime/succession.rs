//! A place's session (theseus-emqx): no session before its first message,
//! and every move recorded both ways.
//!
//! A place resumes on its stored session when that one exists, can take
//! turns, and is not retired. Otherwise it starts *fresh*: nothing is opened
//! at its bind, at `/new`, or when its session can no longer take turns, and
//! its next message opens the session it runs on (`Place::ready`), through
//! `Core::bind_place_to`, which writes the place's record and the
//! supersession (`superseded_by` on the old session, `supersedes` on the new
//! one, the `session.superseded` row) in one frame. So `superseded_by` always
//! names a session that took a message, and no place leaves an empty
//! session behind. A session the owner retired (`session.retire`) is read
//! before each turn (one key): its place's next message starts a successor
//! the same way, so a place never posts into a retired session silently.

use serde_json::Value;
use theseus_protocol::{LedgerKind, SessionInfo, SessionKind, SessionOpenParams, SessionRef};

use super::{terminal, LaneMsg, Place, Shared};

/// The cores whose places open their session at the bind, as before
/// theseus-emqx: the outbox's and the live file's tests submit to a place's
/// session by hand, before any message. Tests only; keyed by the core, so
/// a test beside them in one process is unaffected.
#[cfg(test)]
static OPEN_AT_BIND: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn named_by_test(core: &std::sync::Arc<theseus_core::Core>) -> bool {
    OPEN_AT_BIND
        .lock()
        .unwrap()
        .contains(&(std::sync::Arc::as_ptr(core) as usize))
}

#[cfg(not(test))]
fn named_by_test(_: &std::sync::Arc<theseus_core::Core>) -> bool {
    false
}

/// Places bound on `core` open their session at the bind (tests only).
#[cfg(test)]
pub(crate) fn open_at_bind(core: &std::sync::Arc<theseus_core::Core>) {
    OPEN_AT_BIND
        .lock()
        .unwrap()
        .push(std::sync::Arc::as_ptr(core) as usize);
}

/// A place's mark that it was bound once (`Shared::first_bind`).
const BOUND_META_PREFIX: &str = "discord.bound.";

impl Shared {
    /// The stored session a place resumes on, if it exists, can take
    /// turns and is not retired; none, and the place starts fresh. With it,
    /// whether the place says its bind notice: when it starts fresh.
    pub(super) async fn resumable(
        &self,
        key: &str,
        label: &str,
    ) -> anyhow::Result<(Option<String>, bool)> {
        if let Some(sid) = self.core.outbox.place_session(key)? {
            if let Some(info) = self.session_info(&sid).await {
                // Retired by hand or superseded; an empty one is used.
                let retired = info
                    .retired
                    .is_some_and(|r| r.reason != theseus_protocol::RetiredReason::Empty);
                if !terminal(info.execution_state.as_deref()) && !retired {
                    self.watch(&sid).await;
                    self.place_limit(key, label, &sid);
                    return Ok((Some(sid), false));
                }
            }
        }
        if self.open_at_bind() {
            return Ok((Some(self.open_session(key, label).await?), true));
        }
        Ok((None, self.first_bind(key)?))
    }

    /// Whether this is the place's first bind, marked once (META
    /// `discord.bound.<place>`): a place that starts fresh again (a restart,
    /// or put back in the file live, before its first message) says its bind
    /// notice only the first time, as a place that resumed its session never
    /// said it again.
    fn first_bind(&self, key: &str) -> anyhow::Result<bool> {
        let mark = format!("{BOUND_META_PREFIX}{key}");
        if self.core.store.get_meta::<bool>(&mark)?.is_some() {
            return Ok(false);
        }
        self.core.store.put_meta(&mark, &true)?;
        Ok(true)
    }

    /// Whether this binding's places open their session at the bind, as
    /// before theseus-emqx: for tests that submit to a place's session by
    /// hand, before any message. The crate's own tests name their core
    /// (`open_at_bind`); a debug build's daemon takes the plant
    /// `THESEUS_TEST_OPEN_AT_BIND` (theseusd's outbox, tasks and wakes
    /// tests). A release build has no such plant.
    fn open_at_bind(&self) -> bool {
        named_by_test(&self.core)
            || cfg!(debug_assertions) && std::env::var_os("THESEUS_TEST_OPEN_AT_BIND").is_some()
    }

    /// The session a place's first message runs on: opened, bound to the
    /// place with the move recorded (`Core::bind_place_to`), given the
    /// place's limit, and watched.
    pub(super) async fn open_session(&self, key: &str, label: &str) -> anyhow::Result<String> {
        let info: SessionInfo = self
            .rpc
            .call(
                theseus_protocol::method::SESSION_OPEN,
                SessionOpenParams {
                    kind: Some(SessionKind::Conversation),
                    label: Some(format!("discord {label}")),
                    opened_from: None,
                },
            )
            .await?;
        // The place's record, the supersession, and where the session's
        // posts go from now on.
        self.core.bind_place_to(key, &info.session_id)?;
        self.core.binding_ledger(
            LedgerKind::DiscordBound,
            Some(&info.session_id),
            self.bound_row(key, label),
        );
        self.place_limit(key, label, &info.session_id);
        self.watch(&info.session_id).await;
        Ok(info.session_id)
    }
}

impl Place {
    /// The place's session is ready for a turn: its own, or, when it starts
    /// fresh or its session was retired, a new one opened now. False, with
    /// a notice, when none could be opened.
    pub(super) async fn ready(&mut self) -> bool {
        if !self.fresh && !self.shared.core.session_retired(&self.session_id) {
            return true;
        }
        match self.shared.open_session(&self.key, &self.label).await {
            Ok(sid) => {
                self.adopt(sid).await;
                true
            }
            Err(e) => {
                self.say(&format!("⚠️ Could not open a session here: {e:#}"), None);
                false
            }
        }
    }

    /// After a turn: the messages queued behind it go as the next turn, then
    /// a voice turn that waited, each on a session that is ready for it.
    pub(super) async fn next_after_turn(&mut self) {
        let batch = std::mem::take(&mut self.queued);
        if !batch.is_empty() && self.ready().await {
            self.submit(batch);
        }
        if self.inflight || !self.voice_waits() || self.ready().await {
            self.voice_next();
        }
        self.report();
    }

    /// A voice turn of this place's call, on a session that is ready for it.
    pub(super) async fn voice_when_ready(&mut self, t: super::voice::VoiceTurn) {
        if self.ready().await {
            self.voice_turn(t);
        }
    }

    /// The place's session can take no more turns: it says so, and its next
    /// message opens a fresh one.
    pub(super) fn cannot_take_turns(&mut self, class: &str) {
        self.say(
            &format!(
                "This place's session can't take turns any more ({class}): a fresh one starts with \
                 your next message. The old one stays in the web UI."
            ),
            None,
        );
        self.start_fresh();
    }

    /// The place's next message opens a fresh session (`/new`, or a session
    /// that can take no more turns).
    pub(super) fn start_fresh(&mut self) {
        self.fresh = true;
    }

    /// Point this place at `sid`, the session just opened for it.
    async fn adopt(&mut self, sid: String) {
        let old = std::mem::replace(&mut self.session_id, sid.clone());
        self.fresh = false;
        {
            let mut r = self.shared.routes.lock().unwrap();
            if !old.is_empty() {
                r.by_session.remove(&old);
            }
            r.by_session.insert(sid, self.tx.clone());
        }
        if !old.is_empty() {
            self.shared.board.unplace(&self.label, &old);
            let _ = self
                .shared
                .rpc
                .call::<_, Value>(
                    theseus_protocol::method::SESSION_UNWATCH,
                    SessionRef { session_id: old },
                )
                .await;
        }
        // The old session's turns are dropped with its renderer: the lane
        // forgets their messages (theseus-6809).
        self.renderer = self.shared.renderer();
        let _ = self.lane.send(LaneMsg::Held(self.renderer.held()));
        self.report();
    }

    /// What `/new` answers: nothing is opened until the next message.
    pub(super) fn new_answer(&self) -> String {
        if self.session_id.is_empty() {
            return "🆕 A fresh session starts here with your next message.".into();
        }
        format!(
            "🆕 A fresh session starts here with your next message. The previous one, `{}`, \
             stays in the cockpit, marked replaced once the new one opens.",
            self.session_id
        )
    }
}

/// What a place says when it is bound and starts fresh: how to talk, and
/// each control with its one effect (W1: `/stop` keeps the conversation).
pub(super) fn bind_notice(mention_only: bool) -> String {
    let how = if mention_only {
        "@mention me or reply to one of my messages to talk"
    } else {
        "Talk to me in this place"
    };
    format!(
        "🔗 Theseus is bound here; a fresh session starts with your next message. {how}. `/stop` \
         halts what I am doing and keeps the conversation, `/new` starts a fresh one, `/trust` \
         trusts the conversation again after it reads web text, and `/status`, `/tasks` and \
         `/wakes` show this place's; everything shows in the web UI."
    )
}
