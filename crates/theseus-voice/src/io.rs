//! The seam between the engine and a voice call (design §2.8): frames in per
//! speaker, audio out. songbird implements it for real ([`crate::SongbirdIo`],
//! with the `voice` feature), and [`WavIo`] implements it for tests, from WAV
//! files.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use tokio::time::{sleep_until, Instant};

use crate::audio::{duration_of, read_wav, samples_in, Audio, WavError, FRAME, FRAME_SAMPLES};

/// Who spoke: the Discord user behind an SSRC.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Speaker(pub u64);

impl fmt::Display for Speaker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// One speaker's part of a tick: 20 ms of decoded audio, 960 samples.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub speaker: Speaker,
    pub samples: Vec<i16>,
}

/// A clip the engine plays, named by the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClipId(pub u64);

/// What the call gives the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Heard {
    /// One 20 ms tick: a frame from each speaker who sent audio in it. A
    /// speaker with no frame was silent. Ticks come every 20 ms for as long
    /// as the call is up, so they are also the engine's clock for speech.
    Tick(Vec<Frame>),
    /// A clip stopped: it played to its end, was stopped, or failed.
    Ended(ClipId),
}

/// The seam. The engine owns one per call and polls [`VoiceIo::next`] in a
/// `select!`, so `next` must be cancel-safe: dropping its future loses
/// nothing.
pub trait VoiceIo: Send {
    /// What the call heard or did next, or `None` once the call is gone.
    fn next(&mut self) -> BoxFuture<'_, Option<Heard>>;
    /// Start playing `clip` at once, as `id`. The engine plays one clip at a
    /// time, and starts the next only after the last one's `Ended`.
    fn play(&mut self, id: ClipId, clip: Audio) -> BoxFuture<'_, ()>;
    /// Stop the clip that's playing, if any. Its `Ended` still comes.
    fn stop(&mut self) -> BoxFuture<'_, ()>;
    /// Why the call is gone, once `next` said it was: a dropped connection's
    /// reason. `None` when it simply ended, as a test's WAV runs out.
    fn gone(&self) -> Option<String> {
        None
    }
}

/// A clip played through a [`WavIo`], with its times from the call's start.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Played {
    pub id: ClipId,
    pub length: Duration,
    pub started: Duration,
    /// When it stopped: at its end, or earlier if it was stopped.
    pub ended: Option<Duration>,
    /// Stopped before its end.
    pub stopped: bool,
}

/// The log of what a [`WavIo`] played, shared with the test that reads it.
#[derive(Clone, Debug, Default)]
pub struct PlayLog(Arc<Mutex<Vec<Played>>>);

impl PlayLog {
    pub fn get(&self) -> Vec<Played> {
        self.0.lock().expect("the play log's lock").clone()
    }

    fn push(&self, played: Played) {
        self.0.lock().expect("the play log's lock").push(played);
    }

    fn end(&self, id: ClipId, at: Duration, stopped: bool) {
        let mut log = self.0.lock().expect("the play log's lock");
        if let Some(p) = log.iter_mut().find(|p| p.id == id) {
            p.ended = Some(at);
            p.stopped = stopped;
        }
    }
}

/// The seam's stand-in for tests (design §2.8). Each speaker's audio plays
/// into the call at set times; ticks come every 20 ms on tokio's clock, so a
/// test with the clock paused runs in virtual time, fast and exact under any
/// load; and what the engine plays is logged with its times. A clip "plays"
/// for its length: its `Ended` comes then, or at once when it's stopped.
pub struct WavIo {
    origin: Instant,
    length: Duration,
    lines: Vec<Line>,
    ticks: u64,
    playing: Option<(ClipId, Instant)>,
    ended: VecDeque<ClipId>,
    log: PlayLog,
}

struct Line {
    speaker: Speaker,
    /// The first sample's index on the call's timeline.
    at: usize,
    audio: Audio,
}

impl WavIo {
    /// A call that starts now and lasts `length`: then its ticks stop, and
    /// `next` returns `None`.
    pub fn new(length: Duration) -> Self {
        Self {
            origin: Instant::now(),
            length,
            lines: Vec::new(),
            ticks: 0,
            playing: None,
            ended: VecDeque::new(),
            log: PlayLog::default(),
        }
    }

    /// `speaker` says `audio`, starting `at` from the call's start.
    pub fn say(&mut self, speaker: Speaker, at: Duration, audio: Audio) -> &mut Self {
        self.lines.push(Line {
            speaker,
            at: samples_in(at),
            audio,
        });
        self
    }

    /// `speaker` says what's in the WAV file at `path`, starting `at`.
    pub fn say_wav(
        &mut self,
        speaker: Speaker,
        at: Duration,
        path: &Path,
    ) -> Result<&mut Self, WavError> {
        let audio = read_wav(path)?;
        Ok(self.say(speaker, at, audio))
    }

    /// What the engine played, as it plays.
    pub fn played(&self) -> PlayLog {
        self.log.clone()
    }

    fn since_origin(&self, at: Instant) -> Duration {
        at.saturating_duration_since(self.origin)
    }

