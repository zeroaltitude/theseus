//! A barge-in that pauses until words decide, and replies that wait for the
//! speaker (theseus-9ln5, theseus-kpa7): through the seam, from WAV fixtures,
//! in virtual time, each at its exact times.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    write_wav, Audio, Command, Config, CutWhy, Engine, Event, Failure, HeardAs, Over, Played,
    Speaker, Speech, SpeechError, SpeechFuture, Spoken, StandInSpeech, Synthesis, Transcript,
    TurnId, Utterance, WavIo,
};
use tokio::time::{sleep, sleep_until, Instant};

const OWNER: Speaker = Speaker(101);
const ROBIN: Speaker = Speaker(202);

/// A long first sentence, so there's time to talk over it.
const S1: &str = "This first sentence runs on for quite a while, long enough to talk over.";
const S2: &str = "The second sentence is long enough to be cut late, if it comes to that.";
const S3: &str = "Third.";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// How long the stand-in's audio for `text` plays.
fn len(text: &str) -> Duration {
    StandInSpeech::tone_for(text).duration()
}

/// A WAV fixture: `length` of speech (a 220 Hz tone, well over the VAD's
/// threshold).
fn fixture(dir: &Path, name: &str, length: u64) -> PathBuf {
    let path = dir.join(format!("{name}.wav"));
    write_wav(&path, &Audio::tone(220.0, ms(length), 0.3)).expect("write the fixture");
    path
}

/// How the session answers a turn: after a delay, with a reply's text.
type Answer = Box<dyn FnMut(TurnId, &[Utterance]) -> (Duration, String) + Send>;

/// The call's lines: who says how long, from when.
struct Lines<'a> {
    dir: &'a Path,
    io: WavIo,
    n: usize,
}

impl<'a> Lines<'a> {
    fn new(dir: &'a Path, length: u64) -> Self {
        Self {
            dir,
            io: WavIo::new(ms(length)),
            n: 0,
        }
    }

    fn say(mut self, speaker: Speaker, at: u64, length: u64) -> Self {
        self.n += 1;
        let path = fixture(self.dir, &format!("line-{}", self.n), length);
        self.io
            .say_wav(speaker, ms(at), &path)
            .expect("the fixture");
        self
    }
}

/// A call in virtual time: the engine on a task of its own, and the session
/// played here. Returns every event with when it came, and what was played.
/// No acknowledgment comes in its first minute.
async fn call(
    io: WavIo,
    speech: Arc<dyn Speech>,
    answer: Answer,
    reports: Vec<(Duration, &'static str)>,
) -> (Vec<(Duration, Event)>, Vec<Played>) {
    let mut config = Config::new([OWNER, ROBIN]);
    config.acknowledge_after = Duration::from_secs(60);
    call_with(config, io, speech, answer, reports).await
}

async fn call_with(
    config: Config,
    io: WavIo,
    speech: Arc<dyn Speech>,
    answer: Answer,
    reports: Vec<(Duration, &'static str)>,
) -> (Vec<(Duration, Event)>, Vec<Played>) {
    run(config, io, speech, answer, reports, None).await
}

/// As [`call`], and the session leaves at `leave`.
async fn call_leaving(
    io: WavIo,
    speech: Arc<dyn Speech>,
    answer: Answer,
    reports: Vec<(Duration, &'static str)>,
    leave: Duration,
) -> (Vec<(Duration, Event)>, Vec<Played>) {
    let mut config = Config::new([OWNER, ROBIN]);
    config.acknowledge_after = Duration::from_secs(60);
    run(config, io, speech, answer, reports, Some(leave)).await
}

async fn run(
    config: Config,
    io: WavIo,
    speech: Arc<dyn Speech>,
    mut answer: Answer,
    reports: Vec<(Duration, &'static str)>,
    leave: Option<Duration>,
) -> (Vec<(Duration, Event)>, Vec<Played>) {
    let origin = Instant::now();
    let played = io.played();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = tokio::spawn(engine.run());
    if let Some(at) = leave {
        let commands = handle.commands.clone();
        tokio::spawn(async move {
            sleep_until(origin + at).await;
            let _ = commands.send(Command::Leave);
        });
    }
    for (at, text) in reports {
        let commands = handle.commands.clone();
        tokio::spawn(async move {
            sleep_until(origin + at).await;
            let _ = commands.send(Command::Report { text: text.into() });
        });
    }
    let mut events = handle.events;
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        if let Event::Turn { id, utterances } = &event {
            let (delay, text) = answer(*id, utterances);
            let (turn, commands) = (*id, handle.commands.clone());
            tokio::spawn(async move {
                sleep(delay).await;
                let _ = commands.send(Command::Reply { turn, text });
            });
        }
        seen.push((origin.elapsed(), event));
    }
    engine.await.expect("the engine ran to the call's end");
    (seen, played.get())
}

/// Turn 0 is answered at once with `reply`; any later turn in silence.
fn first(reply: &'static str) -> Answer {
    Box::new(move |turn, _| match turn {
        TurnId(0) => (Duration::ZERO, reply.into()),
        _ => (Duration::ZERO, String::new()),
    })
}

/// The three sentences.
fn three() -> &'static str {
    // A static join, so `first` can hold it.
    "This first sentence runs on for quite a while, long enough to talk over. \
     The second sentence is long enough to be cut late, if it comes to that. Third."
}

fn turns(seen: &[(Duration, Event)]) -> Vec<(Duration, Vec<(Speaker, &str)>)> {
    seen.iter()
        .filter_map(|(at, e)| match e {
            Event::Turn { utterances, .. } => Some((
                *at,
                utterances
                    .iter()
                    .map(|u| (u.speaker, u.text.as_str()))
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

fn utterances(seen: &[(Duration, Event)]) -> Vec<(Duration, &Utterance)> {
    seen.iter()
        .filter_map(|(at, e)| match e {
            Event::Utterance(u) => Some((*at, u)),
            _ => None,
        })
        .collect()
}

/// The events `keep` names, with their times.
fn only(seen: &[(Duration, Event)], keep: fn(&Event) -> bool) -> Vec<(Duration, Event)> {
    seen.iter().filter(|(_, e)| keep(e)).cloned().collect()
}

fn resumed(e: &Event) -> bool {
    matches!(e, Event::Resumed { .. })
}

fn cuts(e: &Event) -> bool {
    matches!(e, Event::Cut { .. } | Event::BargeIn { .. })
}

fn starts(played: &[Played]) -> Vec<(Duration, Duration, bool)> {
    played
        .iter()
        .map(|p| (p.started, p.length, p.stopped))
        .collect()
}

fn saying(sentence: usize, text: &str) -> Option<Over> {
    Some(Over::Saying {
        what: Spoken::Reply(TurnId(0)),
        sentence,
        text: text.into(),
    })
}

/// The owner asks from 0 to 0.5 s: his turn is at 1.2 s, and its reply's first
/// sentence plays from 1.2 s.
fn asked(dir: &Path, length: u64) -> Lines<'_> {
    Lines::new(dir, length).say(OWNER, 0, 500)
}

#[tokio::test(start_paused = true)]
async fn a_sound_with_no_words_pauses_the_reply_and_it_resumes_from_the_cut_sentence() {
    let dir = tempfile::tempdir().unwrap();
    // A 400 ms laugh over the first sentence, from 1.5 s.
    let io = asked(dir.path(), 14_000).say(ROBIN, 1500, 400).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, ""));
    let (seen, played) = call(io, speech.clone(), first(three()), vec![]).await;

    // Stopped 300 ms into the sound; the laugh closed at 2.6 s (its end and
    // 700 ms), heard as no words: the first sentence again from its start.
    let again = ms(2600);
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (again, len(S1), false),
            (again + len(S1), len(S2), false),
            (again + len(S1) + len(S2), len(S3), false),
        ]
    );
    assert_eq!(played[0].ended, Some(ms(1800)));
    assert_eq!(
        speech.syntheses(),
        3,
        "the held audio replayed, none made again"
    );
    assert_eq!(
        only(&seen, resumed),
        [(
            again,
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Wordless,
                held: ms(800),
            }
        )]
    );
    assert!(only(&seen, cuts).is_empty(), "{seen:?}");
    assert_eq!(turns(&seen).len(), 1, "only zeroaltitude's question");
    assert!(seen
        .iter()
        .any(|(at, e)| *at == again + len(S1) + len(S2) + len(S3)
            && *e
                == Event::Spoke {
                    what: Spoken::Reply(TurnId(0))
                }));
}

#[tokio::test(start_paused = true)]
async fn its_own_sentence_heard_back_is_an_echo_and_one_word_never_is() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 14_000).say(ROBIN, 1500, 600).io;
    let speech = Arc::new(
        StandInSpeech::new().transcript(ROBIN, "this first sentence runs on for quite a while"),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(
        only(&seen, resumed),
        [(
            ms(2800),
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Echo,
                held: ms(1000),
            }
        )]
    );
    assert_eq!(utterances(&seen)[1].1.heard_as, HeardAs::Echo);
    assert_eq!(turns(&seen).len(), 1);
    assert_eq!(played.len(), 4);
    assert!(only(&seen, cuts).is_empty());

    // "Stop", one of the sentence's own words: never an echo, so it cuts.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(ROBIN, 1500, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "Stop."));
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(
        only(&seen, cuts),
        [
            (
                ms(2800),
                Event::Cut {
                    what: Spoken::Reply(TurnId(0)),
                    why: CutWhy::Words,
                    sentences: 3,
                    heard: 0,
                    into: ms(600),
                    last_heard: None,
                    cut: S1.into(),
                }
            ),
            (
                ms(2800),
                Event::BargeIn {
                    speaker: ROBIN,
                    what: Spoken::Reply(TurnId(0)),
                    dropped: 3,
                }
            ),
        ]
    );
    assert_eq!(played.len(), 1);
    assert_eq!(turns(&seen)[1], (ms(2800), vec![(ROBIN, "Stop.")]));
}

