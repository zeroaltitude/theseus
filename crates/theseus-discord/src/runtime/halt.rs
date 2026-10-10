//! "halt self" (theseus-pw1q.2): a message whose first two words are `halt
//! self` (any case; the rest is why) throws self-improvement's kill switch,
//! through `self.halt` as its author with the message's ids, wherever the
//! binding reads it: a halt is anyone's. Only the owner releases it, from
//! the CLI or another private place (`theseus self resume`); there is no
//! Discord word for that.

use theseus_protocol::rsi::{SelfHaltParams, SelfSwitchResult};
use theseus_protocol::DiscordOrigin;

use super::Place;

/// The words a halt is: `Some(why)` when `text` begins `halt self`.
pub(super) fn words(text: &str) -> Option<Option<String>> {
    let mut w = text.split_whitespace();
    let (a, b) = (w.next()?, w.next()?);
    if !(a.eq_ignore_ascii_case("halt") && b.eq_ignore_ascii_case("self")) {
        return None;
    }
    let why = w.collect::<Vec<_>>().join(" ");
    Some((!why.is_empty()).then_some(why))
}

impl Place {
    /// Halt self-improvement as `by`, from where the message came.
    pub(super) async fn halt_self(
        &self,
        why: Option<String>,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, SelfSwitchResult>(
                theseus_protocol::method::SELF_HALT,
                SelfHaltParams {
                    why,
                    author: Some(by.to_string()),
                    discord: origin,
                },
            )
            .await;
        match r {
            Ok(r) if r.changed => "🛑 Self-improvement halted: nothing self-directed runs until \
                                   the owner resumes it (`theseus self resume`, from a private \
                                   place)."
                .into(),
            Ok(_) => "🛑 Self-improvement was already halted.".into(),
            Err(e) => format!("⚠️ Could not halt self-improvement: {e}."),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn halt_self_is_the_first_two_words_and_the_rest_is_why() {
        assert_eq!(words("halt self"), Some(None));
        assert_eq!(
            words("Halt SELF it broke the gate"),
            Some(Some("it broke the gate".into()))
        );
        assert_eq!(words("halt"), None);
        assert_eq!(words("please halt self"), None);
        assert_eq!(words("halt selfish"), None);
    }
}
