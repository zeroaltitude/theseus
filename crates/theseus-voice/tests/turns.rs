//! A barge-in that pauses until words decide, and replies that wait for the
//! speaker (theseus-9ln5, theseus-kpa7): through the seam, from WAV fixtures,
//! in virtual time, each at its exact times.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    write_wav, Audio, Command, Config, CutWhy, Engine, Event, Failure, HeardAs, Over, Played,
    Speaker, Speech, SpeechError, SpeechFuture, Spoken, StandInSpeech, Synthesis, Transcript,
    TurnId, Utterance, WavIo,
};
use tokio::time::{sleep, sleep_until, Instant};

const EDDIE: Speaker = Speaker(101);
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
    let mut config = Config::new([EDDIE, ROBIN]);
    config.acknowledge_after = Duration::from_secs(60);
    call_with(config, io, speech, answer, reports).await
}

async fn call_with(
    config: Config,
    io: WavIo,
    speech: Arc<dyn Speech>,
    mut answer: Answer,
    reports: Vec<(Duration, &'static str)>,
) -> (Vec<(Duration, Event)>, Vec<Played>) {
    let origin = Instant::now();
    let played = io.played();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = tokio::spawn(engine.run());
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

/// Eddie asks from 0 to 0.5 s: his turn is at 1.2 s, and its reply's first
/// sentence plays from 1.2 s.
fn asked(dir: &Path, length: u64) -> Lines<'_> {
    Lines::new(dir, length).say(EDDIE, 0, 500)
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
    assert_eq!(turns(&seen).len(), 1, "only Eddie's question");
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
async fn a_yeah_after_a_closing_question_is_a_turn() {
    let dir = tempfile::tempdir().unwrap();
    let reply = "Here is the plan. Should I deploy it now?";
    let end = ms(1200) + len("Here is the plan.") + len("Should I deploy it now?");
    let at = end.as_millis() as u64 + 500;
    let io = asked(dir.path(), 8000).say(EDDIE, at, 400).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "what's the plan?")
            .transcript(EDDIE, "Yeah."),
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
            (ms(1200), vec![(EDDIE, "what's the plan?")]),
            (closed, vec![(EDDIE, "Yeah.")]),
        ]
    );
    let (_, yeah) = utterances(&seen)[1];
    assert_eq!((yeah.over.clone(), yeah.heard_as), (None, HeardAs::Words));
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
        .say(EDDIE, 2000, 500)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "tell me about it")
            .transcript(ROBIN, "")
            .transcript(EDDIE, "Go on."),
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
    // Eddie's words from then.
    let io = Lines::new(dir.path(), 9000).say(EDDIE, 1200, 600).io;
    let speech = Arc::new(StandInSpeech::new().transcript(EDDIE, "hold on, what's that"));
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
                    speaker: EDDIE,
                    what: Spoken::Report,
                    dropped: 2,
                }
            ),
        ]
    );
    // Eddie's turn is answered at 3.0 s; at the pause after, the report
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
    assert_eq!(turn, &[(EDDIE, "hold on, what's that")]);
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
async fn after_an_echo_its_speaker_doesnt_stop_it_and_their_words_cut_late() {
    let dir = tempfile::tempdir().unwrap();
    // An echo from 1.5 s stops it; it resumes at 2.8 s. A second echo from
    // 3.2 s doesn't stop it. Words over the second sentence cut at their
    // transcript.
    let s2_at = ms(2800) + len(S1);
    let words_at = s2_at.as_millis() as u64 + 200;
    let io = asked(dir.path(), 14_000)
        .say(ROBIN, 1500, 600)
        .say(ROBIN, 3200, 600)
        .say(ROBIN, words_at, 600)
        .io;
    let echo = "this first sentence runs on for quite a while";
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(ROBIN, echo)
            .transcript(ROBIN, "long enough to talk over this first sentence")
            .transcript(ROBIN, "hang on a moment please"),
    );
    let (seen, played) = call(io, speech, first(three()), vec![]).await;
    let cut_at = ms(words_at + 600 + 700);
    assert_eq!(
        starts(&played),
        [
            (ms(1200), len(S1), true),
            (ms(2800), len(S1), false),
            (s2_at, len(S2), true),
        ]
    );
    assert_eq!(played[2].ended, Some(cut_at));
    let heard: Vec<_> = utterances(&seen)
        .into_iter()
        .filter(|(_, u)| u.speaker == ROBIN)
        .map(|(_, u)| u.heard_as)
        .collect();
    assert_eq!(heard, [HeardAs::Echo, HeardAs::Echo, HeardAs::Words]);
    assert_eq!(only(&seen, resumed).len(), 1);
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

/// Each turn answered after its own delay, with its own text.
fn answers(by_turn: &'static [(u64, &'static str)]) -> Answer {
    Box::new(move |turn, _| {
        let (delay, text) = by_turn.get(turn.0 as usize).copied().unwrap_or((0, ""));
        (ms(delay), text.into())
    })
}

#[tokio::test(start_paused = true)]
async fn a_reply_waits_for_the_speaker_and_is_superseded_by_their_words() {
    // Eddie asks; his turn is at 1.2 s, and its reply comes at 3.0 s, while
    // he talks again from 2.5 s to 3.5 s. His utterance closes at 4.2 s.
    let reply = "Here are the logs.";
    let script: &'static [(u64, &'static str)] = &[(1800, "Here are the logs.")];
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 8000).say(EDDIE, 2500, 1000).io;
    // A cough: the reply plays when it closes.
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "show me the logs")
            .transcript(EDDIE, ""),
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
    let io = asked(dir.path(), 8000).say(EDDIE, 2500, 1000).io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "show me the logs")
            .transcript(EDDIE, "only the errors, I mean"),
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
        (ms(4200), vec![(EDDIE, "only the errors, I mean")])
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
        .say(EDDIE, 0, 1000)
        .say(EDDIE, 1800, 1000)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "Can you make yourself a")
            .transcript(EDDIE, "tool to order"),
    );
    let (seen, played) = call(io, speech, answers(script), vec![]).await;
    assert_eq!(
        turns(&seen),
        [
            (ms(1700), vec![(EDDIE, "Can you make yourself a")]),
            (ms(4200), vec![(EDDIE, "tool to order")]),
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
        .say(EDDIE, 0, 1000)
        .say(EDDIE, 4000, 1000)
        .io;
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "what changed today?")
            .transcript(EDDIE, "and who changed it?"),
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
            (ms(1700), vec![(EDDIE, "what changed today?")]),
            (ms(6200), vec![(EDDIE, "and who changed it?")]),
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
    let config = Config::new([EDDIE, ROBIN]);
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
        Config::new([EDDIE]),
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
