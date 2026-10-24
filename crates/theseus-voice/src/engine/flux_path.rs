//! The engine on a streaming listener's path (theseus-v774k, `Engine::with_listen`).
//!
//! The listener (Deepgram Flux) decides where a turn ends. A speaker's
//! utterance opens at its `Start` and closes at its `End`, with the
//! listener's transcript, and from the `Pending` onward everything is what it
//! was: `heard.rs`'s rules, the judge, `Utterance`, turns, coalescing.
//!
//! - **Barge-in** (`gate`). A `Start` over Theseus's speech holds the clip at
//!   once, in place of waiting for 300 ms of speech frames, but only when
//!   echo removal agrees: the speaker's residual (the raw frame with
//!   `echo_cancel` off) must show speech since the turn began (a 200 ms
//!   look-back covers the listener's latency), and the speaker is not
//!   echo-prone, not a wordless sound, not at the join. Then the words decide,
//!   as they always did: the `End`'s transcript goes through `heard()`.
//! - **Eager end of turn.** An `Eager` starts the turn early; a `Resumed`
//!   before its reply plays takes the floor back and the reply is superseded
//!   by the `End`'s whole sentence; an `End` after an unresumed `Eager`
//!   confirms it and starts nothing.
//! - **A listener that drops.** A speaker whose stream is down is heard with
//!   the VAD and the pre-recorded recogniser, as without a listener; the turn
//!   in progress when it dropped, kept in a ring of at most [`RING`], is
//!   transcribed once. A listener that cannot be reached at all is off for the
//!   call.
//! - **The VAD** is not used here: its only remaining use is the frame-level
//!   test of speech (`Config::speech_rms`) for the barge-in gate.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use tokio::time::Instant;

use super::{at_tick, Engine, Event, Failure, Pending, Sound, Transcribed};
use crate::audio::{duration_of, rms, Audio, FRAME_SAMPLES};
use crate::deepgram::PROVIDER;
use crate::heard::Overlap;
use crate::io::Speaker;
use crate::listen::{Listen, Listened};
use crate::speech::{SpeechError, Transcript, Usage};

/// The turn in progress a ring keeps for a dropped stream: 30 s.
pub(super) const RING: Duration = Duration::from_secs(30);

/// Audio kept before a turn begins, which a `Start` arrives after: 1 s.
const PRE_ROLL: Duration = Duration::from_secs(1);

/// The ticks before a `Start` whose speech counts toward the gate: 200 ms.
const LOOKBACK: usize = 10;

/// Residual speech frames, since the turn began, that let a `Start` hold a
/// clip: 60 ms.
const GATE_FRAMES: u32 = 3;

/// A listener attached to an engine, and what the engine keeps for it.
pub(super) struct Flux {
    listen: Box<dyn Listen>,
    pub(super) eager: bool,
    model: String,
    lanes: BTreeMap<Speaker, Lane>,
    /// Speakers whose stream is down: heard with the VAD.
    down: BTreeSet<Speaker>,
    /// Speakers whose stream is back but whose VAD utterance is still open.
    rejoin: BTreeSet<Speaker>,
    /// The listener cannot be reached, or is gone: the call goes without it.
    pub(super) off: bool,
}

#[derive(Default)]
struct Lane {
    /// Whether each of the last ticks showed speech (residual or raw).
    recent: VecDeque<bool>,
    /// The raw audio, for a dropped stream's turn.
    ring: VecDeque<i16>,
    turn: Option<OpenTurn>,
    eager: Option<Eager>,
    /// Audio streamed that no utterance has been booked for yet.
    owed: Duration,
}

struct OpenTurn {
    start_tick: u64,
    /// When the `Start` was received.
    event_at: Instant,
    /// Residual speech frames since the turn began (and the look-back).
    speech: u32,
    /// The clip was held for it.
    gated: bool,
    /// An `Eager` made a turn of it already: it no longer holds the floor.
    eager_sent: bool,
}

struct Eager {
    resumed: bool,
}

impl Flux {
    pub(super) fn new(listen: Box<dyn Listen>, model: String, eager: bool) -> Self {
        Self {
            listen,
            eager,
            model,
            lanes: BTreeMap::new(),
            down: BTreeSet::new(),
            rejoin: BTreeSet::new(),
            off: false,
        }
    }

    pub(super) async fn close(&mut self) {
        self.listen.close().await;
    }

    /// Whether a turn of a speaker's is open and holds the floor.
    pub(super) fn floor_held(&self) -> bool {
        self.lanes
            .values()
            .any(|l| l.turn.as_ref().is_some_and(|t| !t.eager_sent))
    }

