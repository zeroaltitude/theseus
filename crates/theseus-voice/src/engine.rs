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
use crate::sentences::sentences;
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
    /// Speakers an echo was heard from: their stop waits for words.
    echo_prone: BTreeSet<Speaker>,
    work: FuturesUnordered<BoxFuture<'static, Done>>,
    outbox: Vec<Out>,
}

/// How long after a sentence ends an utterance may still be its echo.
const ECHO_TAIL: Duration = Duration::from_millis(1200);

/// An open utterance's start: what Theseus was doing at its first speech
/// frame, and the sentences it may echo.
struct Opening {
    over: Option<Over>,
    overlap: Overlap,
    sentences: Vec<String>,
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
            echo_prone: BTreeSet::new(),
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
                    let sentences = sentences(&text);
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
                let opening = self.what_is_over(now);
                self.opening.insert(speaker, opening);
            }
            // An echo-prone speaker's stop waits for their words.
            if step.speech && talking && !self.echo_prone.contains(&speaker) {
                let n = self.talk_frames.entry(speaker).or_default();
                *n += 1;
                barge |= *n >= self.barge_frames;
            }
            match step.closed {
                Some(Closed::Utterance { first_tick, audio }) => {
                    self.talk_frames.remove(&speaker);
                    let opening = self.opening.remove(&speaker).unwrap_or(Opening {
                        over: None,
                        overlap: Overlap::None,
                        sentences: Vec::new(),
                    });
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
        let (over, overlap) = match self.queue.front() {
            Some(item) => {
                if item.clip.is_some() || self.hold.is_some() {
                    sentences.push(item.text.clone());
                }
                let over = Over::Saying {
                    what: item.what,
                    sentence: item.index,
                    text: item.text.clone(),
                };
                (Some(over), Overlap::Speech)
            }
            None => {
                let over = self.turn.as_ref().map(|t| Over::Preparing { turn: t.id });
                let tail = match self.recent.is_empty() {
                    true => Overlap::None,
                    false => Overlap::Tail,
                };
                (over, tail)
            }
        };
        Opening {
            over,
            overlap,
            sentences,
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
            || self.vads.values().any(Vad::is_open)
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

    /// Words over speech: what is held or still queued goes unsaid, and the
    /// utterance is the next turn. The clip stops if it still plays (the late
    /// cut).
    fn commit(&mut self, speaker: Speaker, now: Instant) {
        let Some(what) = self.queue.front().map(|i| i.what) else {
            self.hold = None;
            return;
        };
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
        if let Some(report) = report {
            self.reports.push_front(report);
        } else {
            self.said.remove(&Spoken::Report);
        }
        self.said.retain(|w, _| *w == Spoken::Report);
        self.synthesizing = false;
        self.generation += 1;
        self.talk_frames.clear();
        if self.playing.is_some() && !self.stopping {
            self.stopping = true;
            self.outbox.push(Out::Stop);
        }
    }

    /// The call is over: whatever is still queued goes unsaid.
    fn call_ended(&mut self, now: Instant) {
        if !self.queue.is_empty() {
            self.cut(CutWhy::CallEnded, now);
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

    /// An utterance's transcript: what it was heard as, and over speech,
    /// what that decides.
    fn heard(
        &mut self,
        seq: u64,
        result: Result<Transcript, SpeechError>,
        latency: Duration,
        now: Instant,
    ) {
        let Some(p) = self.pending.iter_mut().find(|p| p.seq == seq) else {
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
        let heard_as = classify(&t.text, overlap, &p.opening.sentences);
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
            self.echo_prone.insert(speaker);
        }
        if overlap != Overlap::Speech {
            return;
        }
        if heard_as == HeardAs::Words {
            self.commit(speaker, now);
        } else if let Some(hold) = &mut self.hold {
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
        if self.turn.as_ref().is_some_and(|t| t.id == turn) {
            self.turn = None;
        }
        let sentences = sentences(text);
        let count = sentences.len();
        self.enqueue(Spoken::Reply(turn), sentences, 0, count, now);
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
            });
            self.next_item += 1;
        }
    }

    /// After every event: resume a hold, start a turn, the acknowledgment,
    /// or a report; synthesize the next sentence; play the next clip.
    fn advance(&mut self, now: Instant) {
        self.resume(now);
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
                    text: ACKNOWLEDGMENT.into(),
                    index: 0,
                    count: 1,
                    audio: Some(self.config.acknowledgment.clone()),
                    clip: None,
                    started: None,
                    first: Some(now),
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
        self.turn = Some(Turn {
            id,
            started: now,
            ack: Ack::Waiting,
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
        let id = ClipId(self.next_clip);
        let Some(item) = self.queue.front_mut() else {
            return;
        };
        let Some(audio) = item.audio.clone() else {
            return;
        };
        item.clip = Some(id);
        item.started = Some(now);
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
