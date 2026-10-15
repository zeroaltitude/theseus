//! "self digest" and "self log" (theseus-pw1q.19): a message of exactly those
//! two words (any case), in a place of the owner's that is private, answers
//! with what Theseus changed about itself: the digest the weekly `self_digest`
//! post carries, for the week so far, or the last ten changes. Anywhere else
//! it answers that the self log is private, and nothing more. Both read the
//! core (`self.digest`, `self.log`) and write nothing; with the mode off they
//! say so plainly and still give the rows so far, since the log records even
//! then.

use theseus_core::rsi::{digest_capped, digest_line};
use theseus_protocol::rsi::{
    SelfDigestParams, SelfDigestResult, SelfLogParams, SelfLogResult, SelfMode,
};
use theseus_protocol::PlaceClass;

use super::{Control, Place};

/// The words' answer in a shared place.
const PRIVATE: &str = "The self log is private.";

/// What the mode off says first.
const OFF: &str = "Self-improvement is off. Nothing has run.";

/// How many changes `self log` lists, and how much of them one post carries.
const LOG_ROWS: usize = 10;
const POST_CHARS: usize = 1_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Word {
    Digest,
    Log,
}

/// Which word `text` is: exactly `self digest` or `self log`, any case.
pub(super) fn words(text: &str) -> Option<Word> {
    let mut w = text.split_whitespace();
    let (a, b) = (w.next()?, w.next()?);
    if w.next().is_some() || !a.eq_ignore_ascii_case("self") {
        return None;
    }
    match b.to_ascii_lowercase().as_str() {
        "digest" => Some(Word::Digest),
        "log" => Some(Word::Log),
        _ => None,
    }
}

impl Place {
    /// The self controls' arm of `control`: a halt, a resume, or a word.
    pub(super) async fn self_control(&self, cmd: Control, by: &str) -> String {
        match cmd {
            Control::HaltSelf(why, origin) => self.halt_self(why, origin, by).await,
            Control::ResumeSelf(origin) => self.resume_self(origin, by).await,
            Control::SelfWord(word) => self.self_word(word).await,
            _ => String::new(),
        }
    }

    /// The word's answer: the core's, in a private place; else that it is
    /// private.
    pub(super) async fn self_word(&self, word: Word) -> String {
        let core = &self.shared.core;
        let class = core
            .runner
            .place_rule
            .class(&core.cfg, Some(self.target.as_str()));
        if class != PlaceClass::Private {
            return PRIVATE.into();
        }
        match word {
            Word::Digest => self.self_digest().await,
            Word::Log => self.self_log().await,
        }
    }

    async fn self_log(&self) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, SelfLogResult>(
                theseus_protocol::method::SELF_LOG,
                SelfLogParams {
                    since_ms: None,
                    limit: Some(LOG_ROWS),
                },
            )
            .await;
        match r {
            Ok(log) => log_text(&log),
            Err(e) => format!("⚠️ Could not read the self log: {e}."),
        }
    }

    async fn self_digest(&self) -> String {
        let rpc = &self.shared.rpc;
        let digest = rpc
            .call::<_, SelfDigestResult>(
                theseus_protocol::method::SELF_DIGEST,
                SelfDigestParams::default(),
            )
            .await;
        let d = match digest {
            Ok(d) => d,
            Err(e) => return format!("⚠️ Could not read the self digest: {e}."),
        };
        // The mode is the log's state; a digest names it too, but only in a
        // sentence.
        let off = rpc
            .call::<_, SelfLogResult>(
                theseus_protocol::method::SELF_LOG,
                SelfLogParams {
                    since_ms: None,
                    limit: Some(1),
                },
            )
            .await
            .is_ok_and(|l| l.state.mode == SelfMode::Off);
        let text = digest_capped(&d.text);
        match off {
            true => format!("{OFF}\n{text}"),
            false => text,
        }
    }
}

/// `self log`'s post: the mode if off, then the changes, newest first, cut to
/// one post.
fn log_text(log: &SelfLogResult) -> String {
    let mut out = String::new();
    if log.state.mode == SelfMode::Off {
        out.push_str(OFF);
        out.push('\n');
    }
    if log.building {
        out.push_str(
            "The ledger's index is still being built after a start: ask again in a minute.",
        );
        return out;
    }
    if log.rows.is_empty() {
        out.push_str("Theseus has changed nothing about itself yet.");
        return out;
    }
    out.push_str(&format!(
        "The last {} change{} Theseus made to itself, newest first:",
        log.rows.len(),
        if log.rows.len() == 1 { "" } else { "s" }
    ));
    for r in &log.rows {
        let line = digest_line(r);
        if out.chars().count() + line.chars().count() + 1 > POST_CHARS {
            out.push_str("\n… `theseus self log` has the rest.");
            return out;
        }
        out.push('\n');
        out.push_str(&line);
    }
    if log.more {
        out.push_str("\n… older: `theseus self log --since 30d`.");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{words, Word};

    #[test]
    fn the_words_are_exactly_two_in_any_case() {
        assert_eq!(words("self digest"), Some(Word::Digest));
        assert_eq!(words("  Self LOG "), Some(Word::Log));
        assert_eq!(words("self"), None);
        assert_eq!(words("self digest please"), None);
        assert_eq!(words("my self log"), None);
        assert_eq!(words("self logs"), None);
        assert_eq!(words("halt self"), None);
    }
}
