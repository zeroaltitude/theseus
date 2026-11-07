//! `Command::Mute` (theseus-k2sw), through the seam, in virtual time: while
//! the call is muted the listener hears nothing, whatever the path
//! (the pause rules' VAD or the streaming listener), what it had open is
//! dropped, and Theseus's own speech goes on, never cut by what is said over
//! it.

use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    Audio, Command, Config, Engine, Event, ListenLog, Listened, Played, Speaker, StandInListen,
    StandInSpeech, TurnId, Utterance, WavIo, FRAME,
};
use tokio::time::{sleep_until, Instant};

const OWNER: Speaker = Speaker(101);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The tick a time falls in: `FRAME` each.
fn tick_at(ms: u64) -> u64 {
    ms / FRAME.as_millis() as u64
}

fn tone(io: &mut WavIo, at: u64, length: u64) {
    io.say(OWNER, ms(at), Audio::tone(220.0, ms(length), 0.3));
}

struct Call {
    events: Vec<Event>,
    played: Vec<Played>,
    speech: Arc<StandInSpeech>,
    log: Option<ListenLog>,
}

impl Call {
    fn utterances(&self) -> Vec<&Utterance> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Utterance(u) => Some(u),
                _ => None,
            })
            .collect()
    }

    fn turns(&self) -> Vec<Vec<String>> {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Turn { utterances, .. } => {
                    Some(utterances.iter().map(|u| u.text.clone()).collect())
                }
                _ => None,
            })
            .collect()
    }

    fn streamed(&self) -> Duration {
        self.events
            .iter()
            .filter_map(|e| match e {
                Event::Streamed { usage, .. } => Some(usage.audio),
                _ => None,
            })
            .sum()
    }
}

/// A call on `io`, muted and unmuted at the times given (`true` mutes), the
/// session answering a turn with `reply` after it, and a streaming listener
/// when `listen` is given.
async fn run(
    io: WavIo,
    speech: Arc<StandInSpeech>,
    listen: Option<(StandInListen, ListenLog)>,
    mutes: &[(u64, bool)],
    reply: &'static str,
) -> Call {
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    let origin = Instant::now();
    let played = io.played();
    let (engine, handle) = Engine::new(config, Box::new(io), speech.clone());
    let (engine, log) = match listen {
        Some((listen, log)) => (
            engine.with_listen(Box::new(listen), "flux-general-en", false),
            Some(log),
        ),
        None => (engine, None),
    };
    let engine = tokio::spawn(engine.run());
    for &(at, on) in mutes {
        let commands = handle.commands.clone();
        tokio::spawn(async move {
            sleep_until(origin + ms(at)).await;
            let _ = commands.send(Command::Mute(on));
        });
    }
    let mut events = handle.events;
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        if let Event::Turn { id, .. } = &event {
            let (turn, commands) = (*id, handle.commands.clone());
            let text = if *id == TurnId(0) { reply } else { "" };
            let _ = commands.send(Command::Reply {
                turn,
                text: text.into(),
            });
        }
        seen.push(event);
    }
    engine.await.expect("the engine ran to the call's end");
    Call {
        events: seen,
        played: played.get(),
        speech,
        log,
    }
}

/// On the pause rules' path: said before the mute is heard, said while muted
/// is not, said after the unmute is heard again. The muted audio is never
/// transcribed.
#[tokio::test(start_paused = true)]
async fn a_muted_call_hears_nothing_and_an_unmuted_one_hears_again() {
    let mut io = WavIo::new(ms(13_000));
    tone(&mut io, 200, 1_000);
    tone(&mut io, 4_000, 1_000);
    tone(&mut io, 9_000, 1_000);
    let speech = Arc::new(StandInSpeech::new());
    let call = run(io, speech, None, &[(3_000, true), (8_000, false)], "").await;
    assert_eq!(call.utterances().len(), 2, "{:#?}", call.events);
    assert_eq!(call.turns().len(), 2);
    assert_eq!(call.speech.transcriptions(), 2, "muted audio goes nowhere");
}

