//! The engine on a streaming listener's path (theseus-v774k), through the seam,
//! in virtual time: a stand-in listener scripts Flux's events at ticks (20 ms
//! each), the call's audio is WAV fixtures, and the echo is the room of
//! `tests/aec.rs`.

use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    Audio, Command, Config, CutWhy, Engine, Event, Failure, HeardAs, ListenLog, Listened, Played,
    Speaker, Speech, SpeechFuture, StandInListen, StandInSpeech, Synthesis, Transcript, TurnId,
    Usage, Utterance, WavIo,
};
use tokio::time::{sleep, Instant};

const OWNER: Speaker = Speaker(101);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Tick `n`'s time: 20 ms each.
fn tick_at(ms: u64) -> u64 {
    ms / 20
}

/// The stand-in, with a voice in place of its tones: noise in syllables, so
/// an echo of it has an envelope to be found by (`tests/aec.rs`).
struct Voiced(Arc<StandInSpeech>);

impl Speech for Voiced {
    fn transcribe<'a>(
        &'a self,
        speaker: Speaker,
        audio: &'a Audio,
    ) -> SpeechFuture<'a, Transcript> {
        self.0.transcribe(speaker, audio)
    }

    fn synthesize<'a>(&'a self, text: &'a str) -> SpeechFuture<'a, Synthesis> {
        Box::pin(async move {
            let length = StandInSpeech::tone_for(text).duration();
            Ok(Synthesis {
                audio: Audio::babble(text.len() as u64, length, 0.1),
                usage: Usage {
                    provider: "stand-in".into(),
                    model: "babble".into(),
                    audio: length,
                    chars: text.len(),
                },
            })
        })
    }
}

type Answer = Box<dyn FnMut(TurnId, &[Utterance]) -> (Duration, String) + Send>;

struct Call {
    events: Vec<(Duration, Event)>,
    played: Vec<Played>,
    log: ListenLog,
    speech: Arc<StandInSpeech>,
}

impl Call {
    fn all(&self) -> impl Iterator<Item = &Event> {
        self.events.iter().map(|(_, e)| e)
    }

    fn utterances(&self) -> Vec<&Utterance> {
        self.all()
            .filter_map(|e| match e {
                Event::Utterance(u) => Some(u),
                _ => None,
            })
            .collect()
    }

    fn turns(&self) -> Vec<(Duration, Vec<String>)> {
        self.events
            .iter()
            .filter_map(|(at, e)| match e {
                Event::Turn { utterances, .. } => {
                    Some((*at, utterances.iter().map(|u| u.text.clone()).collect()))
                }
                _ => None,
            })
            .collect()
    }

    fn stopped(&self) -> Option<Duration> {
        self.played.iter().find(|p| p.stopped).and_then(|p| p.ended)
    }

    fn cut_or_barge(&self) -> bool {
        self.all()
            .any(|e| matches!(e, Event::Cut { .. } | Event::BargeIn { .. }))
    }
}

/// A call of `length`, with `io`, a listener scripted by `listen`, the stand-in
/// speech `speech`, and `answer` for the session.
async fn run(
    io: WavIo,
    listen: (StandInListen, ListenLog),
    speech: Arc<StandInSpeech>,
    eager: bool,
    tweak: impl FnOnce(&mut Config),
    mut answer: Answer,
) -> Call {
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    tweak(&mut config);
    let origin = Instant::now();
    let played = io.played();
    let (listen, log) = listen;
    let (engine, handle) = Engine::new(config, Box::new(io), Arc::new(Voiced(speech.clone())));
    let engine = engine.with_listen(Box::new(listen), "flux-general-en", eager);
    let engine = tokio::spawn(engine.run());
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
    Call {
        events: seen,
        played: played.get(),
        log,
        speech,
    }
}

fn silent_after_first(reply: &'static str) -> Answer {
    Box::new(move |turn, _| match turn {
        TurnId(0) => (Duration::ZERO, reply.into()),
        _ => (Duration::ZERO, String::new()),
    })
}

