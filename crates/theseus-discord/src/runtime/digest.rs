//! `/digest` (theseus-qkoi3): what finished, failed, was answered or woke in
//! this place since it last looked, ephemeral, and a welcome-back line in a
//! DM after four hours of quiet. A child of `runtime`, apart from it for the
//! shape budget's file ceiling.
//!
//! - **The words are the protocol's** (`digest_words`, `digest_summary`): the
//!   same lines `theseus digest`, the TUI and the cockpit say. The read is
//!   `work.digest` as this binding, naming the place's own session: the core
//!   judges it by the place rule (this binding reads no private text, so it
//!   is cut to the roots it names, scrubbed, and cannot answer).
//! - **Seen stays the client's.** Discord's "last read" is this process's:
//!   the position of the last digest shown to a place, by its session. After
//!   a restart the first `/digest` reaches back a day.
//! - **The welcome is the owner's typing, not a timer.** A DM's first typing
//!   of an idle spell (`typing.rs`) after `WELCOME_AFTER_MS` with no earlier
//!   activity in this process posts one line, when the digest is not empty.
//!   Nothing is posted on its own, so it is no unprompted post to pace.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use theseus_protocol::{
    digest_summary, digest_text, method, now_unix_ms, DiscordOrigin, WorkDigestParams,
    WorkDigestResult,
};
use tokio::sync::{mpsc, oneshot};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::{AllowedMentions, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::CommandBuilder;

use super::{typing, Control, Place, PlaceMsg, Shared};

/// Discord's limit on a message's text.
const LIMIT: usize = 2000;
/// With nothing read yet in this process, `/digest` reaches back this far.
const FIRST_MS: u64 = 24 * 3_600_000;
/// A DM quiet this long is welcomed back.
pub(super) const WELCOME_AFTER_MS: u64 = 4 * 3_600_000;

/// What a place last looked at, and when it was last active, by session.
#[derive(Debug, Default, Clone, Copy)]
struct Mark {
    position: u64,
    looked_ms: u64,
    active_ms: u64,
}

static MARKS: LazyLock<Mutex<HashMap<String, Mark>>> = LazyLock::new(Mutex::default);

fn mark_of(session: &str) -> Mark {
    MARKS
        .lock()
        .unwrap()
        .get(session)
        .copied()
        .unwrap_or_default()
}

fn update(session: &str, f: impl FnOnce(&mut Mark)) {
    f(MARKS
        .lock()
        .unwrap()
        .entry(session.to_string())
        .or_default());
}

/// `/digest`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "digest",
        "What finished, failed or was answered here since you last looked (only you see it)",
        CommandType::ChatInput,
    )
    .build()
}

/// The ephemeral answer's text: the summary, each line with its time, what
/// waits, within Discord's limit (the oldest lines that do not fit are left
/// out and counted).
pub(super) fn text(res: &WorkDigestResult, since_ms: u64, hm: &dyn Fn(u64) -> String) -> String {
    digest_text(res, since_ms, hm, LIMIT)
}

/// Whether a DM last active at `was_ms` (0: not seen in this process) is
/// quiet enough at `now` to be welcomed back.
pub(super) fn welcome_due(was_ms: u64, now: u64) -> bool {
    was_ms > 0 && now.saturating_sub(was_ms) >= WELCOME_AFTER_MS
}

