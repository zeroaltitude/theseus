//! The `Speech` contract (design §2.8, §3.12): speech to text and text to
//! speech, each with its usage for spend (45b). The real providers are 45a;
//! the stand-ins here need no key and no network.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::audio::Audio;
use crate::io::Speaker;

pub type SpeechFuture<'a, T> = BoxFuture<'a, Result<T, SpeechError>>;

/// A speech provider: Deepgram, Cartesia, or a stand-in.
pub trait Speech: Send + Sync {
    /// Speech to text: an utterance's audio in, its text and usage out.
    fn transcribe<'a>(&'a self, speaker: Speaker, audio: &'a Audio)
        -> SpeechFuture<'a, Transcript>;
    /// Text to speech: a sentence in, its audio and usage out.
    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Synthesis {
    pub audio: Audio,
    pub usage: Usage,
}

/// What a call to a provider used, for its ledger row and its price (45b):
/// speech to text is priced by audio, text to speech by characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub provider: String,
    pub model: String,
    pub audio: Duration,
    pub chars: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SpeechError(pub String);

/// The canned acknowledgment's words. The engine plays its clip without a
/// synthesis, so it costs nothing (design §2.8).
pub const ACKNOWLEDGMENT: &str = "One moment.";

/// The stand-in speech (design §2.8).
/// - Speech to text gives each speaker's fixture transcripts in order, and
///   then, or live, `[utterance 3.2 s]`.
/// - Text to speech gives the chime for a canned line, and a tone for any
///   other sentence, longer for a longer sentence.
///
/// Delays, on tokio's clock, stand in for a provider's latency.
pub struct StandInSpeech {
    transcripts: Mutex<HashMap<Speaker, VecDeque<String>>>,
    canned: Vec<(String, Audio)>,
    transcribe_delay: Duration,
    synthesize_delay: Duration,
    transcriptions: AtomicUsize,
    syntheses: AtomicUsize,
}

impl Default for StandInSpeech {
    fn default() -> Self {
        Self {
            transcripts: Mutex::default(),
            canned: vec![(ACKNOWLEDGMENT.to_string(), Audio::chime())],
            transcribe_delay: Duration::ZERO,
            synthesize_delay: Duration::ZERO,
            transcriptions: AtomicUsize::new(0),
            syntheses: AtomicUsize::new(0),
        }
    }
}

impl StandInSpeech {
    /// The live stand-in: no fixtures, no delays.
    pub fn new() -> Self {
        Self::default()
    }

    /// `speaker`'s next fixture says `text`.
    #[must_use]
    pub fn transcript(self, speaker: Speaker, text: &str) -> Self {
        self.transcripts
            .lock()
            .expect("the transcripts' lock")
            .entry(speaker)
            .or_default()
            .push_back(text.to_string());
        self
    }

    /// Each call waits this long first, as a provider would.
    #[must_use]
    pub fn delays(self, transcribe: Duration, synthesize: Duration) -> Self {
        Self {
            transcribe_delay: transcribe,
            synthesize_delay: synthesize,
            ..self
        }
    }

    /// How many utterances were transcribed.
    pub fn transcriptions(&self) -> usize {
        self.transcriptions.load(Ordering::SeqCst)
    }

    /// How many sentences were synthesized.
    pub fn syntheses(&self) -> usize {
        self.syntheses.load(Ordering::SeqCst)
    }

    /// The stand-in's audio for a sentence: a tone about as long as the
    /// sentence would take to say (55 ms a character, 0.4 s to 6 s), at one
    /// of four pitches picked by the text, so sentences sound apart.
    pub fn tone_for(text: &str) -> Audio {
        let chars = text.chars().count() as u64;
        let length = Duration::from_millis((chars * 55).clamp(400, 6000));
        let pitch = [392.0, 440.0, 494.0, 523.0][fnv1a(text) as usize % 4];
        Audio::tone(pitch, length, 0.25)
    }
}

impl Speech for StandInSpeech {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a Audio,
    ) -> SpeechFuture<'a, Transcript> {
        Box::pin(async move {
            wait(self.transcribe_delay).await;
            self.transcriptions.fetch_add(1, Ordering::SeqCst);
            let fixture = self
                .transcripts
                .lock()
                .expect("the transcripts' lock")
                .get_mut(&speaker)
                .and_then(VecDeque::pop_front);
            let text = fixture
                .unwrap_or_else(|| format!("[utterance {:.1} s]", audio.duration().as_secs_f64()));
            Ok(Transcript {
                text,
                usage: Usage {
                    provider: "stand-in".into(),
                    model: "fixture".into(),
                    audio: audio.duration(),
                    chars: 0,
                },
            })
        })
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        Box::pin(async move {
            wait(self.synthesize_delay).await;
            self.syntheses.fetch_add(1, Ordering::SeqCst);
            let canned = self.canned.iter().find(|(line, _)| line == text);
            let (audio, model) = match canned {
                Some((_, clip)) => (clip.clone(), "canned"),
                None => (Self::tone_for(text), "tone"),
            };
            Ok(Synthesis {
                usage: Usage {
                    provider: "stand-in".into(),
                    model: model.into(),
                    audio: audio.duration(),
                    chars: text.chars().count(),
                },
                audio,
            })
        })
    }
}

/// A stand-in's latency: no timer at all when there's none.
async fn wait(delay: Duration) {
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

/// FNV-1a, for a pitch that's stable across runs and builds.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fixtures_come_per_speaker_then_the_live_form() {
        let a = Speaker(1);
        let speech = StandInSpeech::new()
            .transcript(a, "hello")
            .transcript(a, "again");
        let audio = Audio::silence(Duration::from_millis(3240));
        let text = |t: Transcript| t.text;
        assert_eq!(
            text(speech.transcribe(Speaker(2), &audio).await.unwrap()),
            "[utterance 3.2 s]"
        );
        assert_eq!(text(speech.transcribe(a, &audio).await.unwrap()), "hello");
        assert_eq!(text(speech.transcribe(a, &audio).await.unwrap()), "again");
        assert_eq!(
            text(speech.transcribe(a, &audio).await.unwrap()),
            "[utterance 3.2 s]"
        );
        assert_eq!(speech.transcriptions(), 4);
    }

    #[tokio::test]
    async fn a_canned_line_is_the_chime_and_a_sentence_is_a_tone() {
        let speech = StandInSpeech::new();
        let ack = speech.synthesize(ACKNOWLEDGMENT).await.unwrap();
        assert_eq!(ack.audio, Audio::chime());
        assert_eq!(ack.usage.model, "canned");
        let line = "The build passed, and the tests too.";
        let said = speech.synthesize(line).await.unwrap();
        assert_eq!(said.audio, StandInSpeech::tone_for(line));
        assert_eq!(said.audio.duration(), Duration::from_millis(36 * 55));
        assert_eq!(said.usage.chars, 36);
        assert_eq!(speech.syntheses(), 2);
    }
}
