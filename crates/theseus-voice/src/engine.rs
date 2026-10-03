//! The pipeline (design §2.8). One [`Engine`] runs one call.
//!
//! - **Receive.** Each listed speaker's frames go through a VAD of their own;
//!   an utterance closes after 700 ms of silence and is transcribed. Anyone
//!   else's audio is dropped at the door, never transcribed.
//! - **Turns.** A transcribed utterance starts a turn when none is in flight.
//!   Utterances that close during a turn coalesce into the next one, in the
//!   order they closed, each keeping its speaker.
//! - **Send.** A reply is split at sentence ends and synthesized one sentence
//!   at a time, one ahead of what's playing, so its first audio starts after
//!   its first sentence. A listed speaker who talks for 300 ms over Theseus
//!   stops the clip and drops the rest (barge-in); the session still has the
//!   whole text.
//! - A turn that runs past 2 s gets the canned acknowledgment, once, when the
//!   line is quiet. A task's report waits for the next pause.
//!
//! The engine runs on its caller's task and spawns nothing, at construction
//! or after: its transcriptions and syntheses are futures it polls itself.
//! The ticks are its clock for speech, and tokio's clock times the
//! acknowledgment, so a test with the clock paused runs in virtual time.

use std::collections::{BTreeMap, BTreeSet, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::stream::{FuturesUnordered, StreamExt as _};
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

use crate::audio::{Audio, FRAME};
use crate::io::{ClipId, Frame, Heard, Speaker, VoiceIo};
use crate::sentences::sentences;
use crate::speech::{Speech, SpeechError, Synthesis, Transcript, Usage};
use crate::vad::{Closed, Vad, VadSettings};

/// The engine's settings. [`Config::new`] gives the design's.
#[derive(Clone, Debug)]
pub struct Config {
    /// Whose audio is used. Anyone else's is dropped, never transcribed.
    pub listed: HashSet<Speaker>,
    /// The silence that ends an utterance: 700 ms (the stand-in VAD).
    pub end_of_utterance: Duration,
    /// How long a listed speaker talks over Theseus to stop it: 300 ms.
    pub barge_in: Duration,
    /// How long a turn runs before the acknowledgment: 2 s.
    pub acknowledge_after: Duration,
    /// The acknowledgment: a canned clip, so no synthesis and no cost.
    pub acknowledgment: Audio,
    /// The VAD's threshold: a frame whose RMS reaches it is speech.
    pub speech_rms: f64,
    /// Less speech than this is noise, not an utterance: 100 ms.
    pub min_speech: Duration,
    /// An utterance this long closes, even mid-speech: 30 s.
    pub max_utterance: Duration,
    /// What was received under the threshold just before an utterance opens
    /// begins it, a word's soft start: 200 ms.
    pub pre_roll: Duration,
}

impl Config {
    pub fn new(listed: impl IntoIterator<Item = Speaker>) -> Self {
        Self {
            listed: listed.into_iter().collect(),
            end_of_utterance: Duration::from_millis(700),
            barge_in: Duration::from_millis(300),
            acknowledge_after: Duration::from_secs(2),
            acknowledgment: Audio::chime(),
            speech_rms: 500.0,
            min_speech: Duration::from_millis(100),
            max_utterance: Duration::from_secs(30),
            pre_roll: Duration::from_millis(200),
        }
    }

    fn vad(&self) -> VadSettings {
        VadSettings {
            speech_rms: self.speech_rms,
            quiet_frames: frames(self.end_of_utterance),
            min_speech_frames: frames(self.min_speech),
            max_frames: frames(self.max_utterance),
            pre_roll_frames: frames(self.pre_roll),
        }
    }
}

/// `length` in whole frames, rounded up.
fn frames(length: Duration) -> u32 {
    length.as_nanos().div_ceil(FRAME.as_nanos()) as u32
}

/// The call's time at the start of tick `tick`.
fn at_tick(tick: u64) -> Duration {
    Duration::from_millis(tick * FRAME.as_millis() as u64)
}

/// A turn, numbered from 0 in each call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TurnId(pub u64);

/// What Theseus says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spoken {
    Reply(TurnId),
    Report,
    Acknowledgment,
}

/// What the session tells the engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// The reply to `turn`, spoken a sentence at a time. It ends the turn; an
    /// empty reply ends it in silence.
    Reply { turn: TurnId, text: String },
    /// A task's report, spoken at the next pause.
    Report { text: String },
    /// Stop speaking, and end the run.
    Leave,
}