#[tokio::test(start_paused = true)]
async fn a_yeah_over_a_reply_goes_on_and_a_short_mm_hm_stops_nothing() {
    let dir = tempfile::tempdir().unwrap();
    // "Yeah" for 600 ms at 1.5 s stops it at 1.8 s; it closes at 2.8 s and
    // the first sentence plays again, to 2.8 s + S1. "Mm-hm" for 200 ms at
    // 3.5 s, over the replay: too short to stop it.
    let io = asked(dir.path(), 14_000)
        .say(ROBIN, 1500, 600)
        .say(ROBIN, 3500, 200)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, "Yeah.")
            .transcript(ROBIN, "Mm-hm."),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(
        only(&seen, resumed),
        [(
            ms(2800),
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Backchannel,
                held: ms(1000),
            }
        )]
    );
    let heard: Vec<_> = utterances(&seen)
        .into_iter()
        .filter(|(_, u)| u.speaker == ROBIN)
        .map(|(at, u)| (at, u.over.clone(), u.heard_as))
        .collect();
    assert_eq!(
        heard,
        [
            (ms(2800), saying(0, S1), HeardAs::Backchannel),
            (ms(4400), saying(0, S1), HeardAs::Backchannel),
        ]
    );
    assert_eq!(turns(&seen).len(), 1, "neither is a turn");
    assert_eq!(
        starts(&played)[..2],
        [(ms(1200), len(S1), true), (ms(2800), len(S1), false)]
    );
    assert_eq!(played.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn a_mhmm_and_an_oh_okay_over_a_long_reply_resume_it() {
    let dir = tempfile::tempdir().unwrap();
    // "Mhmm" from 1.5 s stops it at 1.8 s; it closes at 2.8 s, and the
    // first sentence plays again. "Oh, okay." from 3.5 s stops the replay
    // at 3.8 s; it closes at 4.8 s, and it plays again, then on.
    let io = asked(dir.path(), 16_000)
        .say(ROBIN, 1500, 600)
        .say(ROBIN, 3500, 600)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, "Mhmm.")
            .transcript(ROBIN, "Oh, okay."),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let back = |at: u64| Event::Resumed {
        what: Spoken::Reply(TurnId(0)),
        why: HeardAs::Backchannel,
        held: ms(at),
    };
    assert_eq!(
        only(&seen, resumed),
        [(ms(2800), back(1000)), (ms(4800), back(1000))]
    );
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (ms(2800), len(S1), true),
            (ms(4800), len(S1), false),
            (ms(4800) + len(S1), len(S2), false),
            (ms(4800) + len(S1) + len(S2), len(S3), false),
        ]
    );
    assert_eq!(turns(&seen).len(), 1, "neither is a turn");
    assert!(only(&seen, cuts).is_empty());
}

#[tokio::test(start_paused = true)]
async fn a_yeah_after_a_closing_question_is_a_turn() {
    let dir = tempfile::tempdir().unwrap();
    let reply = "Here is the plan. Should I deploy it now?";
    let end = ms(1200) + len("Here is the plan.") + len("Should I deploy it now?");
    let at = end.as_millis() as u64 + 500;
    let io = asked(dir.path(), 8000).say(OWNER, at, 400).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "what's the plan?")
            .transcript(OWNER, "Yeah."),
    );
    let (seen, played) = call(io, speech, first(reply), vec![]).await;
    assert_eq!(played.len(), 2);
    assert!(played.iter().all(|p| !p.stopped));
    // 0.5 s after the last sentence ended: in the echo tail, where only an
    // echo is dropped. A "yeah" there answers the question.
    let closed = ms(at + 400 + 700);
    assert_eq!(
        turns(&seen),
        [
            (ms(1200), vec![(OWNER, "what's the plan?")]),
            (closed, vec![(OWNER, "Yeah.")]),
        ]
    );
    let (_, yeah) = utterances(&seen)[1];
    assert_eq!((yeah.over.clone(), yeah.heard_as), (None, HeardAs::Words));
}

#[tokio::test(start_paused = true)]
async fn a_yes_begun_on_a_closing_questions_last_word_is_a_turn() {
    // "Should I deploy it now?" ends at 3.4 s. "Yes." from 3.2 s for 300 ms:
    // 200 ms over it, too short to stop it, and it closes at 4.2 s, after
    // the question ended (theseus-1cz8).
    let dir = tempfile::tempdir().unwrap();
    let question = "Should I deploy it now?";
    let reply = "Here is the plan. Should I deploy it now?";
    let io = asked(dir.path(), 8000).say(OWNER, 3200, 300).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "what's the plan?")
            .transcript(OWNER, "Yes."),
    );
    let (seen, played) = call(io, speech, first(reply), vec![]).await;
    assert_eq!(played.len(), 2);
    assert!(played.iter().all(|p| !p.stopped), "{played:?}");
    assert!(only(&seen, cuts).is_empty() && only(&seen, resumed).is_empty());
    assert_eq!(
        turns(&seen),
        [
            (ms(1200), vec![(OWNER, "what's the plan?")]),
            (ms(4200), vec![(OWNER, "Yes.")]),
        ]
    );
    let (_, yes) = utterances(&seen)[1];
    assert_eq!(
        (yes.over.clone(), yes.heard_as),
        (saying(1, question), HeardAs::Words)
    );
}