    /// The speakers with a turn open, to be ticked though they sent no frame.
    pub(super) fn open_speakers(&self) -> Vec<Speaker> {
        self.lanes
            .iter()
            .filter(|(_, l)| l.turn.is_some())
            .map(|(s, _)| *s)
            .collect()
    }

    /// Whether the listener hears `speaker` now.
    fn hears(&self, speaker: Speaker) -> bool {
        !self.off && !self.down.contains(&speaker)
    }
}

/// The next event of the engine's listener, pending while there is none, the
/// listener is off, or the call has none.
pub(super) async fn next(flux: &mut Option<Flux>) -> Option<Listened> {
    match flux {
        Some(f) if !f.off => f.listen.next().await,
        _ => std::future::pending().await,
    }
}

impl Engine {
    /// Listen through `listen`, a streaming recogniser that ends a speaker's
    /// turn itself (theseus-v774k): `model` names it in usage, and `eager`
    /// says whether it is asked for early ends of turn.
    #[must_use]
    pub fn with_listen(mut self, listen: Box<dyn Listen>, model: &str, eager: bool) -> Self {
        self.flux = Some(Flux::new(listen, model.to_string(), eager));
        self
    }

    /// Whether `speaker` is heard by the listener this tick.
    pub(super) fn flux_hears(&self, speaker: Speaker) -> bool {
        self.flux.as_ref().is_some_and(|f| f.hears(speaker))
    }

    /// Whether a listener's open turn holds the floor.
    pub(super) fn flux_floor_held(&self) -> bool {
        self.flux.as_ref().is_some_and(Flux::floor_held)
    }

    /// Speakers the listener hears again once their VAD utterance closes.
    pub(super) fn flux_rejoin(&mut self) {
        let Some(flux) = &mut self.flux else {
            return;
        };
        let back: Vec<Speaker> = flux
            .rejoin
            .iter()
            .copied()
            .filter(|s| self.vads.get(s).is_none_or(|v| !v.is_open()))
            .collect();
        for s in back {
            flux.rejoin.remove(&s);
            flux.down.remove(&s);
        }
    }

    pub(super) fn flux_open_speakers(&self) -> Vec<Speaker> {
        self.flux
            .as_ref()
            .map(Flux::open_speakers)
            .unwrap_or_default()
    }

    /// Tell the listener a tick began.
    pub(super) fn flux_ticked(&mut self, tick: u64) {
        if let Some(flux) = &mut self.flux {
            flux.listen.tick(tick);
        }
    }

    /// A tick of a speaker the listener hears: the raw frame goes to it and
    /// into the ring, and the gate looks at the residual.
    pub(super) fn flux_frame(
        &mut self,
        speaker: Speaker,
        frame: Option<&[i16]>,
        residual_speech: Option<bool>,
        now: Instant,
    ) {
        let speech = residual_speech
            .unwrap_or_else(|| frame.is_some_and(|f| rms(f) >= self.config.speech_rms));
        let Some(flux) = &mut self.flux else {
            return;
        };
        if let Some(f) = frame {
            flux.listen.push(speaker, f);
        }
        let lane = flux.lanes.entry(speaker).or_default();
        lane.recent.push_back(speech);
        while lane.recent.len() > LOOKBACK {
            lane.recent.pop_front();
        }
        let open = lane.turn.is_some();
        match frame {
            Some(f) => lane.ring.extend(f.iter().copied()),
            None if open => lane.ring.extend(std::iter::repeat_n(0, FRAME_SAMPLES)),
            None => {}
        }
        let keep = crate::audio::samples_in(if open { RING } else { PRE_ROLL });
        while lane.ring.len() > keep {
            lane.ring.pop_front();
        }
        if let Some(turn) = &mut lane.turn {
            turn.speech += u32::from(speech);
        }
        self.flux_gate(speaker, now);
    }

    /// Stop the clip for `speaker`'s turn if echo removal shows their speech
    /// (see the module doc). The words decide afterwards.
    fn flux_gate(&mut self, speaker: Speaker, now: Instant) {
        let talking = self.playing.is_some() && !self.stopping && self.hold.is_none();
        let prone = self
            .echoes
            .get(&speaker)
            .is_some_and(|n| *n >= super::ECHO_PRONE)
            || self
                .opening
                .get(&speaker)
                .is_some_and(|o| o.sound == Sound::Wordless || o.at_join);
        let Some(turn) = self
            .flux
            .as_mut()
            .and_then(|f| f.lanes.get_mut(&speaker))
            .and_then(|l| l.turn.as_mut())
        else {
            return;
        };
        if !talking || prone || turn.gated || turn.eager_sent || turn.speech < GATE_FRAMES {
            return;
        }
        let event_at = turn.event_at;
        self.hold(now);
        if let Some(hold) = self.hold.as_mut() {
            hold.event_at = Some(event_at);
            if let Some(turn) = self
                .flux
                .as_mut()
                .and_then(|f| f.lanes.get_mut(&speaker))
                .and_then(|l| l.turn.as_mut())
            {
                turn.gated = true;
            }
            tracing::info!(
                speaker = speaker.0,
                "voice: the listener's start of turn over Theseus's speech held the clip"
            );
        }
    }

