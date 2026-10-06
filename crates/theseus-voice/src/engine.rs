//! The pipeline (design §2.8). One [`Engine`] runs one call.
//!
//! - **Receive.** Each listed speaker's frames go through a VAD of their own;
//!   an utterance closes after 700 ms of silence and is transcribed. Anyone
//!   else's audio is dropped at the door, never transcribed.
//! - **Turns.** A transcribed utterance with words starts a turn when none
//!   is in flight. Utterances that close during a turn coalesce into the next
//!   one, in the order they closed, each keeping its speaker. Each carries
//!   what Theseus was doing at its first speech frame (`over`) and what it
//!   was heard as (`heard_as`, by the rules in `heard.rs`): over Theseus's
//!   speech, a sound with no words, Theseus's own sentence heard back, a
//!   "yeah", or a "go on" is no turn.
//! - **Send.** A reply is made speakable (`speakable`: no markdown, a table
//!   or code block one sentence that points to the text channel, theseus-rkvl)
//!   and split at sentence ends, and synthesized one sentence at a time, one ahead of what's playing, so its first audio starts after
//!   its first sentence. A reply or report begins only on the floor: while a
//!   listed speaker talks it waits, and if their utterance is words it is
//!   superseded (a `Cut`) and their words are the next turn. So is a reply
//!   whose speaker went on within 1.5 s of their last word in its turn (a
//!   thought split by a pause).
//! - **Barge-in.** A listed speaker who talks for 300 ms over Theseus stops
//!   the clip at once, and holds the queue: nothing plays or is synthesized.
//!   The words over it decide. No words, an echo, a "yeah", or a "go on"
//!   resume the cut sentence from its start, from the audio it held
//!   (`Resumed`); words commit the cut (a `Cut` for each reply or report,
//!   then `BargeIn`) and are the next turn. Words too short to stop it, or
//!   from a speaker heard echoing twice (whose stop is off for the call),
//!   cut at their transcript; so does a failed transcription, and a hold's
//!   transcript still due 3 s after its utterance closed (`transcript_bound`,
//!   theseus-aq4t), which is then dropped. A report cut
//!   by words comes back at the next pause from its cut sentence; one still
//!   waiting when the call ends is cut there (theseus-qrwx). The session
//!   still has the whole text.
//! - **The floor's bound.** A reply that has waited 8 s for the floor, or a
//!   hold for its utterances to close, held only by open utterances, has each
//!   one's audio so far transcribed once (`floor_bound`, theseus-aq4t): a
//!   person's long sentence is words and still waited for; a fan's, music's or
//!   a TV's sound heard as none no longer holds the floor, and its stop waits
//!   for its words, as an echo-prone speaker's does.
//! - A turn that runs past 2 s gets the canned acknowledgment, once, when the
//!   line is quiet. A task's report waits for the next pause.
//!
//! The engine runs on its caller's task and spawns nothing, at construction
//! or after: its transcriptions and syntheses are futures it polls itself.
//! The ticks are its clock for speech, and tokio's clock times the
//! acknowledgment, so a test with the clock paused runs in virtual time.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use futures_util::stream::{FuturesUnordered, StreamExt as _};
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

use crate::audio::{Audio, FRAME};
use crate::heard::{classify, HeardAs, Overlap};
use crate::io::{ClipId, Frame, Heard, Speaker, VoiceIo};
use crate::sentences::speakable;
use crate::speech::{Speech, SpeechError, Synthesis, Transcript, Usage, ACKNOWLEDGMENT};
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
    /// How long a hold waits for the transcripts of the utterances over it
    /// after the last of them closed, before it decides as if they failed:
    /// 3 s (theseus-aq4t).
    pub transcript_bound: Duration,
    /// How long a reply waits for a floor held by an open utterance, or a
    /// hold for one to close, before the utterance's audio so far is
    /// transcribed once: heard as no words, it no longer holds the floor,
    /// and its stop waits for its words: 8 s (theseus-aq4t).
    pub floor_bound: Duration,
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
            transcript_bound: Duration::from_secs(3),
            floor_bound: Duration::from_secs(8),
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    /// What Theseus was doing when its first speech frame came (not its
    /// pre-roll): saying a sentence, or preparing a turn's reply. `None`
    /// when it was neither.
    pub over: Option<Over>,
    /// What it was heard as. Only words make a turn.
    pub heard_as: HeardAs,
}

/// What Theseus was doing when an utterance began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Over {
    /// Saying a reply's, a report's or the acknowledgment's sentence (or
    /// holding it, or about to say it, in a gap between sentences still
    /// queued): its index in what it belongs to, and its text.
    Saying {
        what: Spoken,
        sentence: usize,
        text: String,
    },
    /// Preparing the reply to a turn in flight whose reply hadn't begun.
    Preparing { turn: TurnId },
}