impl Shared {
    /// An interaction of `/digest`: true when it was one, and answered.
    pub(super) async fn digest_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
    ) -> bool {
        let Some(InteractionData::ApplicationCommand(c)) = &i.data else {
            return false;
        };
        if c.name != "digest" {
            return false;
        }
        // Acknowledged at once, ephemeral: the flag on the deferral is what
        // the later edit keeps.
        let deferred = InteractionResponse {
            kind: InteractionResponseType::DeferredChannelMessageWithSource,
            data: Some(InteractionResponseData {
                flags: Some(MessageFlags::EPHEMERAL),
                allowed_mentions: Some(AllowedMentions::default()),
                ..Default::default()
            }),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &deferred)
            .await
        {
            self.board.error("digest response", None, e);
        }
        let (rtx, rrx) = oneshot::channel();
        let _ = tx.send(PlaceMsg::Control {
            cmd: Control::Digest,
            by: who.to_string(),
            reply: rtx,
        });
        let words = rrx
            .await
            .unwrap_or_else(|_| "The place did not answer.".into());
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .update_response(&i.token)
            .content(Some(&words))
            .allowed_mentions(Some(&AllowedMentions::default()))
            .await
        {
            self.board.error("digest reply", None, e);
        }
        true
    }

    /// The digest of `session_id`'s place since `mark`, as this binding.
    async fn digest_of(
        &self,
        session_id: &str,
        mark: Mark,
        now: u64,
    ) -> Result<WorkDigestResult, String> {
        let mut params = WorkDigestParams {
            roots: Some(vec![session_id.to_string()]),
            ..Default::default()
        };
        if mark.position > 0 {
            params.since = Some(mark.position);
        } else {
            params.since_ms = Some(now.saturating_sub(FIRST_MS));
        }
        self.rpc
            .call(method::WORK_DIGEST, params)
            .await
            .map_err(|e| e.message)
    }
}

impl Place {
    /// The two controls that only read the core: `/why` and `/digest`.
    pub(super) async fn read_only(&self, cmd: Control) -> String {
        match cmd {
            Control::Digest => self.digest().await,
            _ => self.why().await,
        }
    }

    /// `/digest`'s text for this place's session; the place has looked after.
    pub(super) async fn digest(&self) -> String {
        let now = now_unix_ms();
        let mark = mark_of(&self.session_id);
        let since_ms = if mark.looked_ms > 0 {
            mark.looked_ms
        } else {
            now.saturating_sub(FIRST_MS)
        };
        match self.shared.digest_of(&self.session_id, mark, now).await {
            Ok(res) => {
                update(&self.session_id, |m| {
                    m.position = res.to;
                    m.looked_ms = now;
                    m.active_ms = now;
                });
                text(&res, since_ms, &theseus_core::push::hm)
            }
            Err(e) => format!("⚠️ {e}"),
        }
    }

    /// The owner's typing in this place: the core is told (to warm what the
    /// message will wait for), and a DM quiet for `WELCOME_AFTER_MS` is
    /// welcomed back, once, when there is something to say.
    pub(super) fn typed(&mut self, origin: DiscordOrigin) {
        typing::notice(&self.shared.rpc, &self.session_id, origin);
        let now = now_unix_ms();
        let was = mark_of(&self.session_id);
        update(&self.session_id, |m| m.active_ms = now);
        let quiet = welcome_due(was.active_ms, now);
        if self.kind != "dm" || !quiet {
            return;
        }
        let shared = std::sync::Arc::clone(&self.shared);
        let (sid, target) = (self.session_id.clone(), self.target.clone());
        tokio::spawn(async move {
            let Ok(res) = shared.digest_of(&sid, was, now).await else {
                return;
            };
            let since = if was.looked_ms > 0 {
                was.looked_ms
            } else {
                was.active_ms
            };
            let Some(line) = digest_summary(&res.lines, since, &theseus_core::push::hm) else {
                return;
            };
            update(&sid, |m| {
                m.position = res.to;
                m.looked_ms = now;
            });
            let body = serde_json::json!({
                "kind": "notice",
                "text": format!("Welcome back. {line} · `/digest` for the lines"),
                "reply_to": null,
            });
            if let Err(e) = shared.core.outbox.post(&sid, "", &target, body) {
                shared
                    .board
                    .error("post welcome back", Some(&sid), format!("{e:#}"));
            }
        });
    }
}

#[cfg(test)]
#[path = "tests_digest.rs"]
mod tests;
