//! The join: a sound before the greeting is no turn, a long clear utterance
//! is, and the greeting's head heard back is an echo (theseus-yf9ct): through
//! the seam, from WAV fixtures, in virtual time.

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use theseus_voice::{
    write_wav, Audio, Command, Config, Engine, Event, HeardAs, Played, Speaker, StandInSpeech,
    WavIo,
};
use tokio::time::{sleep_until, Instant};

const OWNER: Speaker = Speaker(101);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// What the engine said in the journal, as tracing's fmt layer wrote it.
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<u8>>>);

impl Write for Journal {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A 10 s call: the owner says `length` ms of speech at `at`, heard as
/// `transcript`, and the greeting is queued at `greeting`. Returns what
/// came, what played, and the journal.
async fn call(
    join_guard: bool,
    at: u64,
    length: u64,
    transcript: &str,
    greeting: u64,
) -> (Vec<Event>, Vec<Played>, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("line.wav");
    write_wav(&path, &Audio::tone(220.0, ms(length), 0.3)).unwrap();
    let mut io = WavIo::new(Duration::from_secs(10));
    io.say_wav(OWNER, ms(at), &path).unwrap();
    let played = io.played();
    let journal = Journal::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer({
            let journal = journal.clone();
            move || journal.clone()
        })
        .with_ansi(false)
        .finish();
    let _guard = tracing::subscriber::set_default(subscriber);
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    config.join_guard = join_guard;
    let speech = Arc::new(StandInSpeech::new().transcript(OWNER, transcript));
    let origin = Instant::now();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = tokio::spawn(engine.run());
    let commands = handle.commands.clone();
    tokio::spawn(async move {
        sleep_until(origin + ms(greeting)).await;
        let _ = commands.send(Command::Report {
            text: "Theseus here.".into(),
        });
    });
    let mut events = handle.events;
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        seen.push(event);
    }
    engine.await.unwrap();
    let journal = String::from_utf8(journal.0.lock().unwrap().clone()).unwrap();
    (seen, played.get(), journal)
}

fn turns(seen: &[Event]) -> usize {
    seen.iter()
        .filter(|e| matches!(e, Event::Turn { .. }))
        .count()
}

fn heard_as(seen: &[Event]) -> Vec<HeardAs> {
    seen.iter()
        .filter_map(|e| match e {
            Event::Utterance(u) => Some(u.heard_as),
            _ => None,
        })
        .collect()
}

#[tokio::test(start_paused = true)]
async fn a_short_sound_before_the_greeting_is_no_turn_and_the_greeting_plays() {
    let (seen, played, journal) = call(true, 100, 600, "ECCR", 1600).await;
    assert_eq!(turns(&seen), 0);
    assert_eq!(heard_as(&seen), [HeardAs::Join]);
    assert_eq!(played.len(), 1, "the greeting plays");
    assert!(
        journal.contains("a sound at the join was not taken as speech: 6"),
        "{journal}"
    );
    assert!(journal.contains("'ECCR'"), "{journal}");
}

#[tokio::test(start_paused = true)]
async fn a_long_clear_utterance_at_the_join_is_a_turn() {
    let (seen, _, journal) = call(true, 100, 2000, "what's the weather like today", 3000).await;
    assert_eq!(turns(&seen), 1);
    assert_eq!(heard_as(&seen), [HeardAs::Words]);
    assert!(!journal.contains("at the join"), "{journal}");
}

#[tokio::test(start_paused = true)]
async fn with_the_guard_off_a_sound_at_the_join_is_a_turn_as_before() {
    let (seen, _, journal) = call(false, 100, 600, "ECCR", 1600).await;
    assert_eq!(turns(&seen), 1);
    assert_eq!(heard_as(&seen), [HeardAs::Words]);
    assert!(!journal.contains("at the join"), "{journal}");
}

#[tokio::test(start_paused = true)]
async fn the_greetings_head_heard_back_in_the_tail_is_an_echo_and_a_yes_is_not() {
    // The greeting plays 5.0 to 5.7 s; the owner's sound comes at 6 s.
    let (seen, played, _) = call(true, 6000, 600, "seus here", 5000).await;
    assert_eq!(heard_as(&seen), [HeardAs::Echo]);
    assert_eq!(turns(&seen), 0);
    assert_eq!(played.len(), 1);
    let (seen, _, _) = call(true, 6000, 600, "yes", 5000).await;
    assert_eq!(heard_as(&seen), [HeardAs::Words]);
    assert_eq!(turns(&seen), 1);
    let (seen, _, _) = call(false, 6000, 600, "seus here", 5000).await;
    assert_eq!(heard_as(&seen), [HeardAs::Words]);
}
