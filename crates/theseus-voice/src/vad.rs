//! The stand-in VAD (design §2.8): one per listed speaker. A frame whose
//! energy reaches a threshold is speech; an utterance opens at the first
//! speech frame and closes after 700 ms with none. A speech provider's own
//! endpointing replaces it (45a).

use crate::audio::{rms, Audio, FRAME_SAMPLES};

/// The VAD's settings, in frames.
#[derive(Clone, Copy, Debug)]
pub(crate) struct VadSettings {
    /// A frame whose RMS reaches this is speech.
    pub speech_rms: f64,
    /// Quiet frames that close an utterance.
    pub quiet_frames: u32,
    /// An utterance with fewer speech frames than this is noise.
    pub min_speech_frames: u32,
    /// Frames after which an utterance closes, even mid-speech.
    pub max_frames: u32,
}

/// One speaker's utterance detector.
#[derive(Debug, Default)]
pub(crate) struct Vad {
    open: Option<Open>,
}

#[derive(Debug)]
struct Open {
    first_tick: u64,
    samples: Vec<i16>,
    frames: u32,
    speech_frames: u32,
    /// The samples up to the end of the last speech frame.
    speech_end: usize,
    quiet: u32,
}

/// What one tick did to a speaker's VAD.
#[derive(Debug, Default)]
pub(crate) struct VadStep {
    /// This tick's frame was speech.
    pub speech: bool,
    pub closed: Option<Closed>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Closed {
    /// An utterance: from its first speech frame to its last, so the closing
    /// silence is left out.
    Utterance { first_tick: u64, audio: Audio },
    /// Too little speech to be an utterance (a click, a cough): dropped.
    Noise,
}

impl Vad {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Feed tick `tick`: this speaker's frame, or `None` if they sent none.
    pub fn push(&mut self, tick: u64, frame: Option<&[i16]>, s: &VadSettings) -> VadStep {
        let speech = frame.is_some_and(|f| rms(f) >= s.speech_rms);
        let Some(open) = &mut self.open else {
            if let (true, Some(f)) = (speech, frame) {
                self.open = Some(Open {
                    first_tick: tick,
                    samples: f.to_vec(),
                    frames: 1,
                    speech_frames: 1,
                    speech_end: f.len(),
                    quiet: 0,
                });
            }
            return VadStep {
                speech,
                closed: None,
            };
        };
        match frame {
            Some(f) => open.samples.extend_from_slice(f),
            None => open.samples.resize(open.samples.len() + FRAME_SAMPLES, 0),
        }
        open.frames += 1;
        if speech {
            open.speech_frames += 1;
            open.speech_end = open.samples.len();
            open.quiet = 0;
        } else {
            open.quiet += 1;
        }
        if open.quiet < s.quiet_frames && open.frames < s.max_frames {
            return VadStep {
                speech,
                closed: None,
            };
        }
        let Some(mut open) = self.open.take() else {
            unreachable!("the utterance is open")
        };
        let closed = if open.speech_frames < s.min_speech_frames {
            Closed::Noise
        } else {
            open.samples.truncate(open.speech_end);
            Closed::Utterance {
                first_tick: open.first_tick,
                audio: Audio::new(open.samples),
            }
        };
        VadStep {
            speech,
            closed: Some(closed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::FRAME;

    const S: VadSettings = VadSettings {
        speech_rms: 500.0,
        quiet_frames: 35,
        min_speech_frames: 5,
        max_frames: 1500,
    };

    fn speech() -> Vec<i16> {
        Audio::tone(220.0, FRAME, 0.3).samples().to_vec()
    }

    /// Feed `pattern` (true: a speech frame; false: no frame) from tick 0,
    /// and return the tick each close came at.
    fn feed(vad: &mut Vad, pattern: &[bool]) -> Vec<(u64, Closed)> {
        let frame = speech();
        let mut closed = Vec::new();
        for (tick, &voiced) in pattern.iter().enumerate() {
            let step = vad.push(tick as u64, voiced.then_some(frame.as_slice()), &S);
            assert_eq!(step.speech, voiced);
            if let Some(c) = step.closed {
                closed.push((tick as u64, c));
            }
        }
        closed
    }

    #[test]
    fn an_utterance_closes_on_the_35th_quiet_frame_without_its_silence() {
        let mut vad = Vad::default();
        let mut pattern = vec![false; 3];
        pattern.extend([true; 10]);
        pattern.extend([false; 40]);
        let closed = feed(&mut vad, &pattern);
        assert_eq!(closed.len(), 1);
        let (tick, Closed::Utterance { first_tick, audio }) = &closed[0] else {
            panic!("an utterance, not noise")
        };
        assert_eq!((*tick, *first_tick), (3 + 10 + 34, 3));
        assert_eq!(audio.duration(), FRAME * 10);
        assert!(!vad.is_open());
    }

    #[test]
    fn a_pause_shorter_than_700_ms_keeps_it_open() {
        let mut vad = Vad::default();
        let mut pattern = vec![true; 10];
        pattern.extend([false; 34]);
        pattern.extend([true; 10]);
        pattern.extend([false; 35]);
        let closed = feed(&mut vad, &pattern);
        let [(_, Closed::Utterance { audio, .. })] = closed.as_slice() else {
            panic!("one utterance")
        };
        assert_eq!(audio.duration(), FRAME * 54);
    }

    #[test]
    fn a_click_is_noise_and_a_monologue_is_cut() {
        let mut vad = Vad::default();
        let mut pattern = vec![true; 4];
        pattern.extend([false; 35]);
        assert_eq!(feed(&mut vad, &pattern)[0].1, Closed::Noise);
        let long = VadSettings {
            max_frames: 50,
            ..S
        };
        let frame = speech();
        let closes: Vec<_> = (0..120)
            .filter_map(|tick| vad.push(tick, Some(&frame), &long).closed)
            .collect();
        assert_eq!(closes.len(), 2);
    }
}
