//! The one message a persona added without a soul draws (theseus-utfnl): it
//! asks the owner for the soul, once. The words are the owner's note head
//! (`owner_note`), the action last; on Discord the card has two buttons,
//! "Write its soul" (a modal) and "Decline" (retires the persona), which the
//! binding's `persona_soul` handler answers. The press is `persona.soul` or
//! `persona.retire` as the presser: the core judges it, so only the owner, from
//! a private place, can write a soul.

use serde_json::json;

use crate::owner_note::{note, Kind};
use crate::rpc::Core;

/// The note's text for persona `key` named `name`, made `by` ("the CLI").
pub fn text(key: &str, name: &str, by: &str, cap: u64) -> String {
    note(
        Kind::Action,
        &format!("New persona: {name}"),
        &format!("you added it from {by}"),
        &format!(
            "It waits for its soul: a few sentences on its voice, its temperament, what it cares \
             about and what it is for, and an emoji. Keep it short (about {} tokens at most). \
             Until then it is listed and never picked.\nWrite it: theseus persona soul {key}",
            cap
        ),
    )
}

impl Core {
    /// Ask the owner for a new persona's soul. Once: a persona is added once.
    /// A failure is warned, never the add's.
    pub(crate) fn persona_soul_note(&self, key: &str, name: &str, by: &str) {
        let body = json!({"kind": "persona_soul", "persona": key, "name": name,
                          "text": text(key, name, by, self.cfg.context.persona_cap())});
        let sent = self
            .outbox
            .stage_to_operator(None, body)
            .and_then(|(post, frame)| {
                self.store.append(&frame)?;
                self.outbox.posted(&post);
                Ok(())
            });
        if let Err(e) = sent {
            tracing::warn!(persona = key, error = %format!("{e:#}"), "the new-persona note was not posted");
        }
    }
}