#[tokio::test(start_paused = true)]
async fn a_yeah_after_a_closing_question_with_another_reply_queued_is_a_turn() {
    // The owner's turn 0 is at 1.2 s, and Robin's turn 1 at 1.4 s, when turn 0's
    // reply comes. Synthesis takes 1 s, so the question plays from 3.4 s to
    // 4.665 s, and Robin's reply, sent at 4.0 s, is synthesizing until 5.0
    // s. The owner's "Yeah." from 4.9 s answers the question: nothing is being
    // said, so it is a turn (theseus-1cz8), and Robin's reply, which waited
    // for it, is superseded.
    let script: &'static [(u64, &'static str)] = &[
        (200, "Here is the plan. Should I deploy it now?"),
        (2600, "The logs are clean."),
    ];
    let dir = tempfile::tempdir().unwrap();
    let io = Lines::new(dir.path(), 9000)
        .say(OWNER, 0, 500)
        .say(ROBIN, 0, 600)
        .say(OWNER, 4900, 400)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .delays(Duration::ZERO, ms(1000))
            .transcript(OWNER, "what's the plan?")
            .transcript(ROBIN, "are the logs clean?")
            .transcript(OWNER, "Yeah."),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    let question = "Should I deploy it now?";
    assert_eq!(
        starts(&played),
        [
            (ms(2400), len("Here is the plan."), false),
            (ms(3400), len(question), false),
        ]
    );
    let closed = ms(4900 + 400 + 700);
    assert_eq!(
        turns(&seen),
        [
            (ms(1200), vec![(OWNER, "what's the plan?")]),
            (ms(1400), vec![(ROBIN, "are the logs clean?")]),
            (closed, vec![(OWNER, "Yeah.")]),
        ]
    );
    let (_, yeah) = utterances(&seen)[2];
    assert_eq!(yeah.heard_as, HeardAs::Words);
    assert_eq!(
        only(&seen, cuts),
        [(
            closed,
            Event::Cut {
                what: Spoken::Reply(TurnId(1)),
                why: CutWhy::Superseded,
                sentences: 1,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: "The logs are clean.".into(),
            }
        )]
    );
}

#[tokio::test(start_paused = true)]
async fn words_over_the_second_of_four_sentences_cut_it() {
    let dir = tempfile::tempdir().unwrap();
    // The first sentence plays from 1.2 s to 2.3 s, and the second from 2.3
    // s; Robin talks over it from then for 600 ms.
    let first_one = "Your build is green.";
    let second = "Second, the deploy to the staging account went out an hour ago.";
    let reply = "Your build is green. \
                 Second, the deploy to the staging account went out an hour ago. \
                 Third, the bill. Fourth, the rest.";
    assert_eq!(len(first_one), ms(1100));
    let io = asked(dir.path(), 8000).say(ROBIN, 2300, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "wait, which account"));
    let (seen, played) = call(io, speech, first(reply), vec![]).await;

    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(first_one), false),
            (ms(2300), len(second), true)
        ]
    );
    assert_eq!(played[1].ended, Some(ms(2600)));
    // At the words' transcript, when the utterance closed.
    let at = ms(2300 + 600 + 700);
    assert_eq!(
        only(&seen, cuts),
        [
            (
                at,
                Event::Cut {
                    what: Spoken::Reply(TurnId(0)),
                    why: CutWhy::Words,
                    sentences: 4,
                    heard: 1,
                    into: ms(300),
                    last_heard: Some(first_one.into()),
                    cut: second.into(),
                }
            ),
            (
                at,
                Event::BargeIn {
                    speaker: ROBIN,
                    what: Spoken::Reply(TurnId(0)),
                    dropped: 3,
                }
            ),
        ]
    );
    let next = seen
        .iter()
        .find_map(|(when, e)| match e {
            Event::Turn {
                id: TurnId(1),
                utterances,
            } => Some((*when, utterances.clone())),
            _ => None,
        })
        .expect("the next turn");
    assert_eq!(next.0, at);
    assert_eq!(next.1.len(), 1);
    assert_eq!(next.1[0].text, "wait, which account");
    assert_eq!(next.1[0].over, saying(1, second));
    assert_eq!(next.1[0].heard_as, HeardAs::Words);
    assert!(only(&seen, resumed).is_empty());
}

#[tokio::test(start_paused = true)]
async fn go_on_after_a_stop_plays_on_from_the_cut_sentence() {
    let dir = tempfile::tempdir().unwrap();
    // A laugh from 1.5 s stops it at 1.8 s; "go on" from 2.0 s to 2.5 s,
    // while it's held, closes at 3.2 s.
    let io = asked(dir.path(), 14_000)
        .say(ROBIN, 1500, 400)
        .say(OWNER, 2000, 500)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "tell me about it")
            .transcript(ROBIN, "")
            .transcript(OWNER, "Go on."),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(
        only(&seen, resumed),
        [(
            ms(3200),
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Resume,
                held: ms(1400),
            }
        )]
    );
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (ms(3200), len(S1), false),
            (ms(3200) + len(S1), len(S2), false),
            (ms(3200) + len(S1) + len(S2), len(S3), false),
        ]
    );
    assert_eq!(turns(&seen).len(), 1, "no turn");
    let go_on = utterances(&seen)
        .into_iter()
        .find(|(_, u)| u.text == "Go on.")
        .expect("heard");
    assert_eq!(go_on.1.heard_as, HeardAs::Resume);
}

#[tokio::test(start_paused = true)]
async fn a_short_no_cuts_late_at_its_transcript() {
    let dir = tempfile::tempdir().unwrap();
    // 200 ms from 1.5 s: never 300 ms, so no stop at the VAD.
    let io = asked(dir.path(), 8000).say(ROBIN, 1500, 200).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "No."));
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let at = ms(1500 + 200 + 700);
    assert_eq!(starts(&played), [(ms(1200), len(S1), true)]);
    assert_eq!(played[0].ended, Some(at));
    assert_eq!(
        only(&seen, cuts),
        [
            (
                at,
                Event::Cut {
                    what: Spoken::Reply(TurnId(0)),
                    why: CutWhy::Words,
                    sentences: 3,
                    heard: 0,
                    into: ms(1200),
                    last_heard: None,
                    cut: S1.into(),
                }
            ),
            (
                at,
                Event::BargeIn {
                    speaker: ROBIN,
                    what: Spoken::Reply(TurnId(0)),
                    dropped: 3,
                }
            ),
        ]
    );
    assert_eq!(turns(&seen)[1], (at, vec![(ROBIN, "No.")]));

    // Over a reply that has ended by its transcript: just a turn.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 5000).say(ROBIN, 1500, 200).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "No."));
    let (seen, played) = call(io, speech, first("Done."), vec![]).await;
    assert_eq!(starts(&played), [(ms(1200), len("Done."), false)]);
    assert!(only(&seen, cuts).is_empty());
    assert_eq!(turns(&seen)[1], (at, vec![(ROBIN, "No.")]));
}

/// The stand-in, but a transcription of `fails`'s audio fails.
struct Deaf {
    inner: StandInSpeech,
    fails: Speaker,
}

impl Speech for Deaf {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a Audio,
    ) -> SpeechFuture<'a, Transcript> {
        if speaker == self.fails {
            return Box::pin(async { Err(SpeechError("the provider hung up".into())) });
        }
        self.inner.transcribe(speaker, audio)
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        self.inner.synthesize(text)
    }
}