fn nothing() -> Answer {
    Box::new(|_, _| (Duration::ZERO, String::new()))
}

fn start() -> Listened {
    Listened::Start { speaker: OWNER }
}

fn end(text: &str) -> Listened {
    Listened::End {
        speaker: OWNER,
        text: text.into(),
        confidence: Some(0.95),
        streamed: ms(1_000),
    }
}

/// The owner says "book a table" for 0.8 s, is quiet for 2 s, and finishes
/// the sentence for 1 s.
fn paused_sentence() -> WavIo {
    let mut io = WavIo::new(ms(9_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(800), 0.3));
    io.say(OWNER, ms(3_000), Audio::tone(220.0, ms(1_000), 0.3));
    io
}

const SENTENCE: &str = "Book a table for two at eight tonight.";

/// The 2 s pause inside the owner's sentence does not end its turn: Flux
/// decides, and the turn is one utterance with Flux's words, heard as such.
#[tokio::test(start_paused = true)]
async fn a_two_second_pause_inside_a_sentence_is_one_turn() {
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(tick_at(4_300), end(SENTENCE));
    let call = run(
        paused_sentence(),
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |_| {},
        nothing(),
    )
    .await;
    assert_eq!(call.turns().len(), 1, "{:#?}", call.events);
    assert_eq!(call.turns()[0].1, vec![SENTENCE.to_string()]);
    let u = call.utterances();
    assert_eq!(u.len(), 1);
    assert_eq!(u[0].heard_as, HeardAs::Words);
    assert_eq!(u[0].usage.model, "flux-general-en");
    assert_eq!(u[0].usage.audio, ms(1_000), "the audio streamed is booked");
    assert_eq!(
        call.speech.transcriptions(),
        0,
        "no second call transcribes it"
    );
    assert!(
        call.log.frames(OWNER) >= 85,
        "the raw frames went to the listener"
    );
    assert!(
        call.log.closed(),
        "the listener is closed at the call's end"
    );
}

/// The nova-3 path, the same audio: the pause closes the first utterance, so
/// it is two utterances and (the session answering the first) two turns.
#[tokio::test(start_paused = true)]
async fn on_the_pause_rules_the_same_audio_is_two_utterances() {
    let io = paused_sentence();
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(OWNER, "Book a table")
            .transcript(OWNER, "for two at eight tonight."),
    );
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    config.end_of_utterance = ms(1_200);
    let origin = Instant::now();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = tokio::spawn(engine.run());
    let mut events = handle.events;
    let mut texts = Vec::new();
    while let Some(event) = events.recv().await {
        if let Event::Utterance(u) = event {
            texts.push((origin.elapsed(), u.text));
        }
    }
    engine.await.unwrap();
    assert_eq!(texts.len(), 2, "{texts:?}");
}

/// No `merge_ms` wait: a Flux turn without a full stop is a turn at its end.
#[tokio::test(start_paused = true)]
async fn a_turn_without_a_full_stop_does_not_wait_for_the_merge() {
    let mut io = WavIo::new(ms(6_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(800), 0.3));
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(tick_at(1_000), end("and then"));
    let call = run(
        io,
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |c| c.merge = ms(1_500),
        nothing(),
    )
    .await;
    let turns = call.turns();
    assert_eq!(turns.len(), 1);
    assert!(turns[0].0 <= ms(1_100), "the turn came at {:?}", turns[0].0);
}