/// Why what Theseus was saying was cut short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutWhy {
    /// A listed speaker's words over it: a barge-in.
    Words,
    /// It hadn't begun, and its speaker talked past it: the reply waited for
    /// them, and their words are the next turn.
    Superseded,
    /// The call ended (`Leave`, or the connection's end) with it unsaid.
    CallEnded,
}

/// What failed: a call to the speech provider, or the call itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// An utterance's transcription: the utterance is dropped.
    Transcribe(Speaker),
    /// A sentence's synthesis: the sentence is skipped.
    Synthesize(Spoken),
    /// The voice connection dropped (44b): the run ends, and the error says
    /// why, as the seam gave it (`VoiceIo::gone`).
    Connection,
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
    /// A listed speaker's words cut Theseus short (`voice.barge_in`): the
    /// clip stopped, and `dropped` sentences went unsaid, the one cut among
    /// them. It comes when the words are known: at the transcript of the
    /// utterance that stopped it, or that talked over it too briefly to stop
    /// it (the late cut).
    BargeIn {
        speaker: Speaker,
        what: Spoken,
        dropped: usize,
    },
    /// A stop that came to nothing: the utterances over the held speech
    /// were `why` (wordless, echo, backchannel or resume), and after `held`
    /// the cut sentence began again from its start, from the audio it held.
    Resumed {
        what: Spoken,
        why: HeardAs,
        held: Duration,
    },
    /// A reply or a report cut short, one event for each (an acknowledgment
    /// isn't one). Of its `sentences`, the first `heard` played whole; the
    /// next was stopped `into` its audio (zero when it never started).
    /// `last_heard` is the last whole sentence's text (`None` when `heard`
    /// is 0), and `cut` the text of the one cut.
    Cut {
        what: Spoken,
        why: CutWhy,
        sentences: usize,
        heard: usize,
        into: Duration,
        last_heard: Option<String>,
        cut: String,
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
    /// Each open utterance's start: what it was over.
    opening: BTreeMap<Speaker, Opening>,
    unlisted: BTreeSet<Speaker>,
    pending: VecDeque<Pending>,
    next_utterance: u64,
    turn: Option<Turn>,
    next_turn: u64,
    reports: VecDeque<QueuedReport>,
    // Send.
    queue: VecDeque<Item>,
    next_item: u64,
    playing: Option<ClipId>,
    stopping: bool,
    next_clip: u64,
    synthesizing: bool,
    generation: u64,
    talk_frames: BTreeMap<Speaker, u32>,
    /// A stop that waits for the words over it.
    hold: Option<Hold>,
    /// The sentences that played to their end, in the echo tail.
    recent: VecDeque<(Instant, String)>,
    /// The last sentence heard whole of each reply or report still queued.
    said: HashMap<Spoken, String>,
    /// How many echoes each speaker was heard giving. From the second, they
    /// are echo-prone: their stop waits for words.
    echoes: BTreeMap<Speaker, u32>,
    /// Each queued reply that hasn't begun: when its turn's speakers last
    /// spoke in it.
    split: HashMap<TurnId, BTreeMap<Speaker, Duration>>,
    /// Each open utterance's number, from 0 in each call.
    next_opening: u64,
    /// A speaker whose sound, heard as no words, closed at the VAD's
    /// maximum, and the tick it closed on: the sound that opens again on the
    /// next tick is the same one.
    steady: BTreeMap<Speaker, u64>,
    work: FuturesUnordered<BoxFuture<'static, Done>>,
    outbox: Vec<Out>,
}

/// How long after a sentence ends an utterance may still be its echo.
const ECHO_TAIL: Duration = Duration::from_millis(1200);

/// An utterance begun this soon after the sentence playing began may echo
/// its head from 2 words (theseus-j2ut).
const ECHO_HEAD: Duration = Duration::from_millis(500);

/// A speaker heard echoing this many times in a call is echo-prone: one
/// verdict that was wrong doesn't take their stop away (theseus-3ug0).
const ECHO_PRONE: u32 = 2;

/// An utterance by one of a turn's speakers that began this soon after
/// their last speech in it goes on the same thought: the turn's reply waits
/// for its words, and they supersede it.
const SPLIT_THOUGHT: Duration = Duration::from_millis(1500);

/// An open utterance's start: what Theseus was doing at its first speech
/// frame, and the sentences it may echo.
struct Opening {
    over: Option<Over>,
    overlap: Overlap,
    sentences: Vec<String>,
    /// It began over the last sentence queued, which had begun: if the queue
    /// is empty when it closes, it answers what was said (theseus-1cz8).
    last: bool,
    /// The sentence playing, begun at most `ECHO_HEAD` before it: a run
    /// from its head is an echo from 2 words (theseus-j2ut).
    head: Option<String>,
    /// Its number, so a probe of its audio so far finds it.
    id: u64,
    /// What the floor's bound heard of it so far.
    sound: Sound,
}