#[tokio::test(start_paused = true)]
async fn a_failed_transcription_over_speech_commits() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(ROBIN, 1500, 600).io;
    let speech = Arc::new(Deaf {
        inner: StandInSpeech::new(),
        fails: ROBIN,
    });
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let at = ms(2800);
    assert_eq!(starts(&played), [(ms(1200), len(S1), true)]);
    assert!(seen.iter().any(|(when, e)| *when == at
        && *e
            == Event::Failed {
                what: Failure::Transcribe(ROBIN),
                error: SpeechError("the provider hung up".into()),
            }));
    assert_eq!(
        only(&seen, cuts),
        [
            (
                at,
                Event::Cut {
                    what: Spoken::Reply(TurnId(0)),
                    why: CutWhy::Words,
                    sentences: 3,
                    heard: 0,
                    into: ms(600),
                    last_heard: None,
                    cut: S1.into(),
                }
            ),
            (
                at,
                Event::BargeIn {
                    speaker: ROBIN,
                    what: Spoken::Reply(TurnId(0)),
                    dropped: 3,
                }
            ),
        ]
    );
    assert!(only(&seen, resumed).is_empty());
    assert_eq!(turns(&seen).len(), 1, "unheard, so no turn");
}

#[tokio::test(start_paused = true)]
async fn a_report_cut_by_words_comes_back_from_its_cut_sentence() {
    let dir = tempfile::tempdir().unwrap();
    let r1 = "The deploy finished.";
    let r2 = "All forty checks passed on the first try.";
    let r3 = "Nothing else.";
    assert_eq!(len(r1), ms(1100));
    // The report plays from 0.1 s; its second sentence from 1.2 s, cut by
    // the owner's words from then.
    let io = Lines::new(dir.path(), 9000).say(OWNER, 1200, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(OWNER, "hold on, what's that"));
    let answer: Answer = Box::new(|_, _| (ms(500), "Sure.".into()));
    let report = "The deploy finished. All forty checks passed on the first try. Nothing else.";
    let (seen, played) = call(io, speech, answer, vec![(ms(100), report)]).await;

    let at = ms(1200 + 600 + 700);
    assert_eq!(
        only(&seen, cuts),
        [
            (
                at,
                Event::Cut {
                    what: Spoken::Report,
                    why: CutWhy::Words,
                    sentences: 3,
                    heard: 1,
                    into: ms(300),
                    last_heard: Some(r1.into()),
                    cut: r2.into(),
                }
            ),
            (
                at,
                Event::BargeIn {
                    speaker: OWNER,
                    what: Spoken::Report,
                    dropped: 2,
                }
            ),
        ]
    );
    // The owner's turn is answered at 3.0 s; at the pause after, the report
    // again from its cut sentence, split once.
    let back = ms(3000) + len("Sure.");
    assert_eq!(
        starts(&played),
        [
            (ms(100), len(r1), false),
            (ms(1200), len(r2), true),
            (ms(3000), len("Sure."), false),
            (back, len(r2), false),
            (back + len(r2), len(r3), false),
        ]
    );
    let (_, turn) = &turns(&seen)[0];
    assert_eq!(turn, &[(OWNER, "hold on, what's that")]);
    let over = &utterances(&seen)[0].1.over;
    assert_eq!(
        over,
        &Some(Over::Saying {
            what: Spoken::Report,
            sentence: 1,
            text: r2.into()
        })
    );
    // One `Speaking` for the report, at its first audio.
    let speaking: Vec<_> = seen
        .iter()
        .filter(|(_, e)| {
            matches!(
                e,
                Event::Speaking {
                    what: Spoken::Report,
                    ..
                }
            )
        })
        .map(|(at, _)| *at)
        .collect();
    assert_eq!(speaking, [ms(100)]);
}

#[tokio::test(start_paused = true)]
async fn after_two_echoes_its_speaker_doesnt_stop_it_and_their_words_cut_late() {
    let dir = tempfile::tempdir().unwrap();
    // An echo from 1.5 s stops it; it resumes at 2.8 s. A second echo from
    // 3.2 s still stops it, at 3.5 s (one verdict leaves the stop on); it
    // resumes at 4.5 s. Now echo-prone, Robin's words over the second
    // sentence don't stop it, and cut at their transcript (theseus-3ug0).
    let again = ms(4500);
    let s2_at = again + len(S1);
    let words_at = s2_at.as_millis() as u64 + 200;
    let io = asked(dir.path(), 16_000)
        .say(ROBIN, 1500, 600)
        .say(ROBIN, 3200, 600)
        .say(ROBIN, words_at, 600)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, "this first sentence runs on for quite a while")
            .transcript(ROBIN, "a while long enough to talk over")
            .transcript(ROBIN, "hang on a moment please"),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let cut_at = ms(words_at + 600 + 700);
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (ms(2800), len(S1), true),
            (again, len(S1), false),
            (s2_at, len(S2), true),
        ]
    );
    assert_eq!(played[1].ended, Some(ms(3500)));
    assert_eq!(played[3].ended, Some(cut_at));
    let heard: Vec<_> = utterances(&seen)
        .into_iter()
        .filter(|(_, u)| u.speaker == ROBIN)
        .map(|(_, u)| u.heard_as)
        .collect();
    assert_eq!(heard, [HeardAs::Echo, HeardAs::Echo, HeardAs::Words]);
    assert_eq!(
        only(&seen, resumed)
            .into_iter()
            .map(|(at, _)| at)
            .collect::<Vec<_>>(),
        [ms(2800), again]
    );
    assert_eq!(
        only(&seen, cuts),
        [
            (
                cut_at,
                Event::Cut {
                    what: Spoken::Reply(TurnId(0)),
                    why: CutWhy::Words,
                    sentences: 3,
                    heard: 1,
                    into: cut_at - s2_at,
                    last_heard: Some(S1.into()),
                    cut: S2.into(),
                }
            ),
            (
                cut_at,
                Event::BargeIn {
                    speaker: ROBIN,
                    what: Spoken::Reply(TurnId(0)),
                    dropped: 2,
                }
            ),
        ]
    );
    assert_eq!(
        turns(&seen)[1],
        (cut_at, vec![(ROBIN, "hang on a moment please")])
    );
}

#[tokio::test(start_paused = true)]
async fn one_echo_verdict_leaves_its_speakers_stop_on() {
    let dir = tempfile::tempdir().unwrap();
    // An echo from 1.5 s stops it at 1.8 s; it resumes at 2.8 s. Robin's
    // words from 3.2 s over the replay still stop it at 300 ms, at 3.5 s,
    // and cut it at their transcript, at 4.5 s.
    let io = asked(dir.path(), 9000)
        .say(ROBIN, 1500, 600)
        .say(ROBIN, 3200, 600)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, "this first sentence runs on for quite a while")
            .transcript(ROBIN, "Hang on, wait a second."),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(
        starts(&played),
        [(ms(1200), len(S1), true), (ms(2800), len(S1), true)]
    );
    assert_eq!(played[1].ended, Some(ms(3500)), "stopped at 300 ms");
    let heard: Vec<_> = utterances(&seen)
        .into_iter()
        .filter(|(_, u)| u.speaker == ROBIN)
        .map(|(_, u)| u.heard_as)
        .collect();
    assert_eq!(heard, [HeardAs::Echo, HeardAs::Words]);
    assert_eq!(
        only(&seen, cuts)[0],
        (
            ms(4500),
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Words,
                sentences: 3,
                heard: 0,
                into: ms(700),
                last_heard: None,
                cut: S1.into(),
            }
        )
    );
    assert_eq!(
        turns(&seen)[1],
        (ms(4500), vec![(ROBIN, "Hang on, wait a second.")])
    );
}