    /// What the listener said.
    pub(super) fn flux_event(&mut self, event: Listened, now: Instant) {
        match event {
            Listened::Start { speaker } => self.flux_start(speaker, now),
            Listened::Words { .. } => {}
            Listened::Eager {
                speaker,
                text,
                confidence,
            } => self.flux_eager(speaker, text, confidence, now),
            Listened::Resumed { speaker } => self.flux_resumed(speaker, now),
            Listened::End {
                speaker,
                text,
                confidence,
                streamed,
            } => self.flux_end(speaker, text, confidence, streamed, now),
            Listened::Down { speaker, error } => self.flux_down(speaker, &error),
            Listened::Up { speaker } => {
                if let Some(flux) = &mut self.flux {
                    if self.vads.get(&speaker).is_some_and(|v| v.is_open()) {
                        flux.rejoin.insert(speaker);
                    } else {
                        flux.down.remove(&speaker);
                    }
                    tracing::info!(speaker = speaker.0, "voice: the listener's stream is back");
                }
            }
            Listened::Unavailable { error } => self.flux_unavailable(&error),
        }
    }

    /// The listener is gone: the call goes on without it.
    pub(super) fn flux_gone(&mut self) {
        self.flux_unavailable("the listener ended");
    }

    fn flux_unavailable(&mut self, error: &str) {
        let speakers = self.flux_open_speakers();
        for speaker in speakers {
            self.flux_lose_turn(speaker);
        }
        let Some(flux) = &mut self.flux else {
            return;
        };
        if flux.off {
            return;
        }
        flux.off = true;
        tracing::warn!(
            "voice: the listener cannot be reached ({error}); the call goes on with the pause rules and pre-recorded speech to text"
        );
        self.emit(Event::Failed {
            what: Failure::Flux,
            error: SpeechError(format!(
                "the streaming listener cannot be reached, so this call uses the pause rules: {error}"
            )),
        });
    }