/// What the floor's bound heard of an open utterance (theseus-aq4t).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sound {
    /// Not asked.
    Unheard,
    /// Its audio so far is being transcribed.
    Asked,
    /// Words, or a probe that failed: it holds the floor, as any utterance.
    Words,
    /// No words: a fan, music, a TV. It doesn't hold the floor, and its
    /// stop waits for its words, as an echo-prone speaker's does.
    Wordless,
}

/// A closed utterance, until its turn starts.
struct Pending {
    seq: u64,
    speaker: Speaker,
    started: Duration,
    length: Duration,
    closed: Duration,
    opening: Opening,
    state: Transcribed,
}

enum Transcribed {
    Waiting,
    Got(Box<Utterance>),
    Dropped,
}

struct Turn {
    id: TurnId,
    started: Instant,
    ack: Ack,
    /// When each of its speakers' last speech in it ended.
    last_speech: BTreeMap<Speaker, Duration>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Ack {
    Waiting,
    Due,
    Done,
}

/// A report to say at a pause, as its sentences: from `first` of `count`,
/// so one cut by words comes back from its cut sentence, split once.
struct QueuedReport {
    sentences: Vec<String>,
    first: usize,
    count: usize,
}

/// A stop that came at 300 ms of speech, held until the words over it
/// decide: resume, or commit the cut.
struct Hold {
    since: Instant,
    /// When it began, from the call's start.
    at: Duration,
    /// The item that was playing, and how long it had played.
    seq: u64,
    what: Spoken,
    into: Duration,
    /// What the utterances over it were heard as, the plainest reason.
    why: Option<HeardAs>,
}

/// A sentence or clip to say, in order. The queue's head may be playing.
struct Item {
    seq: u64,
    what: Spoken,
    /// The sentence: synthesized unless `audio` came with it (a canned clip).
    text: String,
    /// Its index in `what`, and `what`'s sentences.
    index: usize,
    count: usize,
    audio: Option<Audio>,
    clip: Option<ClipId>,
    /// When its clip started.
    started: Option<Instant>,
    /// On a reply's or report's first item, until it plays: when it was
    /// asked for.
    first: Option<Instant>,
    /// When it was queued, from the call's start.
    asked: Duration,
    /// It begins what it belongs to (or a report back from a cut), and
    /// hasn't played: it waits for the floor.
    opens: bool,
}

enum Done {
    /// The floor's bound: an open utterance's audio so far, transcribed.
    Probed {
        speaker: Speaker,
        id: u64,
        result: Result<Transcript, SpeechError>,
    },
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
            opening: BTreeMap::new(),
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
            hold: None,
            recent: VecDeque::new(),
            said: HashMap::new(),
            echoes: BTreeMap::new(),
            split: HashMap::new(),
            next_opening: 0,
            steady: BTreeMap::new(),
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
                Step::Done(done) => self.done(done, now),
                Step::Command(Some(Command::Reply { turn, text })) => self.reply(turn, &text, now),
                Step::Command(Some(Command::Report { text })) => {
                    let sentences = speakable(&text);
                    self.reports.push_back(QueuedReport {
                        count: sentences.len(),
                        first: 0,
                        sentences,
                    });
                }
                Step::Command(None | Some(Command::Leave)) => {
                    self.call_ended(now);
                    break;
                }
                Step::Heard(None) => {
                    self.call_ended(now);
                    // A dropped connection says so; a call that just ended
                    // (a test's WAV running out) does not.
                    if let Some(why) = self.io.gone() {
                        self.emit(Event::Failed {
                            what: Failure::Connection,
                            error: SpeechError(why),
                        });
                    }
                    break;
                }
                Step::Heard(Some(Heard::Tick(frames))) => self.tick(&frames, now),
                Step::Heard(Some(Heard::Ended(id))) => self.ended(id, now),
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
    fn tick(&mut self, frames: &[Frame], now: Instant) {
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
        let talking = self.playing.is_some() && !self.stopping && self.hold.is_none();
        let open = self
            .vads
            .iter()
            .filter(|(_, v)| v.is_open())
            .map(|(s, _)| *s);
        let speakers: BTreeSet<Speaker> = heard.keys().copied().chain(open).collect();
        let mut barge = false;
        for speaker in speakers {
            let frame = heard.get(&speaker).copied();
            let vad = self.vads.entry(speaker).or_default();
            let was_open = vad.is_open();
            let step = vad.push(tick, frame, &self.vad);
            if !was_open && self.vads.get(&speaker).is_some_and(Vad::is_open) {
                let mut opening = self.what_is_over(now);
                // A sound heard as no words, open again on the tick after
                // the VAD's maximum closed it, is the same sound.
                if self.steady.remove(&speaker).is_some_and(|t| t + 1 == tick) {
                    opening.sound = Sound::Wordless;
                }
                self.opening.insert(speaker, opening);
            }
            // An echo-prone speaker's stop waits for their words, and so
            // does a sound heard as none (theseus-aq4t).
            let prone = self.echoes.get(&speaker).is_some_and(|n| *n >= ECHO_PRONE)
                || self
                    .opening
                    .get(&speaker)
                    .is_some_and(|o| o.sound == Sound::Wordless);
            if step.speech && talking && !prone {
                let n = self.talk_frames.entry(speaker).or_default();
                *n += 1;
                barge |= *n >= self.barge_frames;
            }
            match step.closed {
                Some(Closed::Utterance { first_tick, audio }) => {
                    self.talk_frames.remove(&speaker);
                    let mut opening = self.opening.remove(&speaker).unwrap_or(Opening {
                        over: None,
                        overlap: Overlap::None,
                        sentences: Vec::new(),
                        last: false,
                        head: None,
                        id: 0,
                        sound: Sound::Unheard,
                    });
                    if step.speech && opening.sound == Sound::Wordless {
                        self.steady.insert(speaker, tick);
                    }
                    // A "yes" begun on a question's last word, closed after
                    // it ended: the tail's rules, as if begun after it.
                    if opening.last && self.queue.is_empty() {
                        opening.overlap = Overlap::Tail;
                    }
                    self.transcribe(speaker, first_tick, tick, audio, opening);
                }
                Some(Closed::Noise) => {
                    self.talk_frames.remove(&speaker);
                    self.opening.remove(&speaker);
                }
                None => {}
            }
        }
        if barge {
            self.hold(now);
        }
    }