#[tokio::test(start_paused = true)]
async fn the_played_sentence_heard_back_whole_is_still_an_echo() {
    let dir = tempfile::tempdir().unwrap();
    // The whole first sentence back through Robin's microphone, from 1.5 s.
    let io = asked(dir.path(), 14_000).say(ROBIN, 1500, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, S1));
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(utterances(&seen)[1].1.heard_as, HeardAs::Echo);
    assert_eq!(turns(&seen).len(), 1, "no turn");
    assert_eq!(
        only(&seen, resumed),
        [(
            ms(2800),
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Echo,
                held: ms(1000),
            }
        )]
    );
    assert_eq!(played.len(), 4);
    assert!(only(&seen, cuts).is_empty());
}

#[tokio::test(start_paused = true)]
async fn an_either_or_answer_over_its_question_is_a_turn_and_cuts_it() {
    // "The daily view." from 2.0 s over the question, which plays from 1.2 s
    // to 3.51 s: it stops at 2.3 s, and its transcript at 3.3 s cuts the
    // question. Every word of it is the question's, but it is no copy.
    let dir = tempfile::tempdir().unwrap();
    let question = "Do you want the daily or the monthly view?";
    let io = asked(dir.path(), 8000).say(ROBIN, 2000, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "The daily view."));
    let (seen, played) = call(io, speech, first(question), vec![]).await;
    assert_eq!(
        starts(&played),
        [(ms(1200), len(question), true)],
        "no replay"
    );
    let (_, answer) = utterances(&seen)[1];
    assert_eq!(answer.heard_as, HeardAs::Words);
    assert!(only(&seen, resumed).is_empty());
    assert_eq!(
        turns(&seen)[1],
        (ms(3300), vec![(ROBIN, "The daily view.")])
    );
    assert_eq!(
        only(&seen, cuts)[0],
        (
            ms(3300),
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Words,
                sentences: 1,
                heard: 0,
                into: ms(1100),
                last_heard: None,
                cut: question.into(),
            }
        )
    );
}

#[tokio::test(start_paused = true)]
async fn an_answer_with_its_questions_words_in_the_tail_is_a_turn() {
    // "Should I deploy it now?" ends at 3.4 s; "Yes, deploy it now." from
    // 3.9 s, in the echo tail.
    let dir = tempfile::tempdir().unwrap();
    let reply = "Here is the plan. Should I deploy it now?";
    let end = ms(1200) + len("Here is the plan.") + len("Should I deploy it now?");
    assert_eq!(end, ms(3400));
    let io = asked(dir.path(), 8000).say(OWNER, 3900, 700).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "what's the plan?")
            .transcript(OWNER, "Yes, deploy it now."),
    );
    let (seen, played) = call(io, speech, first(reply), vec![]).await;
    assert_eq!(played.len(), 2);
    assert!(played.iter().all(|p| !p.stopped));
    assert_eq!(
        turns(&seen),
        [
            (ms(1200), vec![(OWNER, "what's the plan?")]),
            (ms(3900 + 700 + 700), vec![(OWNER, "Yes, deploy it now.")]),
        ]
    );
    assert_eq!(utterances(&seen)[1].1.heard_as, HeardAs::Words);
}

/// Each turn answered after its own delay, with its own text.
fn answers(by_turn: &'static [(u64, &'static str)]) -> Answer {
    Box::new(move |turn, _| {
        let (delay, text) = by_turn.get(turn.0 as usize).copied().unwrap_or((0, ""));
        (ms(delay), text.into())
    })
}

#[tokio::test(start_paused = true)]
async fn a_reply_waits_for_the_speaker_and_is_superseded_by_their_words() {
    // The owner asks; his turn is at 1.2 s, and its reply comes at 3.0 s, while
    // he talks again from 2.5 s to 3.5 s. His utterance closes at 4.2 s.
    let reply = "Here are the logs.";
    let script: &'static [(u64, &'static str)] = &[(1800, "Here are the logs.")];
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(OWNER, 2500, 1000).io;
    // A cough: the reply plays when it closes.
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "show me the logs")
            .transcript(OWNER, ""),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert_eq!(starts(&played), [(ms(4200), len(reply), false)]);
    assert!(only(&seen, cuts).is_empty());
    assert_eq!(turns(&seen).len(), 1);
    let (_, cough) = utterances(&seen)[1];
    assert_eq!(
        (cough.over.clone(), cough.heard_as),
        (Some(Over::Preparing { turn: TurnId(0) }), HeardAs::Wordless)
    );

    // Words: the reply is never played, and his words are the next turn.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(OWNER, 2500, 1000).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "show me the logs")
            .transcript(OWNER, "only the errors, I mean"),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert!(played.is_empty(), "{played:?}");
    assert_eq!(
        only(&seen, cuts),
        [(
            ms(4200),
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Superseded,
                sentences: 1,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: reply.into(),
            }
        )]
    );
    assert_eq!(
        turns(&seen)[1],
        (ms(4200), vec![(OWNER, "only the errors, I mean")])
    );
}

#[tokio::test(start_paused = true)]
async fn a_thought_split_by_a_pause_gets_one_answer() {
    // "Can you make yourself a" from 0 to 1 s closes at 1.7 s; "tool to
    // order" begins 800 ms after its last word and closes at 3.5 s. The
    // reply to the fragment comes at 4.2 s.
    let script: &'static [(u64, &'static str)] = &[
        (2500, "Your message cut off."),
        (500, "Yes, I can make one."),
    ];
    let dir = tempfile::tempdir().unwrap();
    let io = Lines::new(dir.path(), 9000)
        .say(OWNER, 0, 1000)
        .say(OWNER, 1800, 1000)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "Can you make yourself a")
            .transcript(OWNER, "tool to order"),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert_eq!(
        turns(&seen),
        [
            (ms(1700), vec![(OWNER, "Can you make yourself a")]),
            (ms(4200), vec![(OWNER, "tool to order")]),
        ]
    );
    assert_eq!(
        only(&seen, cuts),
        [(
            ms(4200),
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Superseded,
                sentences: 1,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: "Your message cut off.".into(),
            }
        )]
    );
    // One answer, to the whole.
    assert_eq!(
        starts(&played),
        [(ms(4700), len("Yes, I can make one."), false)]
    );

    // A new question 3 s after the last word doesn't supersede the answer.
    let script: &'static [(u64, &'static str)] = &[(4500, "Here is the first answer.")];
    let dir = tempfile::tempdir().unwrap();
    let io = Lines::new(dir.path(), 9000)
        .say(OWNER, 0, 1000)
        .say(OWNER, 4000, 1000)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "what changed today?")
            .transcript(OWNER, "and who changed it?"),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert!(only(&seen, cuts).is_empty(), "{seen:?}");
    assert_eq!(
        starts(&played),
        [(ms(6200), len("Here is the first answer."), false)]
    );
    assert_eq!(
        turns(&seen),
        [
            (ms(1700), vec![(OWNER, "what changed today?")]),
            (ms(6200), vec![(OWNER, "and who changed it?")]),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_cut_acknowledgment_isnt_said_again() {
    // Turn 0 runs from 1.2 s to 4.2 s: its acknowledgment plays at 3.2 s, and
    // Robin's laugh from 3.24 s stops it at 3.54 s. The reply comes while the
    // laugh is open; when it closes, the reply plays and the chime doesn't.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(ROBIN, 3240, 400).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, ""));
    let answer: Answer = Box::new(|_, _| (ms(3000), "Here you go.".into()));
    let config = Config::new([OWNER, ROBIN]);
    let (seen, played) = call_with(config, io, speech, answer, vec![]).await;
    let laugh_closed = ms(3240 + 400 + 700);
    assert_eq!(
        starts(&played),
        [
            (ms(3200), Audio::chime().duration(), true),
            (laugh_closed, len("Here you go."), false),
        ]
    );
    assert_eq!(played[0].ended, Some(ms(3540)));
    assert_eq!(
        only(&seen, resumed),
        [(
            laugh_closed,
            Event::Resumed {
                what: Spoken::Acknowledgment,
                why: HeardAs::Wordless,
                held: laugh_closed - ms(3540),
            }
        )]
    );
    assert!(only(&seen, cuts).is_empty());
}

#[tokio::test(start_paused = true)]
async fn leaving_with_a_reply_unsaid_cuts_it() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 60_000).io;
    let (engine, mut handle) = Engine::new(
        Config::new([OWNER]),
        Box::new(io),
        Arc::new(StandInSpeech::new()),
    );
    let engine = tokio::spawn(engine.run());
    let origin = Instant::now();
    let mut cut = Vec::new();
    while let Some(event) = handle.events.recv().await {
        match event {
            Event::Turn { id, .. } => {
                let text = three().into();
                handle
                    .commands
                    .send(Command::Reply { turn: id, text })
                    .unwrap();
            }
            Event::Speaking { .. } => {
                sleep(ms(500)).await;
                handle.commands.send(Command::Leave).unwrap();
            }
            e @ Event::Cut { .. } => cut.push((origin.elapsed(), e)),
            _ => {}
        }
    }
    engine.await.unwrap();
    assert_eq!(
        cut,
        [(
            ms(1700),
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::CallEnded,
                sentences: 3,
                heard: 0,
                into: ms(500),
                last_heard: None,
                cut: S1.into(),
            }
        )]
    );
}

