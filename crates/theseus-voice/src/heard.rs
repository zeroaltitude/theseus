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
    /// Within the echo tail after the last queued sentence ended, before a
    /// queued reply began, or begun over the last sentence and closed after
    /// it ended: checked for echo only, since a "yeah" there answers the
    /// reply (theseus-1cz8).
    Tail,
    /// Neither: words, or wordless.
    None,
}

/// A listener's short sounds of assent, each as its words: only what never
/// carries an answer alone said over a reply, since a backchannel over one
/// never cuts it. A speech-to-text provider writes "mm-hmm" many ways.
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
    &["mm", "hmm"],
    &["mhm"],
    &["mhmm"],
    &["mmhm"],
    &["mmhmm"],
    &["mm"],
    &["mmm"],
    &["hm"],
    &["hmm"],
    &["uhhuh"],
    &["uh", "hum"],
    &["gotcha"],
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

/// An echo's run of a sentence's words is at least this long.
const ECHO_RUN: usize = 3;

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

/// Theseus's own words heard back (theseus-3ug0): a near-whole, in-order
/// copy of one of `sentences` (the one that was playing, and those that ended
/// in the echo tail). The longest run of its words found contiguously, in
/// order, in one sentence is at least 3 words and at least 80% of its words.
/// An answer that reuses its question's words ("Yes, deploy it now.", "The
/// daily view.") is not one; a speaker's microphone playing a sentence back
/// is.
pub fn is_echo(text: &str, sentences: &[String]) -> bool {
    let heard = words(text);
    if heard.len() < ECHO_RUN {
        return false;
    }
    let run = sentences
        .iter()
        .map(|s| longest_run(&heard, &words(s)))
        .max()
        .unwrap_or(0);
    run >= ECHO_RUN && run * 5 >= heard.len() * 4
}

/// The longest run of `a`'s words found contiguously, in order, in `b`.
fn longest_run(a: &[String], b: &[String]) -> usize {
    // Each row: the run ending at `a[i]` and `b[j]`.
    let mut row = vec![0usize; b.len() + 1];
    let mut best = 0;
    for word in a {
        let mut diagonal = 0;
        for (j, other) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if word == other { diagonal + 1 } else { 0 };
            best = best.max(row[j + 1]);
            diagonal = above;
        }
    }
    best
}

/// At most 3 words, all from the backchannels' list, each maybe after an
/// "oh" ("oh okay", "oh yeah").
pub fn is_backchannel(text: &str) -> bool {
    let heard = words(text);
    if heard.is_empty() || heard.len() > BACKCHANNEL_WORDS {
        return false;
    }
    let mut rest = heard.as_slice();
    while !rest.is_empty() {
        if rest.len() > 1 && rest[0] == "oh" {
            rest = &rest[1..];
            continue;
        }
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
    fn an_echo_is_a_near_whole_in_order_copy_of_one_sentence() {
        let playing = said(&["The deploy finished at noon, and the tests passed."]);
        assert!(is_echo("the deploy finished at noon", &playing));
        // A run of 4 of its 5 words: 80%.
        assert!(is_echo("the deploy finished at what", &playing));
        // 3 of 5 in a row, or the sentence's words out of order: not one.
        assert!(!is_echo("deploy finished at what now", &playing));
        assert!(!is_echo("noon finished the deploy at", &playing));
        assert!(!is_echo("deploy tests passed what now", &playing));
        // One word is never an echo, even one of its own, nor two.
        assert!(!is_echo("deploy", &playing));
        assert!(!is_echo("stop", &said(&["Stop the build."])));
        assert!(!is_echo("tests passed", &playing));
        // Words of a sentence that ended in the tail count too, each
        // sentence on its own.
        let tail = said(&["Here it is.", "Want the log?"]);
        assert!(is_echo("want the log", &tail));
        assert!(!is_echo("it is want the", &tail));
        // A speaker's microphone playing a sentence back, whole.
        let back = said(&["Creates those. I'll check what's running."]);
        assert!(is_echo("Creates those. I'll check what's running.", &back));
    }

    #[test]
    fn an_answer_that_repeats_its_questions_words_is_no_echo() {
        let asked = said(&["Should I deploy it now?"]);
        assert!(!is_echo("Yes, deploy it now.", &asked));
        assert_eq!(
            classify("Yes, deploy it now.", Overlap::Tail, &asked),
            HeardAs::Words
        );
        let either = said(&["Do you want the daily or the monthly view?"]);
        assert!(!is_echo("The daily view.", &either));
        assert_eq!(
            classify("The daily view.", Overlap::Speech, &either),
            HeardAs::Words
        );
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
            "Mhmm.",
            "Mmhmm",
            "Mm-hmm.",
            "mmhm",
            "Uh huh",
            "Gotcha.",
            "Oh, okay.",
            "Oh yeah.",
            "oh, I see",
        ] {
            assert!(is_backchannel(yes), "{yes}");
        }
        for no in [
            "yeah yeah yeah yeah",
            "yeah but no",
            "got",
            "stop",
            "okay wait",
            "oh",
            "oh no",
            "okay oh",
            "oh okay got it",
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