    fn flux_start(&mut self, speaker: Speaker, now: Instant) {
        let tick = self.ticks;
        let already = self
            .flux
            .as_ref()
            .and_then(|f| f.lanes.get(&speaker))
            .is_some_and(|l| l.turn.is_some());
        if already || !self.flux_hears(speaker) {
            return;
        }
        let opening = self.what_is_over(now);
        self.opening.insert(speaker, opening);
        let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) else {
            return;
        };
        let seen = lane.recent.iter().filter(|s| **s).count() as u32;
        lane.eager = None;
        lane.turn = Some(OpenTurn {
            start_tick: tick.saturating_sub(LOOKBACK as u64),
            event_at: now,
            speech: seen,
            gated: false,
            eager_sent: false,
        });
        self.flux_gate(speaker, now);
    }

    fn flux_eager(
        &mut self,
        speaker: Speaker,
        text: String,
        confidence: Option<f32>,
        now: Instant,
    ) {
        if !self.flux.as_ref().is_some_and(|f| f.eager) {
            return;
        }
        let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) else {
            return;
        };
        if lane.turn.is_none() {
            return;
        }
        lane.eager = Some(Eager { resumed: false });
        self.flux_utterance(speaker, text, confidence, Duration::ZERO, true, now);
    }

    fn flux_resumed(&mut self, speaker: Speaker, now: Instant) {
        let opening = self.what_is_over(now);
        let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) else {
            return;
        };
        let (Some(eager), Some(turn)) = (lane.eager.as_mut(), lane.turn.as_mut()) else {
            return;
        };
        eager.resumed = true;
        turn.eager_sent = false;
        self.opening.insert(speaker, opening);
        tracing::info!(
            speaker = speaker.0,
            "voice: the speaker went on after an eager end of turn; its reply is taken back"
        );
    }

    fn flux_end(
        &mut self,
        speaker: Speaker,
        text: String,
        confidence: Option<f32>,
        streamed: Duration,
        now: Instant,
    ) {
        let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) else {
            return;
        };
        match lane.eager.take() {
            Some(Eager { resumed: false }) => {
                // An eager end confirmed: its turn began already.
                lane.turn = None;
                lane.owed += streamed;
                tracing::info!(
                    speaker = speaker.0,
                    "voice: an eager end of turn was confirmed"
                );
                self.flux_trim(speaker);
            }
            _ => self.flux_utterance(speaker, text, confidence, streamed, false, now),
        }
    }

    /// A turn's utterance: the listener's transcript, settled at once.
    fn flux_utterance(
        &mut self,
        speaker: Speaker,
        text: String,
        confidence: Option<f32>,
        streamed: Duration,
        provisional: bool,
        now: Instant,
    ) {
        let tick = self.ticks;
        let model = match &self.flux {
            Some(f) => f.model.clone(),
            None => return,
        };
        let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) else {
            return;
        };
        let start_tick = lane.turn.as_ref().map_or(tick, |t| t.start_tick);
        if provisional {
            if let Some(turn) = &mut lane.turn {
                turn.eager_sent = true;
            }
        } else {
            lane.turn = None;
        }
        let audio_streamed = std::mem::take(&mut lane.owed) + streamed;
        let mut opening = match self.opening.remove(&speaker) {
            Some(o) => o,
            None => self.what_is_over(now),
        };
        if opening.last && self.queue.front().is_none_or(|f| f.opens) {
            opening.overlap = Overlap::Tail;
        }
        self.talk_frames.remove(&speaker);
        let seq = self.next_utterance;
        self.next_utterance += 1;
        if seq == 0 {
            tracing::info!(
                speaker = speaker.0,
                "voice: the call's first utterance closed, heard by the listener"
            );
        }
        let started = at_tick(start_tick);
        let closed = at_tick(tick + 1);
        let chars = text.chars().count();
        self.pending.push_back(Pending {
            seq,
            speaker,
            started,
            length: closed.saturating_sub(started),
            closed,
            opening,
            state: Transcribed::Waiting,
            flux: true,
            ended_at: now,
        });
        let transcript = Transcript {
            text,
            usage: Usage {
                provider: PROVIDER.into(),
                model,
                audio: audio_streamed,
                chars,
            },
            confidence,
        };
        self.heard(seq, Ok(transcript), Duration::ZERO, now);
        self.flux_trim(speaker);
    }

    /// The ring after a turn: the pre-roll alone.
    fn flux_trim(&mut self, speaker: Speaker) {
        if let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) {
            let keep = crate::audio::samples_in(PRE_ROLL);
            while lane.ring.len() > keep && lane.turn.is_none() {
                lane.ring.pop_front();
            }
        }
    }

    /// `speaker`'s stream dropped: the turn in progress is transcribed once,
    /// and the VAD hears them until the stream is back.
    fn flux_down(&mut self, speaker: Speaker, error: &str) {
        let mid_turn = self.flux_lose_turn(speaker);
        if let Some(flux) = &mut self.flux {
            flux.down.insert(speaker);
        }
        tracing::warn!(
            speaker = speaker.0,
            mid_turn,
            "voice: the listener's stream dropped ({error}); {}",
            if mid_turn {
                "the turn in progress is transcribed once, and they are heard by the pause rules until it is back"
            } else {
                "nothing was lost, and they are heard by the pause rules until it is back"
            }
        );
        self.emit(Event::Failed {
            what: Failure::Flux,
            error: SpeechError(format!("the listener's stream dropped: {error}")),
        });
    }

    /// Hand a speaker's open turn, if any, to the pre-recorded recogniser,
    /// once. Whether there was one.
    fn flux_lose_turn(&mut self, speaker: Speaker) -> bool {
        let tick = self.ticks;
        let Some(lane) = self.flux.as_mut().and_then(|f| f.lanes.get_mut(&speaker)) else {
            return false;
        };
        lane.eager = None;
        let Some(turn) = lane.turn.take() else {
            return false;
        };
        let samples: Vec<i16> = lane.ring.drain(..).collect();
        let start = turn.start_tick;
        let opening = match self.opening.remove(&speaker) {
            Some(o) => o,
            None => self.what_is_over(Instant::now()),
        };
        // The ring holds the pre-roll and the turn: its first tick is that
        // much before now.
        let first = tick.saturating_sub(duration_of(samples.len()).as_millis() as u64 / 20);
        let first = first.min(start);
        self.talk_frames.remove(&speaker);
        self.transcribe(speaker, first, tick, Audio::new(samples), opening);
        true
    }
}
