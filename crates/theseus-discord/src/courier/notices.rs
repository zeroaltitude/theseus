//! The owner's notices place for the operator lane (theseus-nuqut): the
//! announcement place, `theseus-notify`, bound by `notify = true` in the
//! bindings file or `notify here` said in a private place. It takes the
//! announcements (`theseus_core::outbox::announces`) and every FYI note
//! (`owner_note::Kind::Fyi`) that answers no conversation; everything that
//! needs the owner keeps his DM. With none bound, every post keeps the DM.

use serde_json::Value;
use theseus_core::owner_note::{kind_of, Kind};

use super::Lane;

/// Whether a post of `kind` whose finished words are `content` only informs
/// the owner: its head says FYI. The kept-standing card is the one FYI that
/// answers a conversation (it is posted only when the owner said those words
/// in his DM), so it stays where he said them.
pub(super) fn informs(kind: &str, content: &str) -> bool {
    kind != "standing_card" && kind_of(content) == Some(Kind::Fyi)
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

    /// Where an operator post whose words are `content` goes: the notices
    /// place for an announcement, as [`Lane::announced`] says, and for an FYI
    /// that [`informs`]; `dm` for everything else. An FYI whose place is not
    /// one this daemon binds goes to `dm`, so it is never lost.
    pub(super) fn informed(
        &self,
        kind: &str,
        body: &Value,
        content: &str,
        dm: (u64, String),
    ) -> (u64, String) {
        if theseus_core::outbox::announces(body) || !informs(kind, content) {
            return self.announced(body, dm);
        }
        let to = self.notices_place(dm.clone());
        if to.0 == dm.0 || self.shared.binds(&format!("discord:channel:{}", to.0)) {
            return to;
        }
        tracing::info!(
            kind,
            place = %to.1,
            "an FYI's notices place is not one this daemon binds: it goes to the owner's DM"
        );
        dm
    }
}

#[cfg(test)]
mod tests {
    use theseus_core::owner_note::{note, Kind};

    use super::informs;

    #[test]
    fn only_an_fyi_that_answers_no_conversation_informs() {
        let fyi = note(Kind::Fyi, "Theseus started keeping track", "a word", "x");
        assert!(informs("onto_card", &fyi));
        assert!(informs("notice", &fyi));
        assert!(!informs("standing_card", &fyi), "his own words, answered in his DM");
        for k in [Kind::Action, Kind::Decision, Kind::Alert, Kind::Reminder] {
            assert!(!informs("notice", &note(k, "T", "why", "")), "{k:?}");
        }
        assert!(!informs("notice", "plain words with no head"));
    }
}
