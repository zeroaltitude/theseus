//! A tool turn's preamble, said when its loop ends rather than with the whole
//! turn's reply (theseus-xi1x): through the seam, in virtual time.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    write_wav, Audio, Command, Config, CutWhy, Engine, Event, Played, Speaker, Spoken,
    StandInSpeech, TurnId, WavIo,
};
use tokio::time::{sleep_until, Instant};

const OWNER: Speaker = Speaker(101);

const PREAMBLE: &str = "I'll pull the daily figures.";
const REPLY: &str = "The figures are up four percent.";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// How long the stand-in's audio for `text` plays.
fn len(text: &str) -> Duration {
    StandInSpeech::tone_for(text).duration()
}

/// The owner asks from 0 to 0.5 s: the turn begins at 1.2 s.
fn asked(dir: &Path, length: u64) -> WavIo {
    let path = dir.join("asked.wav");
    write_wav(&path, &Audio::tone(220.0, ms(500), 0.3)).expect("write the fixture");
    let mut io = WavIo::new(ms(length));
    io.say_wav(OWNER, ms(0), &path).expect("the fixture");
    io
}

/// The call: turn 0 gets each of `commands` at its time from the call's
/// start. Returns every event with its time, and what was played.
async fn call(
    io: WavIo,
    config: Config,
    commands: Vec<(Duration, Command)>,
) -> (Vec<(Duration, Event)>, Vec<Played>) {
    let origin = Instant::now();
    let played = io.played();
    let (engine, handle) = Engine::new(config, Box::new(io), Arc::new(StandInSpeech::new()));
    let engine = tokio::spawn(engine.run());
    for (at, command) in commands {
        let sender = handle.commands.clone();
        tokio::spawn(async move {
            sleep_until(origin + at).await;
            let _ = sender.send(command);
        });
    }
    let mut events = handle.events;
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        seen.push((origin.elapsed(), event));
    }
    engine.await.expect("the engine ran to the call's end");
    (seen, played.get())
}

fn config(ack_after_secs: u64) -> Config {
    let mut c = Config::new([OWNER]);
    c.acknowledge_after = Duration::from_secs(ack_after_secs);
    c
}

fn say(text: &str) -> Command {
    Command::Say {
        turn: TurnId(0),
        text: text.into(),
    }
}

fn reply(text: &str) -> Command {
    Command::Reply {
        turn: TurnId(0),
        text: text.into(),
    }
}

fn starts(played: &[Played]) -> Vec<(Duration, Duration)> {
    played.iter().map(|p| (p.started, p.length)).collect()
}

/// The preamble is said when it is sent, while the turn is still in flight,
/// and the reply after it, whole: the turn's end is the reply's, not the
/// preamble's.
#[tokio::test(start_paused = true)]
async fn a_preamble_is_said_when_it_comes_and_the_reply_after_it() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 30_000);
    let commands = vec![(ms(4000), say(PREAMBLE)), (ms(15_000), reply(REPLY))];
    let (seen, played) = call(io, config(60), commands).await;
    assert_eq!(
        starts(&played),
        [(ms(4000), len(PREAMBLE)), (ms(15_000), len(REPLY))],
        "the preamble at once, nine seconds before the reply that ends the turn"
    );
    let spoke: Vec<_> = seen
        .iter()
        .filter_map(|(at, e)| match e {
            Event::Spoke { what } => Some((*at, *what)),
            _ => None,
        })
        .collect();
    assert_eq!(
        spoke.iter().map(|(_, w)| *w).collect::<Vec<_>>(),
        [Spoken::Preamble(TurnId(0)), Spoken::Reply(TurnId(0))]
    );
}

/// A turn the preamble comes for with no reply yet is still in flight: its
/// acknowledgment does not follow the preamble, since the preamble is it.
#[tokio::test(start_paused = true)]
async fn a_preamble_stands_for_the_acknowledgment() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 30_000);
    // The acknowledgment is due 2 s into the turn, at 3.2 s; the preamble
    // comes at 2.5 s and is said, so no chime follows it.
    let commands = vec![(ms(2500), say(PREAMBLE)), (ms(20_000), reply(REPLY))];
    let (seen, played) = call(io, config(2), commands).await;
    assert!(
        !seen
            .iter()
            .any(|(_, e)| matches!(e, Event::Acknowledged { .. })),
        "no acknowledgment: {seen:?}"
    );
    assert_eq!(
        starts(&played),
        [(ms(2500), len(PREAMBLE)), (ms(20_000), len(REPLY))]
    );
}

/// A preamble that comes after its turn's reply has nothing to go ahead of:
/// the reply said what it left out, and it is dropped.
#[tokio::test(start_paused = true)]
async fn a_preamble_after_the_reply_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 30_000);
    let commands = vec![(ms(4000), reply(REPLY)), (ms(9000), say(PREAMBLE))];
    let (_, played) = call(io, config(60), commands).await;
    assert_eq!(starts(&played), [(ms(4000), len(REPLY))]);
}

/// A preamble with nothing to say (no words once made speakable) is none.
#[tokio::test(start_paused = true)]
async fn a_preamble_with_no_words_is_not_said() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 30_000);
    let commands = vec![(ms(4000), say("  \n ")), (ms(9000), reply(REPLY))];
    let (_, played) = call(io, config(60), commands).await;
    assert_eq!(starts(&played), [(ms(9000), len(REPLY))]);
}

/// Leaving with the preamble still unsaid cuts it, like any reply: the call's
/// end is a `Cut` for it, as a reply's is.
#[tokio::test(start_paused = true)]
async fn leaving_with_a_preamble_unsaid_cuts_it() {
    let dir = tempfile::tempdir().unwrap();
    let io = asked(dir.path(), 60_000);
    let commands = vec![
        // Two sentences, so the second is still queued at the leave.
        (ms(4000), say(&format!("{PREAMBLE} {PREAMBLE}"))),
        (ms(4000) + len(PREAMBLE) / 2, Command::Leave),
    ];
    let (seen, _) = call(io, config(60), commands).await;
    let cut: Vec<_> = seen
        .iter()
        .filter_map(|(_, e)| match e {
            Event::Cut { what, why, .. } => Some((*what, *why)),
            _ => None,
        })
        .collect();
    assert_eq!(cut, [(Spoken::Preamble(TurnId(0)), CutWhy::CallEnded)]);
}