/// The stand-in, but `speaker`'s first `stalls` transcriptions never come,
/// or fail after `fails_after` (a provider's own request bound).
struct Stalled {
    inner: StandInSpeech,
    speaker: Speaker,
    stalls: AtomicUsize,
    fails_after: Option<Duration>,
}

impl Stalled {
    fn new(inner: StandInSpeech, speaker: Speaker, stalls: usize) -> Self {
        Self {
            inner,
            speaker,
            stalls: AtomicUsize::new(stalls),
            fails_after: None,
        }
    }
}

impl Speech for Stalled {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a Audio,
    ) -> SpeechFuture<'a, Transcript> {
        let stalls = speaker == self.speaker
            && self
                .stalls
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                .is_ok();
        if !stalls {
            return self.inner.transcribe(speaker, audio);
        }
        match self.fails_after {
            None => Box::pin(std::future::pending()),
            Some(after) => Box::pin(async move {
                sleep(after).await;
                Err(SpeechError("the provider timed out".into()))
            }),
        }
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        self.inner.synthesize(text)
    }
}

fn failed(e: &Event) -> bool {
    matches!(e, Event::Failed { .. })
}

/// The hold's bound's `Failed`, for `speaker`.
fn unheard(speaker: Speaker) -> Event {
    Event::Failed {
        what: Failure::Transcribe(speaker),
        error: SpeechError("no transcript 3000 ms after the utterance closed".into()),
    }
}

/// The reply to turn 0, three sentences, cut by Robin with none of it heard,
/// `into` its first.
fn cut_first(at: Duration, into: u64) -> [(Duration, Event); 2] {
    [
        (
            at,
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Words,
                sentences: 3,
                heard: 0,
                into: ms(into),
                last_heard: None,
                cut: S1.into(),
            },
        ),
        (
            at,
            Event::BargeIn {
                speaker: ROBIN,
                what: Spoken::Reply(TurnId(0)),
                dropped: 3,
            },
        ),
    ]
}

#[tokio::test(start_paused = true)]
async fn a_held_reply_whose_transcript_never_comes_is_cut_at_the_bound() {
    // A laugh from 1.5 s stops the reply at 1.8 s and closes at 2.6 s; its
    // transcript never comes. 3 s later, at 5.6 s, the hold decides as a
    // failure does: the cut is committed (theseus-aq4t). The call goes on:
    // Robin's words from 7.0 s are the next turn, and its reply plays.
    let script: &'static [(u64, &'static str)] = &[
        (
            0,
            "This first sentence runs on for quite a while, long enough to talk over. \
             The second sentence is long enough to be cut late, if it comes to that. Third.",
        ),
        (0, "Here are the logs."),
    ];
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 12_000)
        .say(ROBIN, 1500, 400)
        .say(ROBIN, 7000, 500)
        .io;
    let inner = StandInSpeech::new().transcript(ROBIN, "and the logs?");
    let speech = Arc::new(Stalled::new(inner, ROBIN, 1));
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    let at = ms(5600);
    assert_eq!(only(&seen, failed), [(at, unheard(ROBIN))]);
    assert_eq!(only(&seen, cuts), cut_first(at, 600));
    assert!(only(&seen, resumed).is_empty());
    let next = ms(7000 + 500 + 700);
    assert_eq!(turns(&seen)[1..], [(next, vec![(ROBIN, "and the logs?")])]);
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (next, len("Here are the logs."), false)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn the_providers_own_bound_no_longer_sets_the_holds_wait() {
    // The transcription fails 20 s after it was asked, at 22.6 s: the hold
    // decided at 5.6 s, and the provider's failure says nothing again.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 25_000).say(ROBIN, 1500, 400).io;
    let mut speech = Stalled::new(StandInSpeech::new(), ROBIN, 1);
    speech.fails_after = Some(Duration::from_secs(20));
    let (seen, played) = call(io, Arc::new(speech), first(three()), vec![]).await;
    let at = ms(5600);
    assert_eq!(only(&seen, failed), [(at, unheard(ROBIN))]);
    assert_eq!(only(&seen, cuts), cut_first(at, 600));
    assert_eq!(starts(&played), [(ms(1200), len(S1), true)]);
    assert_eq!(turns(&seen).len(), 1, "unheard, so no turn");
}

#[tokio::test(start_paused = true)]
async fn a_reply_waits_8_s_for_a_floor_held_by_a_steady_sound_then_plays_whole() {
    // Robin's fan, from 0.6 s for 40 s, holds his VAD open: it closes at the
    // 30 s maximum and opens on the next frame. The owner's answer, ready at
    // 1.5 s (in the tick from 1.48 s), waits 8 s for the floor; at 9.48 s the
    // fan's audio so far is transcribed, heard as no words, and the answer
    // plays, whole: the fan's next 300 ms don't stop it (theseus-aq4t).
    let reply = "Here are the logs from this morning.";
    let script: &'static [(u64, &'static str)] = &[(300, "Here are the logs from this morning.")];
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 42_000).say(ROBIN, 600, 40_000).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "show me the logs")
            .transcript(ROBIN, "")
            .transcript(ROBIN, "")
            .transcript(ROBIN, ""),
    );
    let (seen, played) = call(io, speech.clone(), answers(script), vec![]).await;
    assert_eq!(starts(&played), [(ms(9480), len(reply), false)]);
    assert!(only(&seen, cuts).is_empty() && only(&seen, resumed).is_empty());
    assert_eq!(turns(&seen).len(), 1, "the fan is no turn");
    // One probe, and the fan's two utterances.
    assert_eq!(speech.transcriptions(), 4);
    let fan: Vec<_> = utterances(&seen)
        .into_iter()
        .filter(|(_, u)| u.speaker == ROBIN)
        .map(|(at, u)| (at, u.heard_as))
        .collect();
    assert_eq!(
        fan,
        [
            (ms(30_600), HeardAs::Wordless),
            (ms(40_600 + 700), HeardAs::Wordless)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_hold_under_a_steady_sound_resumes_at_the_floors_bound() {
    // Robin's fan starts at 1.5 s over the reply, and stops it at 1.8 s. 8 s
    // into the hold, at 9.8 s, its audio so far is heard as no words: the
    // reply resumes from its cut sentence and plays to its end.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 43_000).say(ROBIN, 1500, 40_000).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, "")
            .transcript(ROBIN, "")
            .transcript(ROBIN, ""),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let again = ms(9800);
    assert_eq!(
        only(&seen, resumed),
        [(
            again,
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Wordless,
                held: ms(8000),
            }
        )]
    );
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (again, len(S1), false),
            (again + len(S1), len(S2), false),
            (again + len(S1) + len(S2), len(S3), false),
        ]
    );
    assert!(only(&seen, cuts).is_empty());
    assert_eq!(turns(&seen).len(), 1);
}