    /// What an utterance whose first speech frame comes now is over.
    fn what_is_over(&mut self, now: Instant) -> Opening {
        while self
            .recent
            .front()
            .is_some_and(|(ended, _)| now.saturating_duration_since(*ended) > ECHO_TAIL)
        {
            self.recent.pop_front();
        }
        let mut sentences: Vec<String> = self.recent.iter().map(|(_, t)| t.clone()).collect();
        let tail = match self.recent.is_empty() {
            true => Overlap::None,
            false => Overlap::Tail,
        };
        let mut last = false;
        let mut head = None;
        let (over, overlap) = match self.queue.front() {
            Some(item) => {
                if item.clip.is_some() || self.hold.is_some() {
                    sentences.push(item.text.clone());
                }
                let begun = item.started.filter(|_| item.clip.is_some());
                if begun.is_some_and(|s| now.saturating_duration_since(s) <= ECHO_HEAD) {
                    head = Some(item.text.clone());
                }
                let over = Over::Saying {
                    what: item.what,
                    sentence: item.index,
                    text: item.text.clone(),
                };
                // A reply that hasn't begun is no speech to talk over: a
                // "yeah" now answers what was said (theseus-1cz8).
                if item.opens && self.hold.is_none() {
                    (Some(over), tail)
                } else {
                    last = self.queue.len() == 1;
                    (Some(over), Overlap::Speech)
                }
            }
            None => {
                let over = self.turn.as_ref().map(|t| Over::Preparing { turn: t.id });
                (over, tail)
            }
        };
        let id = self.next_opening;
        self.next_opening += 1;
        Opening {
            over,
            overlap,
            sentences,
            last,
            head,
            id,
            sound: Sound::Unheard,
        }
    }

