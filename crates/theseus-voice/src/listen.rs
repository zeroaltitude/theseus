//! The listener seam (theseus-v774k): a streaming recogniser that decides for
//! itself where a speaker's turn ends, Deepgram Flux for real
//! ([`crate::flux::FluxListen`]) and [`StandInListen`] for tests.
//!
//! The engine pushes each listed speaker's raw frames in and reads
//! [`Listened`] events out. With a listener attached, an utterance opens at a
//! speaker's `Start` and closes at their `End`, with the listener's own
//! transcript: no VAD timer closes it, no second call transcribes it. Without
//! one, nothing here runs and the engine is what it was.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;

use crate::io::Speaker;

/// Flux's forced end of a turn when none is set: 5 s (500 to 10000 ms).
pub const FLUX_EOT_TIMEOUT: Duration = Duration::from_millis(5_000);

/// What a listener tells the engine.
#[derive(Clone, Debug, PartialEq)]
pub enum Listened {
    /// `speaker` began a turn.
    Start { speaker: Speaker },
    /// The turn's words so far.
    Words { speaker: Speaker, text: String },
    /// The listener thinks the turn is ending (only when eager end of turn is
    /// asked for): a reply may start early, and a [`Listened::Resumed`] takes
    /// it back.
    Eager {
        speaker: Speaker,
        text: String,
        confidence: Option<f32>,
    },
    /// The speaker went on after an [`Listened::Eager`].
    Resumed { speaker: Speaker },
    /// The turn ended. `confidence` is the words' (their mean), never the
    /// end-of-turn probability. `streamed` is the audio sent for the speaker
    /// since their last end, for the meter.
    End {
        speaker: Speaker,
        text: String,
        confidence: Option<f32>,
        streamed: Duration,
    },
    /// `speaker`'s stream is down: a turn of theirs in progress is lost to
    /// the listener, and the engine hears them with its VAD and the
    /// pre-recorded recogniser until [`Listened::Up`].
    Down { speaker: Speaker, error: String },
    /// `speaker`'s stream is back.
    Up { speaker: Speaker },
    /// The listener cannot be reached at all: the call goes on without it.
    Unavailable { error: String },
}

/// The seam. The engine owns one per call when it listens this way, and
/// polls [`Listen::next`] in a `select!`, so `next` must be cancel-safe.
pub trait Listen: Send {
    /// A listed speaker's frame, raw, as the call sent it.
    fn push(&mut self, speaker: Speaker, samples: &[i16]);
    /// Tick `tick` of the call began: the stand-in's clock, and a real
    /// listener's cue to keep a quiet stream alive.
    fn tick(&mut self, tick: u64);
    /// How long after a speaker's last frame the engine goes on feeding them
    /// silence: the listener judges a turn's end from the audio it hears, so
    /// it must hear the quiet that ends one. Flux's own forced end of turn.
    fn silence(&self) -> Duration {
        FLUX_EOT_TIMEOUT
    }
    /// The audio streamed for `speaker` that no [`Listened::End`] carried
    /// (trailing silence, keep-alives, a dropped session's remainder), taken:
    /// the engine books it when a stream drops and when the call ends, so
    /// what is billed is what was sent.
    fn unbooked(&mut self, _speaker: Speaker) -> Duration {
        Duration::ZERO
    }
    /// The next event; pending while there is none, `None` once the listener
    /// is gone for good.
    fn next(&mut self) -> BoxFuture<'_, Option<Listened>>;
    /// Close cleanly, at the call's end.
    fn close(&mut self) -> BoxFuture<'_, ()>;
}

/// What a [`StandInListen`] saw, shared with the test that reads it.
#[derive(Clone, Debug, Default)]
pub struct ListenLog(Arc<Mutex<Seen>>);

#[derive(Debug, Default)]
struct Seen {
    /// Each frame pushed: the listener's tick, the speaker, and whether it was
    /// all zeros.
    pushed: Vec<(u64, Speaker, bool)>,
    closed: bool,
    tick: u64,
}

