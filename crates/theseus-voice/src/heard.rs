//! What an utterance heard over Theseus's speech was (theseus-9ln5): pure
//! rules over its transcript and the sentences it overlapped, so a laugh, a
//! cough, Theseus's own voice coming back, or a "yeah" doesn't throw a reply
//! away. The engine decides with them when a held reply resumes, and when
//! words cut it.

/// What an utterance was heard as.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeardAs {
    /// Anything else: a turn, and over speech, a cut.
    Words,
    /// An empty transcript: a laugh, a cough, talk off the microphone.
    Wordless,
    /// Theseus's own sentence, back through a speaker's microphone.
    Echo,
    /// A listener's "yeah" or "mm-hm" over speech: go on.
    Backchannel,
    /// A request to go on: "go on", "continue".
    Resume,
}

/// Where an utterance's first speech frame came, for the rules that apply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    /// While a clip played, during a hold, or in a gap between sentences
    /// still queued: every rule applies.
    Speech,
    /// Within the echo tail after the last queued sentence ended: checked for
    /// echo only, since a "yeah" there answers the reply.
    Tail,
    /// Neither: words, or wordless.
    None,
}

/// A listener's short sounds of assent, each as its words.
const BACKCHANNELS: &[&[&str]] = &[
    &["yeah"],
    &["yes"],
    &["yep"],
    &["yup"],
    &["okay"],
    &["ok"],
    &["right"],
    &["sure"],
    &["uh", "huh"],
    &["mm", "hm"],
    &["mhm"],
    &["mm"],
    &["hm"],
    &["hmm"],
    &["cool"],
    &["nice"],
    &["alright"],
    &["got", "it"],
    &["i", "see"],
];

/// The requests to go on, each as its words.
const RESUMES: &[&str] = &[
    "go on",
    "continue",
    "keep going",
    "carry on",
    "go ahead",
    "please continue",
    "sorry go on",
    "as you were saying",
];

/// A backchannel has at most this many words.
const BACKCHANNEL_WORDS: usize = 3;

/// `text`'s words: its lower-cased runs of letters, digits and apostrophes,
/// a typographic apostrophe read as a plain one.
pub fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('’', "'")
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Theseus's own words heard back: at least 2 words, and at least 60% of
/// its distinct words among the words of `sentences` (the one that was
/// playing, and those that ended in the echo tail).
pub fn is_echo(text: &str, sentences: &[String]) -> bool {
    let heard = words(text);
    if heard.len() < 2 {
        return false;
    }
    let said: std::collections::HashSet<String> = sentences.iter().flat_map(|s| words(s)).collect();
    let distinct: std::collections::HashSet<&String> = heard.iter().collect();
    let echoed = distinct.iter().filter(|w| said.contains(**w)).count();
    echoed * 5 >= distinct.len() * 3
}

/// At most 3 words, all from the backchannels' list.
pub fn is_backchannel(text: &str) -> bool {
    let heard = words(text);
    if heard.is_empty() || heard.len() > BACKCHANNEL_WORDS {
        return false;
    }
    let mut rest = heard.as_slice();
    while !rest.is_empty() {
        let Some(phrase) = BACKCHANNELS
            .iter()
            .filter(|p| rest.len() >= p.len() && rest.iter().zip(p.iter()).all(|(a, b)| a == b))
            .max_by_key(|p| p.len())
        else {
            return false;
        };
        rest = &rest[phrase.len()..];
    }
    true
}

/// Only a request to go on.
pub fn is_resume(text: &str) -> bool {
    let heard = words(text).join(" ");
    RESUMES.contains(&heard.as_str())
}

