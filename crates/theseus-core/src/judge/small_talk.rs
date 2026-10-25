//! Small talk and status checks (theseus-648f8): `categorize.v1` places a
//! session by what its messages are about, and "How is it going?" is about
//! nothing. A window whose every new human message is small talk is not
//! judged: no call, no spend, and no `none` row either. The rule is code,
//! not a model's: a message is small talk when it is short
//! ([`MAX_WORDS`]) and every word of it belongs to a greeting, a status
//! check or an acknowledgement ([`PHRASES`]) or is a filler beside one
//! ([`FILLER`]). One word left over ("refactor") makes it substantive, so
//! "ok let's refactor the auth" is judged.

/// The most words a small-talk message has. A longer message says more than
/// a greeting, whatever it opens with.
pub const MAX_WORDS: usize = 6;

/// Greetings, status checks and acknowledgements, as lowercase words with
/// no punctuation (an apostrophe stays: `how's`). A message is built of
/// these and [`FILLER`] alone.
const PHRASES: &[&str] = &[
    "how is it going",
    "how's it going",
    "hows it going",
    "how are you",
    "how are things",
    "how's everything",
    "what's up",
    "whats up",
    "any update",
    "any updates",
    "any news",
    "status check",
    "status",
    "are you there",
    "you there",
    "still there",
    "ping",
    "good morning",
    "good afternoon",
    "good evening",
    "good night",
    "hi",
    "hello",
    "hey",
    "yo",
    "thanks",
    "thank you",
    "thx",
    "ty",
    "ok",
    "okay",
    "k",
    "cool",
    "great",
    "nice",
    "got it",
    "sounds good",
    "yes",
    "yep",
    "no",
];

/// Words that may sit beside a phrase without adding a subject.
const FILLER: &[&str] = &[
    "there", "you", "so", "much", "a", "lot", "again", "all", "everyone", "please", "very", "just",
];

/// The words of a message: lowercase, a curly apostrophe made straight,
/// split at anything but a letter, a digit or an apostrophe.
fn words(text: &str) -> Vec<String> {
    text.replace('\u{2019}', "'")
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether `text` is small talk or a status check (see the module doc). An
/// empty message is not: it may carry a file.
pub fn is_small_talk(text: &str) -> bool {
    let words = words(text);
    if words.is_empty() || words.len() > MAX_WORDS {
        return false;
    }
    let mut at = 0;
    while at < words.len() {
        let longest = PHRASES
            .iter()
            .map(|p| p.split(' ').collect::<Vec<_>>())
            .filter(|p| words[at..].len() >= p.len() && words[at..at + p.len()] == p[..])
            .map(|p| p.len())
            .max();
        match longest {
            Some(n) => at += n,
            None if FILLER.contains(&words[at].as_str()) => at += 1,
            None => return false,
        }
    }
    true
}

/// Whether every message is small talk; none at all is not (nothing to skip).
pub fn all_small_talk<'a>(texts: impl IntoIterator<Item = &'a str>) -> bool {
    let mut any = false;
    for t in texts {
        if !is_small_talk(t) {
            return false;
        }
        any = true;
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greetings_and_status_checks_are_small_talk() {
        for t in [
            "How is it going?",
            "how's it going",
            "How\u{2019}s it going?",
            "status?",
            "Any update?",
            "what's up",
            "Hi",
            "hello there!",
            "thanks",
            "Thank you so much",
            "ok",
            "Good morning!",
            "ok thanks",
        ] {
            assert!(is_small_talk(t), "{t:?}");
        }
    }

    #[test]
    fn a_message_with_a_subject_is_not() {
        for t in [
            "ok let's refactor the auth",
            "status of the migration?",
            "hi, can you look at the build",
            "How is it going with the migration to the new store layout today?",
            "",
            "   ",
        ] {
            assert!(!is_small_talk(t), "{t:?}");
        }
    }

    #[test]
    fn a_window_is_small_talk_only_when_every_message_is() {
        assert!(all_small_talk(["How is it going?", "thanks", "ok"]));
        assert!(!all_small_talk([
            "How is it going?",
            "fix the harbor build"
        ]));
        assert!(!all_small_talk(std::iter::empty::<&str>()));
    }
}