#[tokio::test(start_paused = true)]
async fn a_speaker_talking_10_s_in_one_breath_is_still_waited_for() {
    // Robin talks from 1.0 s to 11.0 s without a pause. The answer, ready at
    // 1.5 s, waits; at 9.5 s his audio so far is words, so it waits on, and
    // his words, closed at 11.7 s, supersede it.
    let script: &'static [(u64, &'static str)] = &[(300, "Here are the logs.")];
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 14_000).say(ROBIN, 1000, 10_000).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "show me the logs")
            .transcript(ROBIN, "and while you are at it")
            .transcript(
                ROBIN,
                "and while you are at it show me the errors from last night",
            ),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert!(played.is_empty(), "{played:?}");
    let closed = ms(11_700);
    assert_eq!(
        only(&seen, cuts),
        [(
            closed,
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Superseded,
                sentences: 1,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: "Here are the logs.".into(),
            }
        )]
    );
    assert_eq!(
        turns(&seen)[1],
        (
            closed,
            vec![(
                ROBIN,
                "and while you are at it show me the errors from last night"
            )]
        )
    );
}

#[tokio::test(start_paused = true)]
async fn another_speakers_words_supersede_a_waiting_reply() {
    // The owner's turn is at 1.2 s, and its reply comes at 3.0 s, while Robin,
    // who has no part in that turn, talks from 2.5 s to 3.5 s. The reply waits
    // for his utterance, closed at 4.2 s, and its transcript: his words
    // supersede it, and are the next turn (theseus-e6mj).
    let script: &'static [(u64, &'static str)] = &[(1800, "Here are the logs.")];
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(ROBIN, 2500, 1000).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "show me the logs")
            .transcript(ROBIN, "the ones from last night too"),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert!(played.is_empty(), "{played:?}");
    let closed = ms(4200);
    assert_eq!(
        only(&seen, cuts),
        [(
            closed,
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::Superseded,
                sentences: 1,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: "Here are the logs.".into(),
            }
        )]
    );
    assert_eq!(
        turns(&seen)[1],
        (closed, vec![(ROBIN, "the ones from last night too")])
    );
}

#[tokio::test(start_paused = true)]
async fn leaving_while_held_cuts_once_and_resumes_nothing() {
    // A laugh from 1.5 s stops the reply at 1.8 s; `Leave` at 2.1 s, while it
    // is held: one cut, the call's end, 600 ms into its first sentence
    // (theseus-e6mj).
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(ROBIN, 1500, 400).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, ""));
    let (seen, played) = call_leaving(io, speech, first(three()), vec![], ms(2100)).await;
    assert_eq!(
        only(&seen, cuts),
        [(
            ms(2100),
            Event::Cut {
                what: Spoken::Reply(TurnId(0)),
                why: CutWhy::CallEnded,
                sentences: 3,
                heard: 0,
                into: ms(600),
                last_heard: None,
                cut: S1.into(),
            }
        )]
    );
    assert!(only(&seen, resumed).is_empty());
    assert_eq!(starts(&played), [(ms(1200), len(S1), true)]);
}

