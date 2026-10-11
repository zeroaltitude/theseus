//! "halt self" (theseus-pw1q.2): a message whose first two words are `halt
//! self` (any case; the rest is why) throws self-improvement's kill switch,
//! through `self.halt` as its author with the message's ids, wherever the
//! binding reads it: a halt is anyone's. "resume self" releases it through
//! `self.resume`, the same way, and the core counts it only from an author
//! holding one of the owner's handles (`places::owner_anywhere`, the owner's
//! call of 2026-10-10); anyone else's is refused and ledgered there. The CLI's
//! `theseus self resume` and the web UI are the owner's too.

use theseus_protocol::rsi::{SelfHaltParams, SelfResumeParams, SelfSwitchResult};
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

/// Whether `text` is the resume: its first two words `resume self`, any case.
pub(super) fn resume_words(text: &str) -> bool {
    let mut w = text.split_whitespace();
    matches!((w.next(), w.next()), (Some(a), Some(b))
        if a.eq_ignore_ascii_case("resume") && b.eq_ignore_ascii_case("self"))
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
                                   the owner resumes it (`theseus self resume`, or \"resume self\" \
                                   from the owner's own account)."
                .into(),
            Ok(_) => "🛑 Self-improvement was already halted.".into(),
            Err(e) => format!("⚠️ Could not halt self-improvement: {e}."),
        }
    }

    /// Ask the core to release the switch as `by`; it counts only the
    /// owner's.
    pub(super) async fn resume_self(&self, origin: Option<DiscordOrigin>, by: &str) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, SelfSwitchResult>(
                theseus_protocol::method::SELF_RESUME,
                SelfResumeParams {
                    author: Some(by.to_string()),
                    discord: origin,
                    from_job: None,
                },
            )
            .await;
        match r {
            Ok(r) if r.changed => "▶️ Self-improvement resumed.".into(),
            Ok(_) => "▶️ Self-improvement was not halted.".into(),
            Err(e) => format!("🛑 {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{resume_words, words};

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

    #[test]
    fn resume_self_is_the_first_two_words() {
        assert!(resume_words("resume self"));
        assert!(resume_words("Resume SELF now"));
        assert!(!resume_words("resume"));
        assert!(!resume_words("please resume self"));
        assert!(!resume_words("resume selfish"));
        assert!(!resume_words("halt self"));
    }
}