/// A sentence the mute lands in the middle of is dropped whole: no turn, and
/// its audio never goes to the recogniser.
#[tokio::test(start_paused = true)]
async fn a_mute_in_the_middle_of_a_sentence_drops_it() {
    let mut io = WavIo::new(ms(10_000));
    tone(&mut io, 200, 3_000);
    let speech = Arc::new(StandInSpeech::new());
    let call = run(io, speech, None, &[(1_000, true), (6_000, false)], "").await;
    assert!(call.utterances().is_empty(), "{:#?}", call.events);
    assert!(call.turns().is_empty());
    assert_eq!(call.speech.transcriptions(), 0);
}

/// The streaming listener: nothing but silence reaches it while muted, and the
/// turn it had open when the mute landed ends unheard, whether its end comes
/// during the mute or after the unmute: no turn, no words in any event, its
/// audio booked. The turn after the unmute is heard.
async fn streaming(end_at: u64) -> Call {
    let mut io = WavIo::new(ms(13_000));
    tone(&mut io, 200, 3_000);
    tone(&mut io, 9_000, 1_000);
    let (listen, log) = StandInListen::new();
    let script = listen
        .at(tick_at(220), Listened::Start { speaker: OWNER })
        .at(
            tick_at(end_at),
            Listened::End {
                speaker: OWNER,
                text: "not for you".into(),
                confidence: Some(0.95),
                streamed: ms(1_000),
            },
        )
        .at(tick_at(9_020), Listened::Start { speaker: OWNER })
        .at(
            tick_at(10_400),
            Listened::End {
                speaker: OWNER,
                text: "hello again".into(),
                confidence: Some(0.95),
                streamed: ms(1_400),
            },
        );
    let speech = Arc::new(StandInSpeech::new());
    run(
        io,
        speech,
        Some((script, log)),
        &[(1_000, true), (3_000, false)],
        "",
    )
    .await
}

fn silent_while_muted(call: &Call) {
    let log = call.log.as_ref().expect("a streaming call");
    let loud: Vec<u64> = log
        .pushes(OWNER)
        .into_iter()
        .filter(|(t, zero)| !zero && (tick_at(1_000) + 1..tick_at(3_000)).contains(t))
        .map(|(t, _)| t)
        .collect();
    assert!(loud.is_empty(), "real audio reached the listener: {loud:?}");
}

fn only_the_unmuted_turn(call: &Call) {
    assert_eq!(call.turns(), vec![vec!["hello again".to_string()]]);
    for u in call.utterances() {
        assert_ne!(u.text, "not for you", "{u:?}");
    }
    assert_eq!(
        call.streamed() + call.utterances().iter().map(|u| u.usage.audio).sum::<Duration>(),
        ms(2_400),
        "the muted turn's audio is booked, not dropped: {:#?}",
        call.events
    );
}

#[tokio::test(start_paused = true)]
async fn a_streaming_turn_that_ends_while_muted_is_dropped() {
    let call = streaming(2_000).await;
    silent_while_muted(&call);
    only_the_unmuted_turn(&call);
}

#[tokio::test(start_paused = true)]
async fn a_streaming_turn_that_ends_after_the_unmute_is_dropped_too() {
    let call = streaming(3_500).await;
    silent_while_muted(&call);
    only_the_unmuted_turn(&call);
}

/// Mute is for the listener: what Theseus is saying goes on to its end, and
/// the owner talking over it, muted, is no barge-in and no cut.
#[tokio::test(start_paused = true)]
async fn a_muted_call_is_not_cut_by_talk_over_the_reply() {
    let mut io = WavIo::new(ms(14_000));
    tone(&mut io, 200, 1_000);
    tone(&mut io, 5_000, 2_000);
    let speech = Arc::new(StandInSpeech::new());
    let reply = "This reply runs on for quite a while, long enough to be talked over by someone.";
    let call = run(io, speech, None, &[(3_000, true)], reply).await;
    assert!(
        call.events
            .iter()
            .all(|e| !matches!(e, Event::BargeIn { .. } | Event::Cut { .. })),
        "{:#?}",
        call.events
    );
    assert!(!call.played.is_empty(), "the reply played");
    assert!(call.played.iter().all(|p| !p.stopped), "{:#?}", call.played);
}