/// An utterance, transcribed: a user message by its speaker (🎙️).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Utterance {
    pub speaker: Speaker,
    /// When its audio began, from the call's start: its first speech frame,
    /// or the soft start received just before it (`Config::pre_roll`).
    pub started: Duration,
    /// From there to the end of its last speech frame.
    pub length: Duration,
    /// When it closed: after the silence that ended it.
    pub closed: Duration,
    pub text: String,
    pub usage: Usage,
    /// How long its transcription took.
    pub latency: Duration,
}

/// A call to the speech provider that failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// An utterance's transcription: the utterance is dropped.
    Transcribe(Speaker),
    /// A sentence's synthesis: the sentence is skipped.
    Synthesize(Spoken),
}

/// What the engine tells the session, the ledger, and the observatory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// An utterance closed and was transcribed (`voice.utterance`, `speech.stt`).
    Utterance(Utterance),
    /// A turn for the session, answered with [`Command::Reply`]: its
    /// utterances in the order they closed.
    Turn {
        id: TurnId,
        utterances: Vec<Utterance>,
    },
    /// The acknowledgment began: `turn` ran past its time.
    Acknowledged {
        turn: TurnId,
    },
    /// A sentence was synthesized (`speech.tts`), even one a barge-in dropped
    /// while it was being made: it cost the same.
    Synthesized {
        what: Spoken,
        usage: Usage,
        latency: Duration,
    },
    /// A reply's or a report's first audio began, `first_audio` after it was
    /// asked for.
    Speaking {
        what: Spoken,
        sentences: usize,
        first_audio: Duration,
    },
    /// The last of it was played.
    Spoke {
        what: Spoken,
    },
    /// A listed speaker talked over Theseus (`voice.barge_in`): the clip
    /// stopped, and `dropped` sentences went unsaid, the one playing among them.
    BargeIn {
        speaker: Speaker,
        what: Spoken,
        dropped: usize,
    },
    /// An unlisted speaker's audio is being dropped, untranscribed (once a
    /// speaker).
    Unlisted {
        speaker: Speaker,
    },
    Failed {
        what: Failure,
        error: SpeechError,
    },
}

/// The session's side of a running engine.
pub struct EngineHandle {
    pub commands: mpsc::UnboundedSender<Command>,
    pub events: mpsc::UnboundedReceiver<Event>,
}

/// The pipeline for one call. [`Engine::new`] builds it, and the caller
/// runs [`Engine::run`] on a task of its own at join time.
pub struct Engine {
    config: Config,
    vad: VadSettings,
    barge_frames: u32,
    io: Box<dyn VoiceIo>,
    speech: Arc<dyn Speech>,
    commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
    ticks: u64,
    // Receive, and turns.
    vads: BTreeMap<Speaker, Vad>,
    unlisted: BTreeSet<Speaker>,
    pending: VecDeque<Pending>,
    next_utterance: u64,
    turn: Option<Turn>,
    next_turn: u64,
    reports: VecDeque<String>,
    // Send.
    queue: VecDeque<Item>,
    next_item: u64,
    playing: Option<ClipId>,
    stopping: bool,
    next_clip: u64,
    synthesizing: bool,
    generation: u64,
    talk_frames: BTreeMap<Speaker, u32>,
    work: FuturesUnordered<BoxFuture<'static, Done>>,
    outbox: Vec<Out>,
}

/// A closed utterance, until its turn starts.
struct Pending {
    seq: u64,
    speaker: Speaker,
    started: Duration,
    length: Duration,
    closed: Duration,
    state: Transcribed,
}

enum Transcribed {
    Waiting,
    Got(Utterance),
    Dropped,
}