#[tokio::test(start_paused = true)]
async fn a_report_cut_by_words_and_waiting_at_the_calls_end_is_cut() {
    // The report plays from 0.1 s; the owner's words from 1.2 s cut it in its
    // second sentence, at 2.5 s, and it waits for the next pause. His turn is
    // still in flight at 4 s, when the call ends: the report is cut there,
    // heard to its first sentence (theseus-qrwx).
    let r1 = "The deploy finished.";
    let r2 = "All forty checks passed on the first try.";
    let report = "The deploy finished. All forty checks passed on the first try. Nothing else.";
    let dir = tempfile::tempdir().unwrap();
    let io = Lines::new(dir.path(), 9000).say(OWNER, 1200, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(OWNER, "hold on, what's that"));
    let answer: Answer = Box::new(|_, _| (ms(10_000), "Sure.".into()));
    let (seen, _) = call_leaving(io, speech, answer, vec![(ms(100), report)], ms(4000)).await;
    let ended: Vec<_> = only(&seen, cuts)
        .into_iter()
        .filter(|(_, e)| {
            matches!(
                e,
                Event::Cut {
                    why: CutWhy::CallEnded,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        ended,
        [(
            ms(4000),
            Event::Cut {
                what: Spoken::Report,
                why: CutWhy::CallEnded,
                sentences: 3,
                heard: 1,
                into: Duration::ZERO,
                last_heard: Some(r1.into()),
                cut: r2.into(),
            }
        )]
    );
    assert_eq!(
        only(&seen, cuts).len(),
        3,
        "the words' cut and barge-in first"
    );
}

#[tokio::test(start_paused = true)]
async fn a_report_never_begun_is_cut_at_the_calls_end() {
    // The report comes at 2 s, while the owner's turn is in flight; the call
    // ends at 4 s, before any pause (theseus-qrwx).
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 9000).io;
    let speech = Arc::new(StandInSpeech::new());
    let answer: Answer = Box::new(|_, _| (ms(10_000), "Sure.".into()));
    let report = "The backup finished. It took an hour.";
    let (seen, played) = call_leaving(io, speech, answer, vec![(ms(2000), report)], ms(4000)).await;
    assert!(played.is_empty());
    assert_eq!(
        only(&seen, cuts),
        [(
            ms(4000),
            Event::Cut {
                what: Spoken::Report,
                why: CutWhy::CallEnded,
                sentences: 2,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: "The backup finished.".into(),
            }
        )]
    );
}

#[tokio::test(start_paused = true)]
async fn an_echo_prone_speakers_echo_of_a_whole_reply_is_an_echo() {
    // E1 (theseus-j2ut): two echoes make Robin echo-prone (resumed at 2.8 s
    // and 4.5 s). His microphone then carries the first two sentences back
    // in one utterance, from 4.7 s to 12.4 s, with no stop: the run over the
    // sentences joined in the order they played is the whole of it.
    let again = ms(4500);
    let s2_end = again + len(S1) + len(S2);
    let io_len = s2_end.as_millis() as u64 + 4000;
    let dir = tempfile::tempdir().unwrap();
    let echo_len = s2_end.as_millis() as u64 - 4700;
    let io = asked(dir.path(), io_len)
        .say(ROBIN, 1500, 600)
        .say(ROBIN, 3200, 600)
        .say(ROBIN, 4700, echo_len)
        .io;
    let both = format!("{S1} {S2}");
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, "this first sentence runs on for quite a while")
            .transcript(ROBIN, "a while long enough to talk over")
            .transcript(ROBIN, &both),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let heard: Vec<_> = utterances(&seen)
        .into_iter()
        .filter(|(_, u)| u.speaker == ROBIN)
        .map(|(_, u)| u.heard_as)
        .collect();
    assert_eq!(heard, [HeardAs::Echo, HeardAs::Echo, HeardAs::Echo]);
    assert!(only(&seen, cuts).is_empty(), "{seen:?}");
    assert_eq!(turns(&seen).len(), 1, "no turn");
    assert_eq!(
        starts(&played)[2..],
        [
            (again, len(S1), false),
            (again + len(S1), len(S2), false),
            (s2_end, len(S3), false),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn the_first_echo_cut_short_by_its_own_stop_is_an_echo() {
    // E2 (theseus-j2ut): the first sentence starts at 1.2 s, and Robin's
    // microphone plays its head back from 1.3 s; the stop at 1.6 s cuts the
    // echo to two words. A run from the playing sentence's head, by an
    // utterance begun within 0.5 s of its start, is an echo from 2 words.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 14_000).say(ROBIN, 1300, 400).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "This first"));
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(utterances(&seen)[1].1.heard_as, HeardAs::Echo);
    assert_eq!(
        only(&seen, resumed),
        [(
            ms(2400),
            Event::Resumed {
                what: Spoken::Reply(TurnId(0)),
                why: HeardAs::Echo,
                held: ms(800),
            }
        )]
    );
    assert!(only(&seen, cuts).is_empty());
    assert_eq!(turns(&seen).len(), 1);
    assert_eq!(played.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn an_echo_across_a_sentence_boundary_is_an_echo() {
    // E3 (theseus-j2ut): the first sentence ends as the second begins, and
    // Robin's microphone carries back the first's tail and the second's head,
    // from 0.1 s into the second: no one sentence holds 80% of it, the two
    // joined as they played do.
    let s2_at = ms(1200) + len(S1);
    let at = s2_at.as_millis() as u64 + 100;
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 16_000).say(ROBIN, at, 600).io;
    let speech =
        Arc::new(StandInSpeech::new().transcript(ROBIN, "to talk over the second sentence"));
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    assert_eq!(utterances(&seen)[1].1.heard_as, HeardAs::Echo);
    assert_eq!(only(&seen, resumed).len(), 1);
    assert!(only(&seen, cuts).is_empty());
    assert_eq!(turns(&seen).len(), 1);
    assert_eq!(played.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn two_words_not_at_the_playing_sentences_head_are_a_turn() {
    // An answer of two of the question's words, begun within 0.5 s of its
    // start, is no head-run: words, and a turn (theseus-j2ut).
    let dir = tempfile::tempdir().unwrap();
    let question = "Do you want the daily or the monthly view?";
    let io = asked(dir.path(), 8000).say(ROBIN, 1500, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(ROBIN, "Monthly view."));
    let (seen, played) = call(io, speech, first(question), vec![]).await;
    assert_eq!(utterances(&seen)[1].1.heard_as, HeardAs::Words);
    assert_eq!(turns(&seen)[1], (ms(2800), vec![(ROBIN, "Monthly view.")]));
    assert_eq!(starts(&played), [(ms(1200), len(question), true)]);
}

/// The owner's turn 0 is at 1.2 s, and Robin's turn 1 at 1.4 s, when turn 0's
/// reply comes: "Here is the plan." from 1.4 s, then the question, from
/// 2.335 s to 3.6 s. The owner's "Yes." from 3.4 s for 300 ms begins on its
/// last word, too short to stop it, and closes at 4.4 s. Robin's reply comes
/// `robin` after his turn began.
async fn yes_with_a_reply_queued(robin: u64) -> Vec<(Duration, Event)> {
    let script: &'static [(u64, &'static str)] = &[
        (200, "Here is the plan. Should I deploy it now?"),
        (0, "The logs are clean."),
    ];
    let mut by_turn = script.to_vec();
    by_turn[1].0 = robin;
    let by_turn: &'static [(u64, &'static str)] = by_turn.leak();
    let dir = tempfile::tempdir().unwrap();
    let io = Lines::new(dir.path(), 9000)
        .say(OWNER, 0, 500)
        .say(ROBIN, 0, 600)
        .say(OWNER, 3400, 300)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "what's the plan?")
            .transcript(ROBIN, "are the logs clean?")
            .transcript(OWNER, "Yes."),
    );
    let (seen, played) = call(io, speech, answers(by_turn), vec![]).await;
    let question = "Should I deploy it now?";
    assert_eq!(
        starts(&played),
        [
            (ms(1400), len("Here is the plan."), false),
            (ms(1400) + len("Here is the plan."), len(question), false),
        ]
    );
    seen
}

/// The "Yes." is turn 2, at its close, and Robin's reply, which waited for
/// it, is superseded.
fn yes_is_a_turn(seen: &[(Duration, Event)]) {
    let closed = ms(3400 + 300 + 700);
    assert_eq!(turns(seen)[2], (closed, vec![(OWNER, "Yes.")]));
    assert_eq!(utterances(seen)[2].1.heard_as, HeardAs::Words);
    assert_eq!(
        only(seen, cuts),
        [(
            closed,
            Event::Cut {
                what: Spoken::Reply(TurnId(1)),
                why: CutWhy::Superseded,
                sentences: 1,
                heard: 0,
                into: Duration::ZERO,
                last_heard: None,
                cut: "The logs are clean.".into(),
            }
        )]
    );
}

#[tokio::test(start_paused = true)]
async fn a_yes_on_a_closing_questions_last_word_with_a_reply_queued_before_it_is_a_turn() {
    // T1 (theseus-q4pc): Robin's reply is queued at 2.4 s, before the "Yes."
    // began, behind the question.
    yes_is_a_turn(&yes_with_a_reply_queued(1000).await);
}

#[tokio::test(start_paused = true)]
async fn a_yes_on_a_closing_questions_last_word_with_a_reply_queued_under_it_is_a_turn() {
    // T2 (theseus-q4pc): Robin's reply is queued at 3.5 s, after the "Yes."
    // began and before it closed.
    yes_is_a_turn(&yes_with_a_reply_queued(2100).await);
}

#[tokio::test(start_paused = true)]
async fn one_echo_each_from_two_speakers_leaves_both_stops_on() {
    // The count is each speaker's (theseus-3ug0): Robin's echo from 1.5 s
    // stops the reply at 1.8 s, it resumes at 2.8 s; the owner's from 3.2 s
    // stops it at 3.5 s, it resumes at 4.5 s. Robin's words from 5.0 s still
    // stop it at 5.3 s.
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 9000)
        .say(ROBIN, 1500, 600)
        .say(OWNER, 3200, 600)
        .say(ROBIN, 5000, 600)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "tell me about it")
            .transcript(ROBIN, "this first sentence runs on for quite a while")
            .transcript(OWNER, "a while long enough to talk over")
            .transcript(ROBIN, "Hang on, wait a second."),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let heard: Vec<_> = utterances(&seen)[1..]
        .iter()
        .map(|(_, u)| (u.speaker, u.heard_as))
        .collect();
    assert_eq!(
        heard,
        [
            (ROBIN, HeardAs::Echo),
            (OWNER, HeardAs::Echo),
            (ROBIN, HeardAs::Words)
        ]
    );
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (ms(2800), len(S1), true),
            (ms(4500), len(S1), true),
        ]
    );
    assert_eq!(played[2].ended, Some(ms(5300)), "Robin's stop is on");
}