    fn transcribe(
        &mut self,
        speaker: Speaker,
        first_tick: u64,
        tick: u64,
        audio: Audio,
        opening: Opening,
    ) {
        let seq = self.next_utterance;
        self.next_utterance += 1;
        self.pending.push_back(Pending {
            seq,
            speaker,
            started: at_tick(first_tick),
            length: audio.duration(),
            closed: at_tick(tick + 1),
            opening,
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

    /// A listed speaker's 300 ms over a clip: stop it, and hold the queue
    /// until the words over it decide.
    fn hold(&mut self, now: Instant) {
        let Some(item) = self.queue.front_mut() else {
            return;
        };
        let into = item
            .started
            .map_or(Duration::ZERO, |s| now.saturating_duration_since(s));
        // It keeps its place and its audio; its clip's `Ended` won't pop it.
        item.clip = None;
        self.hold = Some(Hold {
            since: now,
            at: at_tick(self.ticks),
            seq: item.seq,
            what: item.what,
            into,
            why: None,
        });
        self.talk_frames.clear();
        if self.playing.is_some() {
            // `playing` stays set until the clip's `Ended`, so nothing new
            // starts over it.
            self.stopping = true;
            self.outbox.push(Out::Stop);
        }
    }

    /// The hold resolves to a resume once every utterance over speech is
    /// heard, none of them words, and nobody is speaking.
    fn resume(&mut self, now: Instant) {
        if self.hold.is_none()
            || self.floor_held()
            || self.pending.iter().any(|p| {
                p.opening.overlap == Overlap::Speech && matches!(p.state, Transcribed::Waiting)
            })
        {
            return;
        }
        let Some(hold) = self.hold.take() else {
            return;
        };
        // A cut acknowledgment isn't said again.
        if hold.what == Spoken::Acknowledgment {
            self.queue.retain(|i| i.seq != hold.seq);
        }
        self.talk_frames.clear();
        self.emit(Event::Resumed {
            what: hold.what,
            why: hold.why.unwrap_or(HeardAs::Wordless),
            held: now.saturating_duration_since(hold.since),
        });
    }

    /// The hold's bound (theseus-aq4t): when the transcripts of the
    /// utterances over a hold are still due `transcript_bound` after the last
    /// of them closed, they decide as if they failed. Each is dropped with
    /// its `Failed`, and the cut is committed, as a failure's is: a stop that
    /// wasn't heard must not be talked over. A transcript that comes later is
    /// dropped (`heard`).
    fn overdue(&mut self, now: Instant) {
        if self.hold.is_none() {
            return;
        }
        let due = |p: &Pending| {
            p.opening.overlap == Overlap::Speech && matches!(p.state, Transcribed::Waiting)
        };
        let Some(last) = self
            .pending
            .iter()
            .filter(|p| due(p))
            .map(|p| p.closed)
            .max()
        else {
            return;
        };
        let bound = self.config.transcript_bound;
        if at_tick(self.ticks) < last + bound {
            return;
        }
        let mut late = Vec::new();
        for p in self.pending.iter_mut().filter(|p| due(p)) {
            p.state = Transcribed::Dropped;
            late.push(p.speaker);
        }
        for speaker in &late {
            self.emit(Event::Failed {
                what: Failure::Transcribe(*speaker),
                error: SpeechError(format!(
                    "no transcript {} ms after the utterance closed",
                    bound.as_millis()
                )),
            });
        }
        if let Some(speaker) = late.first() {
            self.commit(*speaker, now);
        }
    }

    /// Someone holds the floor: an open utterance not heard as a sound with
    /// no words.
    fn floor_held(&self) -> bool {
        self.vads.iter().any(|(speaker, vad)| {
            vad.is_open()
                && !self
                    .opening
                    .get(speaker)
                    .is_some_and(|o| o.sound == Sound::Wordless)
        })
    }

    /// The floor's bound (theseus-aq4t): a reply that has waited
    /// `floor_bound` for the floor, or a hold that has, held only by open
    /// utterances, has each one's audio so far transcribed once. A person's
    /// long sentence is words, and still waited for; a fan's or music's
    /// sound is none, and no longer holds the floor (`probed`).
    fn probe(&mut self) {
        let since = match (&self.hold, self.queue.front()) {
            (Some(hold), _) => hold.at,
            (None, Some(front)) if front.opens && self.playing.is_none() => front.asked,
            _ => return,
        };
        if at_tick(self.ticks) < since + self.config.floor_bound {
            return;
        }
        // Words still due decide first.
        let due = self.pending.iter().any(|p| {
            matches!(p.state, Transcribed::Waiting)
                && match (&self.hold, self.queue.front()) {
                    (Some(_), _) => p.opening.overlap == Overlap::Speech,
                    (None, Some(front)) => self.contends(p, front),
                    (None, None) => false,
                }
        });
        if due {
            return;
        }
        for (speaker, vad) in &self.vads {
            // Once each: the copy of its audio so far is the probe's one cost
            // here.
            let Some(opening) = self
                .opening
                .get_mut(speaker)
                .filter(|o| o.sound == Sound::Unheard)
            else {
                continue;
            };
            let Some(audio) = vad.so_far() else {
                continue;
            };
            opening.sound = Sound::Asked;
            let (speaker, id) = (*speaker, opening.id);
            let speech = Arc::clone(&self.speech);
            self.work.push(Box::pin(async move {
                let result = speech.transcribe(speaker, &audio).await;
                Done::Probed {
                    speaker,
                    id,
                    result,
                }
            }));
        }
    }

    /// A probe's transcript: no words, and the open utterance it heard no
    /// longer holds the floor. A failed one holds it, as words do.
    fn probed(&mut self, speaker: Speaker, id: u64, result: Result<Transcript, SpeechError>) {
        let Some(opening) = self.opening.get_mut(&speaker).filter(|o| o.id == id) else {
            return;
        };
        opening.sound = match &result {
            Ok(t) if classify(&t.text, Overlap::None, &[], None) == HeardAs::Wordless => {
                Sound::Wordless
            }
            _ => Sound::Words,
        };
        if let Err(error) = result {
            self.emit(Event::Failed {
                what: Failure::Transcribe(speaker),
                error,
            });
        }
    }

    /// Utterance `seq`'s words, the next turn: over speech or a hold, they
    /// commit the cut; else a reply or report that waited for them is
    /// superseded.
    fn words(&mut self, seq: u64, speaker: Speaker, overlap: Overlap, now: Instant) {
        if self.hold.is_some() || overlap == Overlap::Speech {
            self.commit(speaker, now);
            return;
        }
        let waiting: Vec<Spoken> = match self.pending.iter().find(|p| p.seq == seq) {
            Some(p) => self
                .queue
                .iter()
                .filter(|i| i.opens && self.contends(p, i))
                .map(|i| i.what)
                .collect(),
            None => Vec::new(),
        };
        for what in waiting {
            self.supersede(what);
        }
    }

    /// Words over speech: what is held or still queued goes unsaid, and the
    /// utterance is the next turn. The clip stops if it still plays (the late
    /// cut). When nothing queued has begun, it is superseded, no barge-in.
    fn commit(&mut self, speaker: Speaker, now: Instant) {
        let Some(front) = self.queue.front() else {
            self.hold = None;
            return;
        };
        let what = front.what;
        if front.opens && self.hold.is_none() {
            // Nothing queued was said yet.
            self.cut(CutWhy::Superseded, now);
            return;
        }
        let dropped = self.queue.len();
        self.cut(CutWhy::Words, now);
        self.emit(Event::BargeIn {
            speaker,
            what,
            dropped,
        });
    }

    /// Everything queued goes unsaid, `why`: a `Cut` for each reply or report
    /// among it, a report back to the front of the reports from its cut
    /// sentence (but at the call's end), and the clip stopped.
    fn cut(&mut self, why: CutWhy, now: Instant) {
        let hold = self.hold.take();
        let into = match (&hold, self.queue.front()) {
            (Some(h), _) => h.into,
            (None, Some(item)) if item.clip.is_some() => item
                .started
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s)),
            _ => Duration::ZERO,
        };
        let items: Vec<Item> = self.queue.drain(..).collect();
        self.cut_items(items, why, into);
        self.synthesizing = false;
        self.generation += 1;
        self.talk_frames.clear();
        if self.playing.is_some() && !self.stopping {
            self.stopping = true;
            self.outbox.push(Out::Stop);
        }
    }

    /// `items`, taken from the queue, go unsaid, `why`: a `Cut` for each
    /// reply or report among them, the first stopped `into` its audio, and a
    /// report back to the front of the reports from its cut sentence (but at
    /// the call's end).
    fn cut_items(&mut self, items: Vec<Item>, why: CutWhy, into: Duration) {
        let mut report: Option<QueuedReport> = None;
        let mut seen: Vec<Spoken> = Vec::new();
        for (at, item) in items.iter().enumerate() {
            if item.what == Spoken::Acknowledgment {
                continue;
            }
            if item.what == Spoken::Report && why != CutWhy::CallEnded {
                let r = report.get_or_insert_with(|| QueuedReport {
                    sentences: Vec::new(),
                    first: item.index,
                    count: item.count,
                });
                r.sentences.push(item.text.clone());
            }
            if seen.contains(&item.what) {
                continue;
            }
            seen.push(item.what);
            if let Spoken::Reply(turn) = item.what {
                self.split.remove(&turn);
            }
            let last_heard = match item.index {
                0 => None,
                _ => self.said.get(&item.what).cloned(),
            };
            self.emit(Event::Cut {
                what: item.what,
                why,
                sentences: item.count,
                heard: item.index,
                into: if at == 0 { into } else { Duration::ZERO },
                last_heard,
                cut: item.text.clone(),
            });
        }
        // A report back keeps its last sentence heard.
        let back = report.is_some();
        self.said
            .retain(|w, _| !seen.contains(w) || (back && *w == Spoken::Report));
        if let Some(report) = report {
            self.reports.push_front(report);
        }
    }

    /// The call is over: whatever is still queued goes unsaid, and so does
    /// each report still waiting for a pause, sent back by a cut or never
    /// begun (theseus-qrwx).
    fn call_ended(&mut self, now: Instant) {
        if !self.queue.is_empty() {
            self.cut(CutWhy::CallEnded, now);
        }
        for report in std::mem::take(&mut self.reports) {
            let Some(cut) = report.sentences.first() else {
                continue;
            };
            let last_heard = match report.first {
                0 => None,
                _ => self.said.get(&Spoken::Report).cloned(),
            };
            self.emit(Event::Cut {
                what: Spoken::Report,
                why: CutWhy::CallEnded,
                sentences: report.count,
                heard: report.first,
                into: Duration::ZERO,
                last_heard,
                cut: cut.clone(),
            });
        }
    }

    fn ended(&mut self, id: ClipId, now: Instant) {
        if self.playing != Some(id) {
            return;
        }
        self.playing = None;
        self.stopping = false;
        if self.queue.front().is_some_and(|i| i.clip == Some(id)) {
            if let Some(item) = self.queue.pop_front() {
                self.recent.push_back((now, item.text.clone()));
                if self.queue.iter().any(|i| i.what == item.what) {
                    self.said.insert(item.what, item.text);
                } else {
                    self.said.remove(&item.what);
                    self.emit(Event::Spoke { what: item.what });
                }
            }
        }
        if self.queue.is_empty() {
            self.talk_frames.clear();
        }
    }

    fn done(&mut self, done: Done, now: Instant) {
        match done {
            Done::Transcribed {
                seq,
                result,
                latency,
            } => self.heard(seq, result, latency, now),
            Done::Probed {
                speaker,
                id,
                result,
            } => self.probed(speaker, id, result),
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
                                next.opens |= item.opens;
                            } else if item.first.is_none() {
                                self.emit(Event::Spoke { what: item.what });
                            }
                        }
                    }
                }
            }
        }
    }

    /// An utterance's transcript: what it was heard as, and over speech,
    /// what that decides.
    fn heard(
        &mut self,
        seq: u64,
        result: Result<Transcript, SpeechError>,
        latency: Duration,
        now: Instant,
    ) {
        // One that the hold's bound already decided is dropped, as a
        // failure's is: nothing is said of it twice.
        let Some(p) = self
            .pending
            .iter_mut()
            .find(|p| p.seq == seq && matches!(p.state, Transcribed::Waiting))
        else {
            return;
        };
        let (speaker, overlap) = (p.speaker, p.opening.overlap);
        let t = match result {
            Ok(t) => t,
            Err(error) => {
                p.state = Transcribed::Dropped;
                self.emit(Event::Failed {
                    what: Failure::Transcribe(speaker),
                    error,
                });
                // A "stop" that wasn't heard must not be talked over.
                if overlap == Overlap::Speech {
                    self.commit(speaker, now);
                }
                return;
            }
        };
        let heard_as = classify(
            &t.text,
            overlap,
            &p.opening.sentences,
            p.opening.head.as_deref(),
        );
        let utterance = Utterance {
            speaker,
            started: p.started,
            length: p.length,
            closed: p.closed,
            text: t.text,
            usage: t.usage,
            latency,
            over: p.opening.over.clone(),
            heard_as,
        };
        p.state = match heard_as {
            HeardAs::Words => Transcribed::Got(Box::new(utterance.clone())),
            _ => Transcribed::Dropped,
        };
        self.emit(Event::Utterance(utterance));
        if heard_as == HeardAs::Echo {
            *self.echoes.entry(speaker).or_default() += 1;
        }
        if heard_as == HeardAs::Words {
            self.words(seq, speaker, overlap, now);
        } else if let Some(hold) = self.hold.as_mut().filter(|_| overlap == Overlap::Speech) {
            // The plainest reason heard: a request to go on, over a "yeah",
            // over an echo, over a sound.
            let rank = |h: HeardAs| match h {
                HeardAs::Resume => 3,
                HeardAs::Backchannel => 2,
                HeardAs::Echo => 1,
                _ => 0,
            };
            if hold.why.is_none_or(|why| rank(heard_as) > rank(why)) {
                hold.why = Some(heard_as);
            }
        }
    }

    fn reply(&mut self, turn: TurnId, text: &str, now: Instant) {
        let ended = self.turn.take_if(|t| t.id == turn);
        let sentences = speakable(text);
        if sentences.is_empty() {
            return;
        }
        if let Some(ended) = ended {
            self.split.insert(turn, ended.last_speech);
        }
        let count = sentences.len();
        self.enqueue(Spoken::Reply(turn), sentences, 0, count, now);
        // Its speaker talked past it: what they went on to say is heard.
        let what = Spoken::Reply(turn);
        let superseded = self.queue.iter().find(|i| i.what == what).is_some_and(|i| {
            self.pending
                .iter()
                .any(|p| matches!(p.state, Transcribed::Got(_)) && self.contends(p, i))
        });
        if superseded {
            self.supersede(what);
        }
    }

    /// Whether `p`'s words decide whether `item`, which hasn't begun, is
    /// said: it closed after `item` was queued (it held the floor, or began
    /// over it), or it goes on its turn's speaker's thought.
    fn contends(&self, p: &Pending, item: &Item) -> bool {
        if p.closed >= item.asked {
            return true;
        }
        let Spoken::Reply(turn) = item.what else {
            return false;
        };
        self.split
            .get(&turn)
            .and_then(|last| last.get(&p.speaker))
            .is_some_and(|end| p.started <= *end + SPLIT_THOUGHT)
    }

    /// `what`, which hasn't begun, goes unsaid: its speaker's words are the
    /// next turn.
    fn supersede(&mut self, what: Spoken) {
        let (items, kept): (Vec<Item>, Vec<Item>) =
            self.queue.drain(..).partition(|i| i.what == what);
        self.queue = kept.into();
        self.cut_items(items, CutWhy::Superseded, Duration::ZERO);
    }

    /// `what`'s sentences from index `first` of `count`.
    fn enqueue(
        &mut self,
        what: Spoken,
        sentences: Vec<String>,
        first: usize,
        count: usize,
        now: Instant,
    ) {
        let asked = at_tick(self.ticks);
        for (i, text) in sentences.into_iter().enumerate() {
            self.queue.push_back(Item {
                seq: self.next_item,
                what,
                text,
                index: first + i,
                count,
                audio: None,
                clip: None,
                started: None,
                // A report back from a cut began before.
                first: (i == 0 && first == 0).then_some(now),
                asked,
                opens: i == 0,
            });
            self.next_item += 1;
        }
    }

    /// After every event: resume a hold, start a turn, the acknowledgment,
    /// or a report; synthesize the next sentence; play the next clip.
    fn advance(&mut self, now: Instant) {
        self.overdue(now);
        self.probe();
        self.resume(now);
        self.start_turn(now);
        let quiet = self.playing.is_none() && self.queue.is_empty() && !self.floor_held();
        if quiet {
            let due = self.turn.as_mut().filter(|t| t.ack == Ack::Due);
            if let Some(turn) = due {
                turn.ack = Ack::Done;
                let id = turn.id;
                self.queue.push_back(Item {
                    seq: self.next_item,
                    what: Spoken::Acknowledgment,
                    text: ACKNOWLEDGMENT.into(),
                    index: 0,
                    count: 1,
                    audio: Some(self.config.acknowledgment.clone()),
                    clip: None,
                    started: None,
                    first: Some(now),
                    asked: at_tick(self.ticks),
                    opens: true,
                });
                self.next_item += 1;
                self.emit(Event::Acknowledged { turn: id });
            } else if self.turn.is_none() && self.pending.is_empty() {
                // A pause: no turn in flight or waiting, nobody speaking,
                // nothing said or to say.
                if let Some(r) = self.reports.pop_front() {
                    self.enqueue(Spoken::Report, r.sentences, r.first, r.count, now);
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
                utterances.push(*utterance);
            }
        }
        if utterances.is_empty() {
            return;
        }
        let id = TurnId(self.next_turn);
        self.next_turn += 1;
        let mut last_speech: BTreeMap<Speaker, Duration> = BTreeMap::new();
        for u in &utterances {
            let end = last_speech.entry(u.speaker).or_default();
            *end = (*end).max(u.started + u.length);
        }
        self.turn = Some(Turn {
            id,
            started: now,
            ack: Ack::Waiting,
            last_speech,
        });
        self.emit(Event::Turn { id, utterances });
    }

    /// Synthesize the first sentence without audio: one at a time, at most
    /// one ready ahead of the clip that's playing, and none while held.
    fn synthesize_next(&mut self) {
        if self.synthesizing || self.hold.is_some() {
            return;
        }
        let ready = self.queue.iter().take_while(|i| i.audio.is_some()).count();
        if ready > 1 {
            return;
        }
        let Some(item) = self.queue.iter().find(|i| i.audio.is_none()) else {
            return;
        };
        let (seq, what, generation) = (item.seq, item.what, self.generation);
        let text = item.text.clone();
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
        if self.playing.is_some() || self.hold.is_some() {
            return;
        }
        // A reply or report begins only on the floor: nobody speaking, and
        // no words it waits for.
        if self.queue.front().is_some_and(|front| {
            front.opens
                && (self.floor_held()
                    || self.pending.iter().any(|p| {
                        matches!(p.state, Transcribed::Waiting) && self.contends(p, front)
                    }))
        }) {
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
        item.started = Some(now);
        item.opens = false;
        if let Spoken::Reply(turn) = item.what {
            self.split.remove(&turn);
        }
        let first = item.first.take().map(|asked| Event::Speaking {
            what: item.what,
            sentences: item.count,
            first_audio: now - asked,
        });
        // What an open utterance may echo.
        let text = item.text.clone();
        for opening in self.opening.values_mut() {
            opening.sentences.push(text.clone());
        }
        self.next_clip += 1;
        self.playing = Some(id);
        self.outbox.push(Out::Play(id, audio));
        if let Some(event) = first {
            self.emit(event);
        }
    }
}