struct Turn {
    id: TurnId,
    started: Instant,
    ack: Ack,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ack {
    Waiting,
    Due,
    Done,
}

/// A sentence or clip to say, in order. The queue's head may be playing.
struct Item {
    seq: u64,
    what: Spoken,
    /// The sentence to synthesize; `None` for a canned clip.
    text: Option<String>,
    audio: Option<Audio>,
    clip: Option<ClipId>,
    /// On a reply's or report's first item: its sentences, and when it was
    /// asked for.
    first: Option<(usize, Instant)>,
}

enum Done {
    Transcribed {
        seq: u64,
        result: Result<Transcript, SpeechError>,
        latency: Duration,
    },
    Synthesized {
        generation: u64,
        seq: u64,
        what: Spoken,
        result: Result<Synthesis, SpeechError>,
        latency: Duration,
    },
}

/// A call into the `VoiceIo`, made after the event that asked for it.
enum Out {
    Play(ClipId, Audio),
    Stop,
}

enum Step {
    Done(Done),
    Command(Option<Command>),
    Heard(Option<Heard>),
    AckDue,
}

impl Engine {
    /// The engine for a call. It spawns nothing (FAST: no voice work before
    /// serving, design §2.8).
    pub fn new(
        config: Config,
        io: Box<dyn VoiceIo>,
        speech: Arc<dyn Speech>,
    ) -> (Self, EngineHandle) {
        let (command_tx, commands) = mpsc::unbounded_channel();
        let (events, event_rx) = mpsc::unbounded_channel();
        let engine = Self {
            vad: config.vad(),
            barge_frames: frames(config.barge_in).max(1),
            config,
            io,
            speech,
            commands,
            events,
            ticks: 0,
            vads: BTreeMap::new(),
            unlisted: BTreeSet::new(),
            pending: VecDeque::new(),
            next_utterance: 0,
            turn: None,
            next_turn: 0,
            reports: VecDeque::new(),
            queue: VecDeque::new(),
            next_item: 0,
            playing: None,
            stopping: false,
            next_clip: 0,
            synthesizing: false,
            generation: 0,
            talk_frames: BTreeMap::new(),
            work: FuturesUnordered::new(),
            outbox: Vec::new(),
        };
        let handle = EngineHandle {
            commands: command_tx,
            events: event_rx,
        };
        (engine, handle)
    }

    /// Run the call until [`Command::Leave`], the handle's drop, or the
    /// call's end.
    pub async fn run(mut self) {
        loop {
            let ack_at = self
                .turn
                .as_ref()
                .filter(|t| t.ack == Ack::Waiting)
                .map(|t| t.started + self.config.acknowledge_after);
            let step = tokio::select! {
                biased;
                Some(done) = self.work.next(), if !self.work.is_empty() => Step::Done(done),
                command = self.commands.recv() => Step::Command(command),
                heard = self.io.next() => Step::Heard(heard),
                () = sleep_until(ack_at.unwrap_or_else(Instant::now)), if ack_at.is_some() => {
                    Step::AckDue
                }
            };
            let now = Instant::now();
            match step {
                Step::Done(done) => self.done(done),
                Step::Command(Some(Command::Reply { turn, text })) => self.reply(turn, &text, now),
                Step::Command(Some(Command::Report { text })) => self.reports.push_back(text),
                Step::Command(None | Some(Command::Leave)) | Step::Heard(None) => break,
                Step::Heard(Some(Heard::Tick(frames))) => self.tick(&frames),
                Step::Heard(Some(Heard::Ended(id))) => self.ended(id),
                Step::AckDue => {
                    if let Some(turn) = &mut self.turn {
                        turn.ack = Ack::Due;
                    }
                }
            }
            self.advance(now);
            for out in std::mem::take(&mut self.outbox) {
                match out {
                    Out::Play(id, clip) => self.io.play(id, clip).await,
                    Out::Stop => self.io.stop().await,
                }
            }
        }
        if self.playing.is_some() {
            self.io.stop().await;
        }
    }

    fn emit(&self, event: Event) {
        // A session that stopped listening doesn't stop the call.
        let _ = self.events.send(event);
    }

    /// One 20 ms tick: listed speakers' frames into their VADs; anyone
    /// else's dropped.
    fn tick(&mut self, frames: &[Frame]) {
        let tick = self.ticks;
        self.ticks += 1;
        let mut heard: BTreeMap<Speaker, &[i16]> = BTreeMap::new();
        for frame in frames {
            if self.config.listed.contains(&frame.speaker) {
                heard.insert(frame.speaker, &frame.samples);
            } else if self.unlisted.insert(frame.speaker) {
                self.emit(Event::Unlisted {
                    speaker: frame.speaker,
                });
            }
        }
        let talking = self.playing.is_some() && !self.stopping;
        let open = self
            .vads
            .iter()
            .filter(|(_, v)| v.is_open())
            .map(|(s, _)| *s);
        let speakers: BTreeSet<Speaker> = heard.keys().copied().chain(open).collect();
        let mut barge = None;
        for speaker in speakers {
            let frame = heard.get(&speaker).copied();
            let step = self
                .vads
                .entry(speaker)
                .or_default()
                .push(tick, frame, &self.vad);
            if step.speech && talking {
                let n = self.talk_frames.entry(speaker).or_default();
                *n += 1;
                if *n >= self.barge_frames && barge.is_none() {
                    barge = Some(speaker);
                }
            }
            match step.closed {
                Some(Closed::Utterance { first_tick, audio }) => {
                    self.talk_frames.remove(&speaker);
                    self.transcribe(speaker, first_tick, tick, audio);
                }
                Some(Closed::Noise) => {
                    self.talk_frames.remove(&speaker);
                }
                None => {}
            }
        }
        if let Some(speaker) = barge {
            self.barge_in(speaker);
        }
    }

