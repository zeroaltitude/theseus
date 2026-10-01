//! Step 44a's tests (design §3): the pipeline through the seam, from WAV
//! fixtures, in virtual time. tokio's clock is paused, so a test's seconds
//! of call take milliseconds, and its timings are exact under any load.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    write_wav, Audio, Command, Config, Engine, Event, Played, Speaker, Spoken, StandInSpeech,
    TurnId, Utterance, WavIo,
};
use tokio::time::{sleep, sleep_until, Instant};

const EDDIE: Speaker = Speaker(101);
const ROBIN: Speaker = Speaker(202);
const STRANGER: Speaker = Speaker(303);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A WAV fixture: speech (a 220 Hz tone, well over the VAD's threshold) for
/// each `(true, ms)` span, and silence for each `(false, ms)`.
fn fixture(dir: &Path, name: &str, spans: &[(bool, u64)]) -> PathBuf {
    let parts: Vec<Audio> = spans
        .iter()
        .map(|&(speech, length)| match speech {
            true => Audio::tone(220.0, ms(length), 0.3),
            false => Audio::silence(ms(length)),
        })
        .collect();
    let path = dir.join(format!("{name}.wav"));
    write_wav(&path, &Audio::concat(&parts)).expect("write the fixture");
    path
}

/// How the session answers a turn: after a delay, with a reply's text.
type Answer = Box<dyn FnMut(TurnId, &[Utterance]) -> (Duration, String) + Send>;

/// A call in virtual time: the engine on a task of its own, and the session
/// played here. Returns every event with when it came, and what was played.
async fn call(
    io: WavIo,
    config: Config,
    speech: Arc<StandInSpeech>,
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

/// Every turn answered at once, in silence.
fn silent() -> Answer {
    Box::new(|_, _| (Duration::ZERO, String::new()))
}

fn utterances(seen: &[(Duration, Event)]) -> Vec<&Utterance> {
    seen.iter()
        .filter_map(|(_, e)| match e {
            Event::Utterance(u) => Some(u),
            _ => None,
        })
        .collect()
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

#[tokio::test(start_paused = true)]
async fn an_utterance_ends_after_700_ms_of_silence() {
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(6000));
    io.say_wav(
        EDDIE,
        ms(0),
        &fixture(dir.path(), "one-second", &[(true, 1000)]),
    )
    .unwrap();
    // A 600 ms pause inside an utterance doesn't end it.
    let paused = fixture(
        dir.path(),
        "with-a-pause",
        &[(true, 600), (false, 600), (true, 600)],
    );
    io.say_wav(EDDIE, ms(3000), &paused).unwrap();
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "first")
            .transcript(EDDIE, "second"),
    );
    let (seen, _) = call(io, Config::new([EDDIE]), speech, silent(), vec![]).await;

    let said = utterances(&seen);
    assert_eq!(said.len(), 2, "{said:?}");
    let times = |u: &Utterance| (u.started, u.length, u.closed);
    assert_eq!(times(said[0]), (ms(0), ms(1000), ms(1700)));
    assert_eq!(said[0].text, "first");
    assert_eq!(times(said[1]), (ms(3000), ms(1800), ms(5500)));
    assert_eq!(said[1].text, "second");
    // Each came the moment its 700 ms of silence ended, not a frame before.
    let came: Vec<_> = seen
        .iter()
        .filter(|(_, e)| matches!(e, Event::Utterance(_)))
        .map(|(at, _)| *at)
        .collect();
    assert_eq!(came, [ms(1700), ms(5500)]);
}