impl ListenLog {
    /// The frames pushed for `speaker`.
    pub fn frames(&self, speaker: Speaker) -> usize {
        self.pushes(speaker).len()
    }

    /// The frames pushed for `speaker` that were all zeros: the engine's fill
    /// for a quiet speaker.
    pub fn zero_frames(&self, speaker: Speaker) -> usize {
        self.pushes(speaker)
            .iter()
            .filter(|(_, zero)| *zero)
            .count()
    }

    /// Each frame pushed for `speaker`: the tick it came on, and whether it
    /// was all zeros.
    pub fn pushes(&self, speaker: Speaker) -> Vec<(u64, bool)> {
        let seen = self.0.lock().expect("the listen log's lock");
        seen.pushed
            .iter()
            .filter(|(_, s, _)| *s == speaker)
            .map(|(t, _, zero)| (*t, *zero))
            .collect()
    }

    /// Whether the engine closed the listener.
    pub fn closed(&self) -> bool {
        self.0.lock().expect("the listen log's lock").closed
    }
}

/// The stand-in listener for tests: events scripted at ticks, so the engine
/// runs in virtual time, like [`crate::StandInSpeech`]'s fixtures.
pub struct StandInListen {
    script: VecDeque<(u64, Listened)>,
    ready: VecDeque<Listened>,
    log: ListenLog,
    silence: Duration,
    owing: BTreeMap<Speaker, Duration>,
}

impl StandInListen {
    pub fn new() -> (Self, ListenLog) {
        let log = ListenLog::default();
        let listen = Self {
            script: VecDeque::new(),
            ready: VecDeque::new(),
            log: log.clone(),
            silence: FLUX_EOT_TIMEOUT,
            owing: BTreeMap::new(),
        };
        (listen, log)
    }

    /// `event` comes at the start of tick `tick`. Events are kept in tick
    /// order (a later call for the same tick comes after the earlier).
    #[must_use]
    pub fn at(mut self, tick: u64, event: Listened) -> Self {
        let at = self.script.partition_point(|(t, _)| *t <= tick);
        self.script.insert(at, (tick, event));
        self
    }

    /// `audio` was streamed for `speaker` that no end of turn carries: the
    /// engine takes it when their stream drops or the call ends.
    #[must_use]
    pub fn owing(mut self, speaker: Speaker, audio: Duration) -> Self {
        *self.owing.entry(speaker).or_default() += audio;
        self
    }

    /// How long the engine feeds a quiet speaker silence (the default is
    /// Flux's own forced end of turn).
    #[must_use]
    pub fn silence_for(mut self, silence: Duration) -> Self {
        self.silence = silence;
        self
    }
}

impl Listen for StandInListen {
    fn push(&mut self, speaker: Speaker, samples: &[i16]) {
        let mut seen = self.log.0.lock().expect("the listen log's lock");
        let tick = seen.tick;
        seen.pushed
            .push((tick, speaker, samples.iter().all(|s| *s == 0)));
    }

    fn silence(&self) -> Duration {
        self.silence
    }

    fn tick(&mut self, tick: u64) {
        self.log.0.lock().expect("the listen log's lock").tick = tick;
        while self.script.front().is_some_and(|(t, _)| *t <= tick) {
            if let Some((_, event)) = self.script.pop_front() {
                self.ready.push_back(event);
            }
        }
    }

    fn next(&mut self) -> BoxFuture<'_, Option<Listened>> {
        Box::pin(async move {
            match self.ready.pop_front() {
                Some(event) => Some(event),
                None => std::future::pending().await,
            }
        })
    }

    fn unbooked(&mut self, speaker: Speaker) -> Duration {
        self.owing.remove(&speaker).unwrap_or_default()
    }

    fn close(&mut self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            self.log.0.lock().expect("the listen log's lock").closed = true;
        })
    }
}