    fn transcribe(&mut self, speaker: Speaker, first_tick: u64, tick: u64, audio: Audio) {
        let seq = self.next_utterance;
        self.next_utterance += 1;
        self.pending.push_back(Pending {
            seq,
            speaker,
            started: at_tick(first_tick),
            length: audio.duration(),
            closed: at_tick(tick + 1),
            state: Transcribed::Waiting,
        });
        let speech = Arc::clone(&self.speech);
        self.work.push(Box::pin(async move {
            let asked = Instant::now();
            let result = speech.transcribe(speaker, &audio).await;
            Done::Transcribed {
                seq,
                result,
                latency: asked.elapsed(),
            }
        }));
    }

    fn barge_in(&mut self, speaker: Speaker) {
        let Some(what) = self.queue.front().map(|i| i.what) else {
            return;
        };
        let dropped = self.queue.len();
        self.queue.clear();
        self.synthesizing = false;
        self.generation += 1;
        self.talk_frames.clear();
        if self.playing.is_some() {
            // `playing` stays set until the clip's `Ended`, so nothing new
            // starts over it.
            self.stopping = true;
            self.outbox.push(Out::Stop);
        }
        self.emit(Event::BargeIn {
            speaker,
            what,
            dropped,
        });
    }

    fn ended(&mut self, id: ClipId) {
        if self.playing != Some(id) {
            return;
        }
        self.playing = None;
        self.stopping = false;
        if self.queue.front().is_some_and(|i| i.clip == Some(id)) {
            if let Some(item) = self.queue.pop_front() {
                if !self.queue.iter().any(|i| i.what == item.what) {
                    self.emit(Event::Spoke { what: item.what });
                }
            }
        }
        if self.queue.is_empty() {
            self.talk_frames.clear();
        }
    }

    fn done(&mut self, done: Done) {
        match done {
            Done::Transcribed {
                seq,
                result,
                latency,
            } => {
                let Some(p) = self.pending.iter_mut().find(|p| p.seq == seq) else {
                    return;
                };
                let event = match result {
                    Ok(t) => {
                        let utterance = Utterance {
                            speaker: p.speaker,
                            started: p.started,
                            length: p.length,
                            closed: p.closed,
                            text: t.text,
                            usage: t.usage,
                            latency,
                        };
                        p.state = if utterance.text.trim().is_empty() {
                            Transcribed::Dropped
                        } else {
                            Transcribed::Got(utterance.clone())
                        };
                        Event::Utterance(utterance)
                    }
                    Err(error) => {
                        p.state = Transcribed::Dropped;
                        Event::Failed {
                            what: Failure::Transcribe(p.speaker),
                            error,
                        }
                    }
                };
                self.emit(event);
            }
            Done::Synthesized {
                generation,
                seq,
                what,
                result,
                latency,
            } => {
                let current = generation == self.generation;
                if current {
                    self.synthesizing = false;
                }
                let at = self
                    .queue
                    .iter()
                    .position(|i| i.seq == seq)
                    .filter(|_| current);
                match result {
                    Ok(synthesis) => {
                        self.emit(Event::Synthesized {
                            what,
                            usage: synthesis.usage,
                            latency,
                        });
                        if let Some(at) = at {
                            self.queue[at].audio = Some(synthesis.audio);
                        }
                    }
                    Err(error) => {
                        self.emit(Event::Failed {
                            what: Failure::Synthesize(what),
                            error,
                        });
                        if let Some(item) = at.and_then(|at| self.queue.remove(at)) {
                            // Its successor in the same reply takes its place.
                            if let Some(next) = self.queue.iter_mut().find(|i| i.what == item.what)
                            {
                                next.first = next.first.or(item.first);
                            } else if item.first.is_none() {
                                self.emit(Event::Spoke { what: item.what });
                            }
                        }
                    }
                }
            }
        }
    }