#[tokio::test(start_paused = true)]
async fn two_speakers_are_kept_apart() {
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(6000));
    io.say_wav(EDDIE, ms(0), &fixture(dir.path(), "eddie", &[(true, 1000)]))
        .unwrap()
        .say_wav(
            ROBIN,
            ms(500),
            &fixture(dir.path(), "robin", &[(true, 1500)]),
        )
        .unwrap();
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "hello from eddie")
            .transcript(ROBIN, "and robin here"),
    );
    let (seen, _) = call(io, Config::new([EDDIE, ROBIN]), speech, silent(), vec![]).await;

    // Overlapping speech, two utterances: each its own speaker's audio
    // alone, closed by its own silence, transcribed as its own.
    let said: Vec<_> = utterances(&seen)
        .into_iter()
        .map(|u| (u.speaker, u.started, u.length, u.closed, u.text.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            (EDDIE, ms(0), ms(1000), ms(1700), "hello from eddie"),
            (ROBIN, ms(500), ms(1500), ms(2700), "and robin here"),
        ]
    );
    assert_eq!(
        turns(&seen),
        [
            (ms(1700), vec![(EDDIE, "hello from eddie")]),
            (ms(2700), vec![(ROBIN, "and robin here")]),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn utterances_during_a_turn_coalesce_into_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let short = fixture(dir.path(), "short", &[(true, 500)]);
    let aside = fixture(dir.path(), "aside", &[(true, 400)]);
    let mut io = WavIo::new(ms(8000));
    io.say_wav(EDDIE, ms(0), &short)
        .unwrap()
        // Both close while turn 0 is in flight (1.2 s to 4.2 s).
        .say_wav(ROBIN, ms(1600), &aside)
        .unwrap()
        .say_wav(EDDIE, ms(2200), &aside)
        .unwrap();
    let speech = Arc::new(
        StandInSpeech::new()
            .transcript(EDDIE, "what changed?")
            .transcript(ROBIN, "the deploy")
            .transcript(EDDIE, "and the tests"),
    );
    let mut config = Config::new([EDDIE, ROBIN]);
    config.acknowledge_after = Duration::from_secs(60);
    let answer: Answer = Box::new(|turn, _| match turn {
        TurnId(0) => (ms(3000), "Two things.".into()),
        _ => (Duration::ZERO, String::new()),
    });
    let (seen, _) = call(io, config, speech, answer, vec![]).await;

    assert_eq!(
        turns(&seen),
        [
            (ms(1200), vec![(EDDIE, "what changed?")]),
            // The reply ends turn 0 at 4.2 s; the two that closed during it
            // (at 2.7 s and 3.3 s) are the next turn, in that order.
            (
                ms(4200),
                vec![(ROBIN, "the deploy"), (EDDIE, "and the tests")]
            ),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_barge_in_stops_playback_within_300_ms() {
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(8000));
    io.say_wav(EDDIE, ms(0), &fixture(dir.path(), "ask", &[(true, 500)]))
        .unwrap()
        // A listed speaker's 200 ms over the reply: too short to stop it.
        .say_wav(ROBIN, ms(1500), &fixture(dir.path(), "hm", &[(true, 200)]))
        .unwrap()
        // Eddie talks over it from 2.0 s.
        .say_wav(
            EDDIE,
            ms(2000),
            &fixture(dir.path(), "wait", &[(true, 600)]),
        )
        .unwrap();
    let speech = Arc::new(StandInSpeech::new());
    let reply = "This first sentence of a long reply runs on for a good few seconds. \
                 This second sentence is never heard.";
    let answer: Answer = Box::new(move |turn, _| match turn {
        TurnId(0) => (Duration::ZERO, reply.into()),
        _ => (Duration::ZERO, String::new()),
    });
    let (seen, played) = call(io, Config::new([EDDIE, ROBIN]), speech, answer, vec![]).await;

    // The first sentence started at 1.2 s, and stopped 300 ms into Eddie's
    // speech; the second never played.
    assert_eq!(played.len(), 1, "{played:?}");
    assert_eq!(played[0].started, ms(1200));
    assert!(played[0].stopped);
    let stopped = played[0].ended.expect("it ended");
    assert!(stopped - ms(2000) <= ms(300), "stopped at {stopped:?}");
    assert_eq!(stopped, ms(2300));
    let barge_ins: Vec<_> = seen
        .iter()
        .filter(|(_, e)| matches!(e, Event::BargeIn { .. }))
        .collect();
    assert_eq!(
        barge_ins,
        [&(
            ms(2300),
            Event::BargeIn {
                speaker: EDDIE,
                what: Spoken::Reply(TurnId(0)),
                dropped: 2
            }
        )]
    );
    // What Eddie said over it is still heard: the next turn.
    assert!(turns(&seen)
        .iter()
        .any(|(at, who)| *at == ms(3300) && who.len() == 1 && who[0].0 == EDDIE));
}

#[tokio::test(start_paused = true)]
async fn the_acknowledgment_comes_after_2_s() {
    let dir = tempfile::tempdir().unwrap();
    let ask = fixture(dir.path(), "ask", &[(true, 500)]);
    let mut io = WavIo::new(ms(10_000));
    io.say_wav(EDDIE, ms(0), &ask)
        .unwrap()
        .say_wav(EDDIE, ms(6000), &ask)
        .unwrap();
    let speech = Arc::new(StandInSpeech::new());
    // Turn 0 takes 3 s, turn 1 takes 1.5 s.
    let answer: Answer = Box::new(|turn, _| match turn {
        TurnId(0) => (ms(3000), "Here you go.".into()),
        _ => (ms(1500), "Done.".into()),
    });
    let (seen, played) = call(io, Config::new([EDDIE]), speech, answer, vec![]).await;

    let acks: Vec<_> = seen
        .iter()
        .filter(|(_, e)| matches!(e, Event::Acknowledged { .. }))
        .collect();
    // Turn 0 began at 1.2 s: its acknowledgment came at 3.2 s, exactly 2 s
    // on. Turn 1 began at 7.2 s and was answered in 1.5 s: none.
    assert_eq!(acks, [&(ms(3200), Event::Acknowledged { turn: TurnId(0) })]);
    let starts: Vec<_> = played.iter().map(|p| (p.started, p.length)).collect();
    assert_eq!(
        starts,
        [
            (ms(3200), Audio::chime().duration()),
            (ms(4200), StandInSpeech::tone_for("Here you go.").duration()),
            (ms(8700), StandInSpeech::tone_for("Done.").duration()),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_report_waits_for_the_pause() {
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(7000));
    io.say_wav(EDDIE, ms(0), &fixture(dir.path(), "long", &[(true, 1500)]))
        .unwrap();
    let speech = Arc::new(StandInSpeech::new());
    let answer: Answer = Box::new(|_, _| (ms(500), "Sure.".into()));
    let reports = vec![
        // Mid-speech: it waits for the utterance, its turn, and the reply.
        (ms(500), "The deploy finished."),
        // At a pause: it plays at once.
        (ms(5000), "The tests passed."),
    ];
    let (seen, played) = call(io, Config::new([EDDIE]), speech, answer, reports).await;

    let reply = StandInSpeech::tone_for("Sure.").duration();
    let first_report = StandInSpeech::tone_for("The deploy finished.").duration();
    let starts: Vec<_> = played.iter().map(|p| (p.started, p.length)).collect();
    assert_eq!(
        starts,
        [
            // Eddie spoke until 1.5 s and his turn ran 2.2 s to 2.7 s: the
            // reply came first, and the report at the pause after it.
            (ms(2700), reply),
            (ms(2700) + reply, first_report),
            (
                ms(5000),
                StandInSpeech::tone_for("The tests passed.").duration()
            ),
        ]
    );
    let spoken: Vec<_> = seen
        .iter()
        .filter_map(|(at, e)| match e {
            Event::Speaking { what, .. } => Some((*at, *what)),
            _ => None,
        })
        .collect();
    assert_eq!(
        spoken,
        [
            (ms(2700), Spoken::Reply(TurnId(0))),
            (ms(2700) + reply, Spoken::Report),
            (ms(5000), Spoken::Report),
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn an_unlisted_speaker_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let talk = fixture(dir.path(), "talk", &[(true, 1000)]);
    let mut io = WavIo::new(ms(7000));
    io.say_wav(STRANGER, ms(0), &talk)
        .unwrap()
        .say_wav(EDDIE, ms(1500), &fixture(dir.path(), "ask", &[(true, 500)]))
        .unwrap()
        // Over the reply too: no barge-in.
        .say_wav(STRANGER, ms(3000), &talk)
        .unwrap();
    let speech = Arc::new(StandInSpeech::new());
    let reply = "A reply long enough to be talked over by a stranger.";
    let answer: Answer = Box::new(move |_, _| (Duration::ZERO, reply.into()));
    let (seen, played) = call(io, Config::new([EDDIE]), speech.clone(), answer, vec![]).await;

    // Never transcribed: one transcription, Eddie's.
    assert_eq!(speech.transcriptions(), 1);
    let said: Vec<_> = utterances(&seen).iter().map(|u| u.speaker).collect();
    assert_eq!(said, [EDDIE]);
    let unlisted: Vec<_> = seen
        .iter()
        .filter(|(_, e)| matches!(e, Event::Unlisted { .. }))
        .collect();
    assert_eq!(unlisted, [&(ms(20), Event::Unlisted { speaker: STRANGER })]);
    assert!(!seen.iter().any(|(_, e)| matches!(e, Event::BargeIn { .. })));
    assert_eq!(played.len(), 1);
    assert!(!played[0].stopped);
    assert_eq!(played[0].length, StandInSpeech::tone_for(reply).duration());
}

#[tokio::test(start_paused = true)]
async fn a_reply_is_synthesized_and_played_a_sentence_at_a_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(5000));
    io.say_wav(EDDIE, ms(0), &fixture(dir.path(), "ask", &[(true, 500)]))
        .unwrap();
    // Each synthesis takes 150 ms, and each sentence plays for 400 ms.
    let speech = Arc::new(StandInSpeech::new().delays(Duration::ZERO, ms(150)));
    let answer: Answer = Box::new(|_, _| (Duration::ZERO, "One. Two. Six.".into()));
    let (seen, played) = call(io, Config::new([EDDIE]), speech.clone(), answer, vec![]).await;

    // The first audio began after one synthesis, not three, and the rest
    // followed back to back: each made while the one before it played.
    let starts: Vec<_> = played.iter().map(|p| (p.started, p.stopped)).collect();
    assert_eq!(
        starts,
        [(ms(1350), false), (ms(1750), false), (ms(2150), false)]
    );
    assert_eq!(speech.syntheses(), 3);
    assert!(seen.iter().any(|(at, e)| *at == ms(1350)
        && *e
            == Event::Speaking {
                what: Spoken::Reply(TurnId(0)),
                sentences: 3,
                first_audio: ms(150)
            }));
    assert!(seen.iter().any(|(at, e)| *at == ms(2550)
        && *e
            == Event::Spoke {
                what: Spoken::Reply(TurnId(0))
            }));
}

#[tokio::test]
async fn building_the_engine_spawns_nothing() {
    let metrics = tokio::runtime::Handle::current().metrics();
    let before = metrics.num_alive_tasks();
    let (engine, _handle) = Engine::new(
        Config::new([EDDIE]),
        Box::new(WavIo::new(ms(100))),
        Arc::new(StandInSpeech::new()),
    );
    assert_eq!(metrics.num_alive_tasks(), before);
    drop(engine);
}

#[tokio::test(start_paused = true)]
async fn leave_stops_speaking_and_ends_the_run() {
    let dir = tempfile::tempdir().unwrap();
    let mut io = WavIo::new(ms(60_000));
    io.say_wav(EDDIE, ms(0), &fixture(dir.path(), "ask", &[(true, 500)]))
        .unwrap();
    let played = io.played();
    let (engine, mut handle) = Engine::new(
        Config::new([EDDIE]),
        Box::new(io),
        Arc::new(StandInSpeech::new()),
    );
    let engine = tokio::spawn(engine.run());
    while let Some(event) = handle.events.recv().await {
        match event {
            Event::Turn { id, .. } => {
                let text = "A reply that is still playing when the call is left.".into();
                handle
                    .commands
                    .send(Command::Reply { turn: id, text })
                    .unwrap();
            }
            Event::Speaking { .. } => {
                sleep(ms(500)).await;
                handle.commands.send(Command::Leave).unwrap();
            }
            _ => {}
        }
    }
    engine.await.unwrap();
    let played = played.get();
    assert_eq!(played.len(), 1);
    assert!(played[0].stopped);
    assert_eq!(played[0].ended, Some(ms(1700)));
}
