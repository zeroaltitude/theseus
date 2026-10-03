//! The stand-in VAD (design §2.8): one per listed speaker. A frame whose
//! energy reaches a threshold is speech; an utterance opens at the first
//! speech frame and closes after 700 ms with none. A speech provider's own
//! endpointing replaces it (45a).
//!
//! The soft start of a word (a fricative, a breath) can sit under the
//! threshold, and a transcriber that hears a word without its start drops it:
//! 45a's live round trip lost "Theseus" so. So the frames received in the
//! 200 ms before an utterance opens are kept, and open it.

use std::collections::VecDeque;

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
    /// The ticks before an utterance opens whose frames, received under the
    /// threshold, begin it.
    pub pre_roll_frames: u32,
}

/// One speaker's utterance detector.
#[derive(Debug, Default)]
pub(crate) struct Vad {
    open: Option<Open>,
    /// The frames received while closed, by tick, the last `pre_roll_frames`.
    before: VecDeque<(u64, Vec<i16>)>,
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
            match (speech, frame) {
                (true, Some(f)) => self.open = Some(self.opening(tick, f, s)),
                (false, Some(f)) if s.pre_roll_frames > 0 => {
                    self.before.push_back((tick, f.to_vec()));
                    while self.before.len() > s.pre_roll_frames as usize {
                        self.before.pop_front();
                    }
                }
                _ => {}
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

    /// An utterance opening at `tick` with its first speech frame `f`, after
    /// the frames received in the pre-roll's ticks, each at its own tick and
    /// a tick with none as silence.
    fn opening(&mut self, tick: u64, f: &[i16], s: &VadSettings) -> Open {
        let from = tick.saturating_sub(u64::from(s.pre_roll_frames));
        let kept: Vec<(u64, Vec<i16>)> = self
            .before
            .drain(..)
            .filter(|(t, _)| (from..tick).contains(t))
            .collect();
        let first_tick = kept.first().map_or(tick, |(t, _)| *t);
        let mut samples = Vec::with_capacity((tick - first_tick + 1) as usize * FRAME_SAMPLES);
        let mut at = first_tick;
        for (t, frame) in kept {
            samples.resize(samples.len() + (t - at) as usize * FRAME_SAMPLES, 0);
            samples.extend_from_slice(&frame);
            at = t + 1;
        }
        samples.resize(samples.len() + (tick - at) as usize * FRAME_SAMPLES, 0);
        samples.extend_from_slice(f);
        Open {
            first_tick,
            frames: (tick - first_tick + 1) as u32,
            speech_frames: 1,
            speech_end: samples.len(),
            samples,
            quiet: 0,
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
        pre_roll_frames: 10,
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

    /// A word's soft start, received under the threshold just before it,
    /// begins the utterance: the 10 frames before it at most, each at its
    /// tick, and a tick with no frame as silence. Older frames, and ticks
    /// with none before the first kept, add nothing.
    #[test]
    fn the_soft_start_before_an_utterance_begins_it() {
        let soft: Vec<i16> = Audio::tone(220.0, FRAME, 0.005).samples().to_vec();
        assert!(rms(&soft) < S.speech_rms, "under the threshold");
        let loud = speech();
        let mut vad = Vad::default();
        // Soft frames at ticks 0 to 11, none at 12, then speech from 13.
        for tick in 0..12 {
            assert!(!vad.push(tick, Some(&soft), &S).speech);
        }
        vad.push(12, None, &S);
        for tick in 13..23 {
            vad.push(tick, Some(&loud), &S);
        }
        let closed = (23..60)
            .find_map(|tick| vad.push(tick, None, &S).closed)
            .expect("it closes");
        let Closed::Utterance { first_tick, audio } = closed else {
            panic!("an utterance")
        };
        // Ticks 3 to 12 (12 has no frame, so silence), then 13 to 22.
        assert_eq!(first_tick, 3);
        assert_eq!(audio.duration(), FRAME * 20);
        let s = audio.samples();
        assert_eq!(&s[..FRAME_SAMPLES], soft.as_slice(), "tick 3's soft frame");
        assert!(s[9 * FRAME_SAMPLES..10 * FRAME_SAMPLES]
            .iter()
            .all(|&x| x == 0));
        assert_eq!(&s[10 * FRAME_SAMPLES..11 * FRAME_SAMPLES], loud.as_slice());
        // Off, nothing is kept.
        let mut vad = Vad::default();
        let off = VadSettings {
            pre_roll_frames: 0,
            ..S
        };
        vad.push(0, Some(&soft), &off);
        for tick in 1..10 {
            vad.push(tick, Some(&loud), &off);
        }
        let Some(Closed::Utterance { first_tick, .. }) =
            (10..60).find_map(|tick| vad.push(tick, None, &off).closed)
        else {
            panic!("an utterance")
        };
        assert_eq!(first_tick, 1);
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