    fn reply(&mut self, turn: TurnId, text: &str, now: Instant) {
        if self.turn.as_ref().is_some_and(|t| t.id == turn) {
            self.turn = None;
        }
        self.enqueue(Spoken::Reply(turn), sentences(text), now);
    }

    fn enqueue(&mut self, what: Spoken, sentences: Vec<String>, now: Instant) {
        let count = sentences.len();
        for (i, text) in sentences.into_iter().enumerate() {
            self.queue.push_back(Item {
                seq: self.next_item,
                what,
                text: Some(text),
                audio: None,
                clip: None,
                first: (i == 0).then_some((count, now)),
            });
            self.next_item += 1;
        }
    }

    /// After every event: start a turn, the acknowledgment, or a report;
    /// synthesize the next sentence; play the next clip.
    fn advance(&mut self, now: Instant) {
        self.start_turn(now);
        let quiet = self.playing.is_none()
            && self.queue.is_empty()
            && !self.vads.values().any(Vad::is_open);
        if quiet {
            let due = self.turn.as_mut().filter(|t| t.ack == Ack::Due);
            if let Some(turn) = due {
                turn.ack = Ack::Done;
                let id = turn.id;
                self.queue.push_back(Item {
                    seq: self.next_item,
                    what: Spoken::Acknowledgment,
                    text: None,
                    audio: Some(self.config.acknowledgment.clone()),
                    clip: None,
                    first: Some((1, now)),
                });
                self.next_item += 1;
                self.emit(Event::Acknowledged { turn: id });
            } else if self.turn.is_none() && self.pending.is_empty() {
                // A pause: no turn in flight or waiting, nobody speaking,
                // nothing said or to say.
                if let Some(text) = self.reports.pop_front() {
                    self.enqueue(Spoken::Report, sentences(&text), now);
                }
            }
        }
        self.synthesize_next();
        self.play_next(now);
    }

    /// When no turn is in flight: the transcribed utterances at the head of
    /// the line, in the order they closed, become the next turn.
    fn start_turn(&mut self, now: Instant) {
        if self.turn.is_some() {
            return;
        }
        let mut utterances = Vec::new();
        while self
            .pending
            .front()
            .is_some_and(|p| !matches!(p.state, Transcribed::Waiting))
        {
            if let Some(Pending {
                state: Transcribed::Got(utterance),
                ..
            }) = self.pending.pop_front()
            {
                utterances.push(utterance);
            }
        }
        if utterances.is_empty() {
            return;
        }
        let id = TurnId(self.next_turn);
        self.next_turn += 1;
        self.turn = Some(Turn {
            id,
            started: now,
            ack: Ack::Waiting,
        });
        self.emit(Event::Turn { id, utterances });
    }

    /// Synthesize the first sentence without audio: one at a time, and at
    /// most one ready ahead of the clip that's playing.
    fn synthesize_next(&mut self) {
        if self.synthesizing {
            return;
        }
        let ready = self.queue.iter().take_while(|i| i.audio.is_some()).count();
        if ready > 1 {
            return;
        }
        let Some(item) = self.queue.iter().find(|i| i.audio.is_none()) else {
            return;
        };
        let Some(text) = item.text.clone() else {
            return;
        };
        let (seq, what, generation) = (item.seq, item.what, self.generation);
        let speech = Arc::clone(&self.speech);
        self.synthesizing = true;
        self.work.push(Box::pin(async move {
            let asked = Instant::now();
            let result = speech.synthesize(&text).await;
            Done::Synthesized {
                generation,
                seq,
                what,
                result,
                latency: asked.elapsed(),
            }
        }));
    }

    fn play_next(&mut self, now: Instant) {
        if self.playing.is_some() {
            return;
        }
        let id = ClipId(self.next_clip);
        let Some(item) = self.queue.front_mut() else {
            return;
        };
        let Some(audio) = item.audio.clone() else {
            return;
        };
        item.clip = Some(id);
        let first = item.first.take().map(|(sentences, asked)| Event::Speaking {
            what: item.what,
            sentences,
            first_audio: now - asked,
        });
        self.next_clip += 1;
        self.playing = Some(id);
        self.outbox.push(Out::Play(id, audio));
        if let Some(event) = first {
            self.emit(event);
        }
    }
}
