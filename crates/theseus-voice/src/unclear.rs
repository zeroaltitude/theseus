//! A short or unclear transcript, asked of a judge (theseus-s3dwj): what the
//! rules in `heard.rs` can't decide well is a word or two, or a garble, in
//! Theseus's own speech, just after it, or at the join. "yeah", "the", "is
//! miss here" may be the owner, Theseus's echo, or someone else in the room.
//! When the rules would make a turn of one, the engine asks a [`Judge`] and
//! waits at most `Config::judge_ms`: at the judge's act band its answer
//! decides, and on anything else (a lower band, a failure, no answer in time)
//! the rules' verdict stands, as it always stood. The engine knows nothing of
//! Jev: the session gives it a [`Judge`], and with none, nothing is asked.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::heard::{words, Overlap};
use crate::speech::Transcript;

/// A transcript this short (in words) is unclear enough to ask about.
pub const SHORT_WORDS: usize = 3;

/// A transcript the recogniser itself is less sure of than this is unclear:
/// nova-3 gives clear speech 0.9 and up, so under 0.8 it is guessing.
pub const LOW_CONFIDENCE: f32 = 0.80;

/// The sentences of Theseus's a case carries at most.
pub const SAID: usize = 3;

/// When the utterance began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Began {
    /// While a sentence played, or in a gap between its sentences.
    During,
    /// Within the echo tail after the last sentence, or before a queued reply began.
    Tail,
    /// At the join, before the first turn.
    Join,
}

/// The utterance's timing against Theseus's speech, from its first speech frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Timing {
    /// After the sentence playing began, when one was playing.
    pub after_start: Option<Duration>,
    /// After the last sentence that ended, when one ended in the echo tail.
    pub after_end: Option<Duration>,
}

/// What a judge is asked: one transcript, and what it may be an echo of.
#[derive(Clone, Debug, PartialEq)]
pub struct Case {
    pub text: String,
    /// Theseus's last spoken sentences (at most [`SAID`], oldest first): the
    /// strings the rules match against.
    pub said: Vec<String>,
    pub began: Began,
    pub timing: Timing,
    pub length: Duration,
    pub confidence: Option<f32>,
    /// The speaker's microphone has already played Theseus back.
    pub echo_prone: bool,
}

/// What an utterance was, by a judge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// Speech to answer.
    Addressed,
    /// Theseus's own voice back through a microphone.
    Echo,
    /// Noise, or another speaker.
    Other,
}

/// A judge's answer, and the judgment that recorded it.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub said: Said,
    pub probability: f64,
    /// It reached the judge's act band: only then does it decide.
    pub acts: bool,
    pub judgment: String,
}

/// A judge of unclear transcripts. `None`: no answer (a failure, the judge
/// off). Its future is polled on the engine's own task and is cut at
/// `Config::judge_ms`, so a slow judge costs the utterance that long at most.
pub trait Judge: Send + Sync {
    fn judge(&self, case: Case) -> BoxFuture<'static, Option<Verdict>>;
}

/// What was asked of a judge about an utterance, and what it said, on the
/// utterance (`Utterance::judged`).
#[derive(Clone, Debug, PartialEq)]
pub struct Judged {
    pub verdict: Verdict,
    /// How long the judge took.
    pub took: Duration,
}

/// Whether `text`, which the rules would make a turn of and which began as
/// `overlap` says (or at the join), is a case: short or low in confidence,
/// and over Theseus's speech, after it, or at the join.
pub(crate) fn asked(text: &str, confidence: Option<f32>, overlap: Overlap, at_join: bool) -> bool {
    (overlap != Overlap::None || at_join)
        && (words(text).len() <= SHORT_WORDS || confidence.is_some_and(|c| c < LOW_CONFIDENCE))
}

/// The case for `t` when it is one: `began` is where the utterance began
/// (`Overlap`, and whether at the join), `sentences` what it may echo.
pub(crate) fn case(
    t: &Transcript,
    length: Duration,
    (overlap, at_join): (Overlap, bool),
    sentences: &[String],
    timing: Timing,
) -> Option<Case> {
    asked(&t.text, t.confidence, overlap, at_join).then(|| Case {
        text: t.text.clone(),
        said: sentences[sentences.len().saturating_sub(SAID)..].to_vec(),
        began: match (at_join, overlap) {
            (true, _) => Began::Join,
            (false, Overlap::Speech) => Began::During,
            (false, _) => Began::Tail,
        },
        timing,
        length,
        confidence: t.confidence,
        echo_prone: false,
    })
}

/// A judge for tests and scratch daemons: one fixed answer after a delay,
/// counting what it was asked.
pub struct StandInJudge {
    answer: Option<(Said, f64)>,
    delay: Duration,
    asked: Mutex<Vec<Case>>,
    judgments: AtomicUsize,
}

impl StandInJudge {
    /// Answers `said` at `probability`, at the act band from 0.80.
    pub fn new(said: Said, probability: f64) -> Self {
        Self {
            answer: Some((said, probability)),
            delay: Duration::ZERO,
            asked: Mutex::default(),
            judgments: AtomicUsize::new(0),
        }
    }

    /// Never answers.
    pub fn silent() -> Self {
        Self {
            answer: None,
            ..Self::new(Said::Other, 0.0)
        }
    }

    /// Each answer waits this long first, on tokio's clock.
    #[must_use]
    pub fn after(self, delay: Duration) -> Self {
        Self { delay, ..self }
    }

    /// Everything it was asked, in order.
    pub fn cases(&self) -> Vec<Case> {
        self.asked.lock().expect("the cases' lock").clone()
    }
}

impl Judge for StandInJudge {
    fn judge(&self, case: Case) -> BoxFuture<'static, Option<Verdict>> {
        self.asked.lock().expect("the cases' lock").push(case);
        let n = self.judgments.fetch_add(1, Ordering::SeqCst);
        let (delay, answer) = (self.delay, self.answer);
        Box::pin(async move {
            tokio::time::sleep(delay).await;
            let (said, probability) = answer?;
            Some(Verdict {
                said,
                probability,
                acts: probability >= 0.80,
                judgment: format!("jdg_standin{n}"),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_or_low_confidence_transcript_over_speech_is_asked_and_nothing_else() {
        let long = "please deploy the build to staging now";
        assert!(asked("yeah", None, Overlap::Speech, false));
        assert!(asked("is miss here", None, Overlap::Tail, false));
        assert!(asked(long, Some(0.55), Overlap::Speech, false));
        assert!(asked("yes", None, Overlap::None, true), "at the join");
        assert!(!asked(long, Some(0.95), Overlap::Speech, false), "clear");
        assert!(
            !asked(long, None, Overlap::Tail, false),
            "no confidence given"
        );
        assert!(
            !asked("yeah", Some(0.5), Overlap::None, false),
            "far from speech"
        );
        assert!(
            !asked("a b c d", None, Overlap::Speech, false),
            "four words"
        );
    }
}