    /// The frames of tick `tick`: each speaker's samples in its 20 ms, with
    /// a speaker's overlapping lines mixed.
    fn frames(&self, tick: u64) -> Vec<Frame> {
        let start = tick as usize * FRAME_SAMPLES;
        let end = start + FRAME_SAMPLES;
        let mut mixed: BTreeMap<Speaker, Vec<i32>> = BTreeMap::new();
        for line in &self.lines {
            let line_end = line.at + line.audio.len();
            if line_end <= start || line.at >= end {
                continue;
            }
            let frame = mixed
                .entry(line.speaker)
                .or_insert_with(|| vec![0; FRAME_SAMPLES]);
            for (i, slot) in frame.iter_mut().enumerate() {
                let t = start + i;
                if t >= line.at && t < line_end {
                    *slot += i32::from(line.audio.samples()[t - line.at]);
                }
            }
        }
        mixed
            .into_iter()
            .map(|(speaker, samples)| Frame {
                speaker,
                samples: samples
                    .into_iter()
                    .map(|s| s.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
                    .collect(),
            })
            .collect()
    }
}

impl VoiceIo for WavIo {
    fn next(&mut self) -> BoxFuture<'_, Option<Heard>> {
        Box::pin(async move {
            // Cancel-safe: nothing changes until the wait is over, and nothing
            // waits after a change.
            if let Some(id) = self.ended.pop_front() {
                return Some(Heard::Ended(id));
            }
            let window = duration_of(self.ticks as usize * FRAME_SAMPLES);
            if window >= self.length {
                return None;
            }
            // Tick k carries the audio of [20k, 20k + 20) ms, and comes at its end.
            let tick_at = self.origin + window + FRAME;
            match self.playing {
                Some((id, ends)) if ends <= tick_at => {
                    sleep_until(ends).await;
                    self.playing = None;
                    self.log.end(id, self.since_origin(ends), false);
                    Some(Heard::Ended(id))
                }
                _ => {
                    sleep_until(tick_at).await;
                    let frames = self.frames(self.ticks);
                    self.ticks += 1;
                    Some(Heard::Tick(frames))
                }
            }
        })
    }

    fn play(&mut self, id: ClipId, clip: Audio) -> BoxFuture<'_, ()> {
        let now = Instant::now();
        if let Some((old, _)) = self.playing.take() {
            self.log.end(old, self.since_origin(now), true);
            self.ended.push_back(old);
        }
        self.playing = Some((id, now + clip.duration()));
        self.log.push(Played {
            id,
            length: clip.duration(),
            started: self.since_origin(now),
            ended: None,
            stopped: false,
        });
        Box::pin(std::future::ready(()))
    }

    fn stop(&mut self) -> BoxFuture<'_, ()> {
        if let Some((id, _)) = self.playing.take() {
            self.log.end(id, self.since_origin(Instant::now()), true);
            self.ended.push_back(id);
        }
        Box::pin(std::future::ready(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Speaker = Speaker(1);
    const B: Speaker = Speaker(2);

    #[tokio::test(start_paused = true)]
    async fn ticks_come_every_20_ms_with_each_speakers_frame() {
        let mut io = WavIo::new(Duration::from_millis(100));
        io.say(
            A,
            Duration::from_millis(20),
            Audio::new(vec![7; 2 * FRAME_SAMPLES]),
        )
        .say(
            B,
            Duration::from_millis(40),
            Audio::new(vec![-3; FRAME_SAMPLES]),
        );
        let start = Instant::now();
        let mut ticks = Vec::new();
        while let Some(heard) = io.next().await {
            let Heard::Tick(frames) = heard else {
                panic!("nothing played, so nothing ends")
            };
            let who: Vec<_> = frames.iter().map(|f| (f.speaker, f.samples[0])).collect();
            ticks.push((start.elapsed().as_millis(), who));
        }
        assert_eq!(
            ticks,
            vec![
                (20, vec![]),
                (40, vec![(A, 7)]),
                (60, vec![(A, 7), (B, -3)]),
                (80, vec![]),
                (100, vec![]),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_clip_ends_after_its_length_or_at_once_when_stopped() {
        let mut io = WavIo::new(Duration::from_secs(1));
        let log = io.played();
        io.play(ClipId(1), Audio::silence(Duration::from_millis(50)))
            .await;
        let mut ended = None;
        while let Some(heard) = io.next().await {
            if heard == Heard::Ended(ClipId(1)) {
                ended = Some(io.since_origin(Instant::now()));
                break;
            }
        }
        assert_eq!(ended, Some(Duration::from_millis(50)));
        io.play(ClipId(2), Audio::silence(Duration::from_secs(5)))
            .await;
        io.next().await; // a tick at 60 ms
        io.stop().await;
        assert_eq!(io.next().await, Some(Heard::Ended(ClipId(2))));
        let log = log.get();
        assert_eq!(log[0].ended, Some(Duration::from_millis(50)));
        assert!(!log[0].stopped);
        assert_eq!(log[1].started, Duration::from_millis(50));
        assert_eq!(log[1].ended, Some(Duration::from_millis(60)));
        assert!(log[1].stopped);
    }
}
