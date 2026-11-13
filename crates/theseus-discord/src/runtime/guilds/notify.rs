//! The announcement place, bound from chat (theseus-nuqut; the owner's standing
//! principle S17-D7: what he binds, he binds by talking). A child of `guilds`,
//! which holds the places' bits.
//!
//! - `notify here` in a private place makes it the announcement place: the
//!   operator lane writes the announcements (`theseus_core::outbox::announces`)
//!   there instead of to his DM, and everything that needs him keeps the DM.
//! - `notify off` gives the announcements back to the DM.
//! - `notify where` says where they go and nothing else.
//!
//! What it writes is one META key, `discord.notify_place`, holding the
//! channel: the binding's own state, as `discord.task_home` and
//! `discord.bound.<place>` are, so a restart binds the same place again and no
//! session, turn or ledger row is involved. It takes precedence over the
//! bindings file's `notify = true`, because it is the later word.
//!
//! **Who may say it** is `self_word`'s gate exactly: the place rule must call
//! this place private, and the message's author must be one the bindings file
//! lets drive it (`Routes::resolve`, before `control` is ever reached). It
//! never names a place the file does not bind, and it moves announcements
//! alone: nothing that waits on the owner can land anywhere but his DM,
//! whatever is bound here.

use theseus_protocol::PlaceClass;

use super::super::{Control, Place, Shared};

/// The META key: the channel the owner bound from chat, as a string, or
/// empty for "the DM, as before".
pub(super) const KEY: &str = "discord.notify_place";

/// What the owner said about the announcement place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Said {
    /// `notify here`: this place takes the announcements.
    Here,
    /// `notify off`: the owner's DM takes them again.
    Off,
    /// `notify where`: say where they go.
    Where,
}

/// The answer in a place the place rule does not call private.
const PRIVATE: &str = "Where the announcements go is yours alone.";

/// Which word `text` is: exactly `notify here`, `notify off` or
/// `notify where`, any case. Anything else is an ordinary message, so a
/// sentence that merely begins with the word runs as a turn, as before.
pub(crate) fn words(text: &str) -> Option<Control> {
    let mut w = text.split_whitespace();
    let (a, b) = (w.next()?, w.next()?);
    if w.next().is_some() || !a.eq_ignore_ascii_case("notify") {
        return None;
    }
    let said = match b.to_ascii_lowercase().as_str() {
        "here" => Said::Here,
        "off" => Said::Off,
        "where" => Said::Where,
        _ => return None,
    };
    Some(Control::NotifyPlace(said))
}

impl Shared {
    /// The announcement place the owner bound from chat, by its channel: the
    /// META key, when it holds a channel. None gives the bindings file's
    /// `notify = true` channel its say, and with neither the announcements
    /// keep the owner's DM, which is what a daemon that has bound none
    /// writes.
    pub(crate) fn notify_said(&self) -> Option<u64> {
        self.core
            .store
            .get_meta::<String>(KEY)
            .ok()
            .flatten()
            .and_then(|s| s.parse().ok())
    }

    /// Write the owner's word: the channel, or empty for the DM.
    fn say_notify(&self, channel: Option<u64>) -> anyhow::Result<()> {
        let v = channel.map(|c| c.to_string()).unwrap_or_default();
        self.core.store.put_meta(KEY, &v)?;
        Ok(())
    }
}

impl Place {
    /// `notify here`, `notify off`, `notify where`: the answer, said in the
    /// place. A read and at most one META write; no turn, no frame of any
    /// session's, no model call.
    pub(in crate::runtime) fn notify_place(&self, said: Said, by: &str) -> String {
        let core = &self.shared.core;
        let class = core
            .runner
            .place_rule
            .class(&core.cfg, Some(self.target.as_str()));
        if class != PlaceClass::Private {
            return PRIVATE.into();
        }
        let here = self.target.strip_prefix("discord:channel:").and_then(|c| {
            let id: u64 = c.parse().ok()?;
            Some(id)
        });
        match said {
            Said::Where => self.notify_where(),
            Said::Off => match self.shared.say_notify(None) {
                Ok(()) => {
                    "The announcements go to your DM again. `notify here` moves them back.".into()
                }
                Err(e) => format!("⚠️ Could not write where the announcements go: {e:#}."),
            },
            Said::Here => {
                let Some(channel) = here else {
                    return "The announcements already reach you here. Say `notify here` in the \
                            channel or thread that should carry them instead."
                        .into();
                };
                match self.shared.say_notify(Some(channel)) {
                    Ok(()) => format!(
                        "🔔 The announcements come here now (bound by {by}): the morning note, \
                         the digests, the self-improvement notices, the AWS month warning and \
                         the Google upkeep note. Anything that needs you — an approval, a \
                         question, a card, a retraction, the day's ceiling — still goes to your \
                         DM. `notify off` gives them back."
                    ),
                    Err(e) => format!("⚠️ Could not write where the announcements go: {e:#}."),
                }
            }
        }
    }

    /// Where the announcements go, in one line: the owner's own word, else the
    /// bindings file's, else his DM.
    fn notify_where(&self) -> String {
        if let Some(c) = self.shared.notify_said() {
            return format!("The announcements go to <#{c}>, which you bound here. `notify off` gives them back to your DM.");
        }
        match self.shared.place_bits.notify_place() {
            Some((c, label)) => format!(
                "The announcements go to <#{c}> ({label}), which the bindings file marks \
                 `notify = true`. `notify here` moves them to the place you say it in."
            ),
            None => "The announcements go to your DM. `notify here`, said in a channel or thread \
                     of yours, moves them there."
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_exactly_three_and_nothing_else_is_one() {
        let said = |t: &str| match words(t) {
            Some(Control::NotifyPlace(s)) => Some(s),
            _ => None,
        };
        assert_eq!(said("notify here"), Some(Said::Here));
        assert_eq!(said("Notify HERE"), Some(Said::Here));
        assert_eq!(said("notify off"), Some(Said::Off));
        assert_eq!(said("notify where"), Some(Said::Where));
        // Not a word: a third word, a sentence that opens with it, another
        // second word, and the bare word.
        for t in [
            "notify here please",
            "notify the team",
            "notify",
            "please notify here",
            "",
            "notifyhere",
        ] {
            assert!(words(t).is_none(), "{t:?}");
        }
    }
}