/// What an utterance whose transcript is `text` was heard as, by where it
/// came (`overlap`) and the sentences it may echo.
pub fn classify(text: &str, overlap: Overlap, sentences: &[String]) -> HeardAs {
    if words(text).is_empty() {
        return HeardAs::Wordless;
    }
    match overlap {
        Overlap::None => HeardAs::Words,
        Overlap::Tail if is_echo(text, sentences) => HeardAs::Echo,
        Overlap::Tail => HeardAs::Words,
        Overlap::Speech if is_echo(text, sentences) => HeardAs::Echo,
        Overlap::Speech if is_backchannel(text) => HeardAs::Backchannel,
        Overlap::Speech if is_resume(text) => HeardAs::Resume,
        Overlap::Speech => HeardAs::Words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn words_are_lower_cased_runs_of_letters_digits_and_apostrophes() {
        assert_eq!(
            words("Don’t stop: it's 3 o'clock, OK?"),
            ["don't", "stop", "it's", "3", "o'clock", "ok"]
        );
        assert_eq!(words("Uh-huh."), ["uh", "huh"]);
        assert!(words(" ... ").is_empty());
    }

    #[test]
    fn an_empty_transcript_is_wordless_wherever_it_came() {
        for overlap in [Overlap::Speech, Overlap::Tail, Overlap::None] {
            assert_eq!(classify("", overlap, &[]), HeardAs::Wordless);
            assert_eq!(classify("  . ", overlap, &[]), HeardAs::Wordless);
        }
    }

    #[test]
    fn an_echo_is_most_of_a_sentence_it_overlapped() {
        let playing = said(&["The deploy finished at noon, and the tests passed."]);
        assert!(is_echo("the deploy finished at noon", &playing));
        // 3 of 5 distinct words: 60%.
        assert!(is_echo("deploy tests passed what now", &playing));
        // 2 of 4: under.
        assert!(!is_echo("deploy tests what now", &playing));
        // One word is never an echo, even one of its own.
        assert!(!is_echo("deploy", &playing));
        assert!(!is_echo("stop", &said(&["Stop the build."])));
        // Words of a sentence that ended in the tail count too.
        let tail = said(&["Here it is.", "Want the log?"]);
        assert!(is_echo("want the log", &tail));
    }

    #[test]
    fn a_backchannel_is_up_to_3_words_from_the_list() {
        for yes in [
            "Yeah.",
            "mm-hm",
            "Uh-huh!",
            "okay, got it",
            "I see.",
            "yeah yeah yeah",
            "Right, sure",
            "Mhm.",
        ] {
            assert!(is_backchannel(yes), "{yes}");
        }
        for no in [
            "yeah yeah yeah yeah",
            "yeah but no",
            "got",
            "stop",
            "okay wait",
            "",
        ] {
            assert!(!is_backchannel(no), "{no}");
        }
    }

    #[test]
    fn a_resume_is_only_a_request_to_go_on() {
        for yes in [
            "Go on.",
            "continue",
            "Keep going!",
            "Sorry, go on.",
            "As you were saying",
        ] {
            assert!(is_resume(yes), "{yes}");
        }
        for no in ["go on to the next one", "don't continue", "go", "please"] {
            assert!(!is_resume(no), "{no}");
        }
    }

    #[test]
    fn over_speech_every_rule_applies_and_words_are_the_rest() {
        let playing = said(&["Here is the first of three long sentences."]);
        let at = |t| classify(t, Overlap::Speech, &playing);
        assert_eq!(at("the first of three long sentences"), HeardAs::Echo);
        assert_eq!(at("yeah"), HeardAs::Backchannel);
        assert_eq!(at("go on"), HeardAs::Resume);
        assert_eq!(at("wait, which account"), HeardAs::Words);
        assert_eq!(at("stop"), HeardAs::Words);
    }

    #[test]
    fn in_the_tail_only_echo_is_checked_and_elsewhere_none() {
        let ended = said(&["Should I deploy it now?"]);
        assert_eq!(classify("yeah", Overlap::Tail, &ended), HeardAs::Words);
        assert_eq!(classify("go on", Overlap::Tail, &ended), HeardAs::Words);
        assert_eq!(
            classify("should I deploy it now", Overlap::Tail, &ended),
            HeardAs::Echo
        );
        assert_eq!(
            classify("should I deploy it now", Overlap::None, &ended),
            HeardAs::Words
        );
        assert_eq!(classify("yeah", Overlap::None, &ended), HeardAs::Words);
    }
}