/// Eager end of turn, resumed: the early turn's reply never plays, and the end
/// makes the whole sentence one turn.
#[tokio::test(start_paused = true)]
async fn a_resumed_eager_end_takes_its_reply_back_and_the_end_is_one_turn() {
    let mut io = WavIo::new(ms(12_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(800), 0.3));
    io.say(OWNER, ms(3_000), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(
            tick_at(1_300),
            Listened::Eager {
                speaker: OWNER,
                text: "Book a table".into(),
                confidence: Some(0.9),
            },
        )
        .at(tick_at(2_300), Listened::Resumed { speaker: OWNER })
        .at(tick_at(4_300), end(SENTENCE));
    // The session needs 2.5 s to answer the early turn: its reply is ready
    // after the speaker went on.
    let answer: Answer = Box::new(|turn, _| match turn {
        TurnId(0) => (ms(2_500), "Sure, for how many?".into()),
        _ => (Duration::ZERO, "Done.".into()),
    });
    let call = run(
        io,
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        true,
        |_| {},
        answer,
    )
    .await;
    let turns = call.turns();
    assert_eq!(turns.len(), 2, "{:#?}", call.events);
    assert_eq!(turns[0].1, vec!["Book a table".to_string()]);
    assert_eq!(turns[1].1, vec![SENTENCE.to_string()]);
    let superseded = call.all().any(|e| {
        matches!(
            e,
            Event::Cut {
                what: theseus_voice::Spoken::Reply(TurnId(0)),
                why: CutWhy::Superseded,
                ..
            }
        )
    });
    assert!(superseded, "{:#?}", call.events);
    // Only the second turn's reply, "Done.", was ever played.
    assert_eq!(call.played.len(), 1, "{:#?}", call.played);
    assert!(call.played[0].started >= ms(4_300));
}

/// Eager end of turn, confirmed: the end starts nothing more, and the reply
/// begins before the confirmation.
#[tokio::test(start_paused = true)]
async fn a_confirmed_eager_end_is_one_turn_and_its_reply_does_not_wait() {
    let mut io = WavIo::new(ms(9_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(
            tick_at(1_300),
            Listened::Eager {
                speaker: OWNER,
                text: SENTENCE.into(),
                confidence: Some(0.9),
            },
        )
        .at(tick_at(1_900), end(SENTENCE));
    let call = run(
        io,
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        true,
        |_| {},
        silent_after_first("Table for two, eight tonight."),
    )
    .await;
    assert_eq!(call.turns().len(), 1, "{:#?}", call.events);
    assert_eq!(call.utterances().len(), 1);
    assert_eq!(call.played.len(), 1);
    assert!(
        call.played[0].started < ms(1_900),
        "the reply began at {:?}, before the confirmation",
        call.played[0].started
    );
}

/// An eager event is ignored when it was not asked for.
#[tokio::test(start_paused = true)]
async fn without_eager_only_the_end_starts_a_turn() {
    let mut io = WavIo::new(ms(4_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(
            tick_at(1_300),
            Listened::Eager {
                speaker: OWNER,
                text: "Book".into(),
                confidence: None,
            },
        )
        .at(tick_at(1_900), end(SENTENCE));
    let call = run(
        io,
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |_| {},
        nothing(),
    )
    .await;
    assert_eq!(call.turns().len(), 1);
    assert_eq!(call.turns()[0].1, vec![SENTENCE.to_string()]);
}

/// The owner says something at 0.2 s (turn 0), Theseus answers with two long
/// sentences (about 8 s), and the owner's speakers send it back, 180 ms late
/// and 12 dB down. From `talks` the owner talks over it, if they do.
fn over_a_reply(talks: Option<u64>) -> WavIo {
    let mut io = WavIo::new(ms(14_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(600), 0.3));
    io.echo(OWNER, ms(180), 12.0, 0.6);
    if let Some(at) = talks {
        io.say(OWNER, ms(at), Audio::tone(310.0, ms(2_000), 0.15));
    }
    io
}

fn reply() -> Answer {
    silent_after_first("This first sentence runs on for quite a while, long enough to talk over. The second sentence is long enough to be cut late, if it comes to that.")
}

/// A start of turn with the owner's voice over the echo stops the clip at once,
/// not 300 ms later, and the words commit the cut. The time to the stop is in
/// the events.
#[tokio::test(start_paused = true)]
async fn a_start_of_turn_over_speech_stops_the_clip_at_once_and_its_words_cut_it() {
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(tick_at(800), end("Go ahead."))
        .at(tick_at(6_100), start())
        .at(tick_at(8_000), end("Wait, stop, that is not right."));
    let call = run(
        over_a_reply(Some(6_000)),
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |c| c.echo_cancel = true,
        reply(),
    )
    .await;
    let stopped = call.stopped().expect("the reply was stopped");
    assert!(
        stopped >= ms(6_100) && stopped <= ms(6_160),
        "the start of turn came at 6.1 s and the clip stopped at {stopped:?}"
    );
    let barge = call
        .all()
        .any(|e| matches!(e, Event::BargeIn { dropped, .. } if *dropped >= 1));
    assert!(barge, "{:#?}", call.events);
    let timed: Vec<_> = call
        .all()
        .filter_map(|e| match e {
            Event::StopLatency {
                to_stop, to_ended, ..
            } => Some((*to_stop, *to_ended)),
            _ => None,
        })
        .collect();
    assert_eq!(timed.len(), 1, "{:#?}", call.events);
    let (to_stop, to_ended) = timed[0];
    assert!(to_stop.is_some_and(|d| d <= ms(40)), "{to_stop:?}");
    assert!(to_ended.is_some_and(|d| d <= ms(60)), "{to_ended:?}");
    let turns = call.turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(
        turns[1].1,
        vec!["Wait, stop, that is not right.".to_string()]
    );
}

/// Theseus's own voice back through the owner's speakers makes the listener
/// start a turn, but nobody spoke: the echo canceller's residual is silent, so
/// the clip is not stopped, and the words, Theseus's own, are an echo.
#[tokio::test(start_paused = true)]
async fn a_start_of_turn_on_the_echo_alone_does_not_stop_the_clip() {
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(tick_at(800), end("Go ahead."))
        .at(tick_at(6_100), start())
        .at(
            tick_at(8_000),
            end("The second sentence is long enough to be cut late, if it comes to that."),
        );
    let call = run(
        over_a_reply(None),
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |c| c.echo_cancel = true,
        reply(),
    )
    .await;
    assert!(call.stopped().is_none(), "{:#?}", call.played);
    assert!(!call.cut_or_barge(), "{:#?}", call.events);
    assert_eq!(call.played.len(), 2, "both sentences played to their end");
}

/// A cough over the reply: the clip is held at once, the listener's turn has
/// no words, and the sentence is said again from its start.
#[tokio::test(start_paused = true)]
async fn a_start_of_turn_with_no_words_resumes_the_clip() {
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(tick_at(800), end("Go ahead."))
        .at(tick_at(6_100), start())
        .at(tick_at(6_500), end(""));
    let mut io = over_a_reply(None);
    io.say(OWNER, ms(6_000), Audio::tone(310.0, ms(400), 0.15));
    let call = run(
        io,
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |c| c.echo_cancel = true,
        reply(),
    )
    .await;
    let resumed = call.all().any(|e| {
        matches!(
            e,
            Event::Resumed {
                why: HeardAs::Wordless,
                ..
            }
        )
    });
    assert!(resumed, "{:#?}", call.events);
    assert!(!call.cut_or_barge(), "{:#?}", call.events);
    assert!(call.stopped().is_some(), "the clip was held");
    assert!(
        call.played.len() >= 3,
        "the cut sentence began again: {:#?}",
        call.played
    );
}

/// With echo cancellation off the raw frame counts, as it always did.
#[tokio::test(start_paused = true)]
async fn with_echo_cancel_off_the_raw_frame_counts() {
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(tick_at(800), end("Go ahead."))
        .at(tick_at(6_100), start())
        .at(tick_at(8_000), end("Wait, stop, that is not right."));
    let call = run(
        over_a_reply(Some(6_000)),
        (script, listen.1),
        Arc::new(StandInSpeech::new()),
        false,
        |c| c.echo_cancel = false,
        reply(),
    )
    .await;
    let stopped = call.stopped().expect("the reply was stopped");
    assert!(stopped <= ms(6_160), "{stopped:?}");
}

/// The stream drops in the middle of a turn: its audio is transcribed once by
/// the pre-recorded recogniser, the turn happens, and the next turn, after the
/// stream is back, is the listener's.
#[tokio::test(start_paused = true)]
async fn a_stream_that_drops_mid_turn_loses_nothing() {
    let mut io = WavIo::new(ms(12_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(1_600), 0.3));
    io.say(OWNER, ms(8_000), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen
        .0
        .at(tick_at(220), start())
        .at(
            tick_at(2_000),
            Listened::Down {
                speaker: OWNER,
                error: "reset by peer".into(),
            },
        )
        .at(tick_at(5_000), Listened::Up { speaker: OWNER })
        .at(tick_at(8_100), start())
        .at(tick_at(9_300), end("Next one."));
    let speech =
        Arc::new(StandInSpeech::new().transcript(OWNER, "The turn the stream dropped in."));
    let call = run(io, (script, listen.1), speech, false, |_| {}, nothing()).await;
    assert_eq!(call.speech.transcriptions(), 1, "the turn's audio, once");
    let texts: Vec<_> = call.turns().into_iter().flat_map(|(_, t)| t).collect();
    assert_eq!(
        texts,
        vec![
            "The turn the stream dropped in.".to_string(),
            "Next one.".to_string()
        ]
    );
    let failed = call
        .all()
        .filter(|e| {
            matches!(
                e,
                Event::Failed {
                    what: Failure::Flux,
                    ..
                }
            )
        })
        .count();
    assert_eq!(failed, 1, "said once per drop");
}

/// While a stream is down, the speaker is heard by the pause rules: a turn
/// is not lost between a drop and the stream's return.
#[tokio::test(start_paused = true)]
async fn while_a_stream_is_down_the_pause_rules_hear_the_speaker() {
    let mut io = WavIo::new(ms(8_000));
    io.say(OWNER, ms(1_000), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen.0.at(
        5,
        Listened::Down {
            speaker: OWNER,
            error: "reset by peer".into(),
        },
    );
    let speech = Arc::new(StandInSpeech::new().transcript(OWNER, "Heard without the stream."));
    let call = run(io, (script, listen.1), speech, false, |_| {}, nothing()).await;
    let texts: Vec<_> = call.turns().into_iter().flat_map(|(_, t)| t).collect();
    assert_eq!(texts, vec!["Heard without the stream.".to_string()]);
}

/// A listener that cannot be reached at all: the call goes on with the pause
/// rules, and says so once.
#[tokio::test(start_paused = true)]
async fn a_listener_that_cannot_be_reached_leaves_the_call_on_the_pause_rules() {
    let mut io = WavIo::new(ms(8_000));
    io.say(OWNER, ms(1_000), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen.0.at(
        1,
        Listened::Unavailable {
            error: "Deepgram Flux answered 401".into(),
        },
    );
    let speech = Arc::new(StandInSpeech::new().transcript(OWNER, "Heard without Flux."));
    let call = run(io, (script, listen.1), speech, false, |_| {}, nothing()).await;
    let texts: Vec<_> = call.turns().into_iter().flat_map(|(_, t)| t).collect();
    assert_eq!(texts, vec!["Heard without Flux.".to_string()]);
    let failed: Vec<_> = call
        .all()
        .filter_map(|e| match e {
            Event::Failed {
                what: Failure::Flux,
                error,
            } => Some(error.0.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(failed.len(), 1, "said once");
    assert!(failed[0].contains("401"), "{failed:?}");
}

/// A turn the listener never ends does not hold the floor for ever: after
/// 90 s its audio is transcribed once and the turn happens.
#[tokio::test(start_paused = true)]
async fn a_turn_the_listener_never_ends_is_given_up_on() {
    let mut io = WavIo::new(ms(100_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(1_000), 0.3));
    let listen = StandInListen::new();
    let script = listen.0.at(tick_at(220), start());
    let speech = Arc::new(StandInSpeech::new().transcript(OWNER, "Said, and never ended."));
    let call = run(io, (script, listen.1), speech, false, |_| {}, nothing()).await;
    let texts: Vec<_> = call.turns().into_iter().flat_map(|(_, t)| t).collect();
    assert_eq!(texts, vec!["Said, and never ended.".to_string()]);
    let at = call.turns()[0].0;
    assert!(at >= ms(90_000) && at <= ms(91_500), "{at:?}");
}
