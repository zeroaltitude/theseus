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
    assert_eq!(
        journal.matches("the call's first clip is playing").count(),
        1,
        "the first clip is said once in the journal: {journal}"
    );
    assert!(
        journal.contains("a sound at the join was not taken as speech: 6"),
        "{journal}"
    );
    assert!(journal.contains("'ECCR'"), "{journal}");
}

/// The chime can come back 2 s after the join, before the greeting is
/// queued: still within the join's 3 s, an echo canceller's warm-up.
#[tokio::test(start_paused = true)]
async fn a_sound_two_seconds_after_the_join_before_the_greeting_is_queued_is_no_turn() {
    let (seen, played, _) = call(true, 2000, 600, "ECCR", 5000).await;
    assert_eq!(heard_as(&seen), [HeardAs::Join]);
    assert_eq!(turns(&seen), 0);
    assert_eq!(played.len(), 1, "the greeting plays");
}

/// The chime heard back over the greeting doesn't stop it: a sound begun at
/// the join waits for its words before it may cut.
#[tokio::test(start_paused = true)]
async fn a_sound_over_the_greeting_at_the_join_does_not_stop_it() {
    let (seen, played, _) = call(true, 200, 600, "ECCR", 100).await;
    assert_eq!(heard_as(&seen), [HeardAs::Join]);
    assert_eq!(turns(&seen), 0);
    let stopped = seen
        .iter()
        .any(|e| matches!(e, Event::Resumed { .. } | Event::BargeIn { .. }));
    assert!(!stopped, "the greeting was stopped: {seen:?}");
    assert_eq!(played.len(), 1, "the greeting plays once, whole");
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

/// What the journal was told, by name: each event's fields as text, read from
/// the recorded event so a reworded message cannot red the test.
#[derive(Clone, Default)]
struct Told(Arc<Mutex<Vec<std::collections::BTreeMap<&'static str, String>>>>);

struct Fields(std::collections::BTreeMap<&'static str, String>);

impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
        self.0.insert(f.name(), format!("{v:?}"));
    }
}

impl tracing::Subscriber for Told {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, e: &tracing::Event<'_>) {
        let mut fields = Fields(Default::default());
        e.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// A [`WavIo`] on a call whose clips ride a lead-in and whose audio has (or
/// has not) flowed, as a songbird call's do.
struct Flows {
    inner: WavIo,
    flowed: bool,
}

impl theseus_voice::VoiceIo for Flows {
    fn next(&mut self) -> futures_util::future::BoxFuture<'_, Option<theseus_voice::Heard>> {
        self.inner.next()
    }
    fn play(
        &mut self,
        id: theseus_voice::ClipId,
        clip: Audio,
    ) -> futures_util::future::BoxFuture<'_, ()> {
        self.inner.play(id, clip)
    }
    fn stop(&mut self) -> futures_util::future::BoxFuture<'_, ()> {
        self.inner.stop()
    }
    fn lead_in(&self) -> usize {
        30 * theseus_voice::FRAME_SAMPLES
    }
    fn flowed(&self) -> bool {
        self.flowed
    }
}

/// A call that says two reports (at 1 s and 4 s) over an io whose audio has
/// `flowed` or not, and every event the journal was told with a `since_join_ms`.
async fn first_clip_events(flowed: bool) -> Vec<std::collections::BTreeMap<&'static str, String>> {
    let told = Told::default();
    let _guard = tracing::subscriber::set_default(told.clone());
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    let io = Flows {
        inner: WavIo::new(Duration::from_secs(10)),
        flowed,
    };
    let speech = Arc::new(StandInSpeech::new());
    let origin = Instant::now();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = tokio::spawn(engine.run());
    let commands = handle.commands.clone();
    tokio::spawn(async move {
        for (at, text) in [(1000, "Theseus here."), (4000, "And a second thing.")] {
            sleep_until(origin + ms(at)).await;
            let _ = commands.send(Command::Report { text: text.into() });
        }
    });
    let mut events = handle.events;
    while events.recv().await.is_some() {}
    engine.await.unwrap();
    let told = told.0.lock().unwrap();
    told.iter()
        .filter(|e| e.contains_key("since_join_ms"))
        .cloned()
        .collect()
}

/// The call's first clip says, once, the silence before it, how long after
/// the join it played, and whether the call's audio had flowed (theseus-emqbm:
/// the clipped-intro check, and the greeting sent on the flow against the one
/// sent when `GREETING_WAIT` ran out).
#[tokio::test(start_paused = true)]
async fn the_first_clip_says_its_lead_in_the_time_since_the_join_and_whether_the_audio_flowed() {
    for flowed in [false, true] {
        let said = first_clip_events(flowed).await;
        assert_eq!(said.len(), 1, "the first clip says it once: {said:?}");
        assert_eq!(said[0]["lead_in_ms"], "600", "30 frames of 20 ms");
        let since: u64 = said[0]["since_join_ms"].parse().expect("milliseconds");
        assert!(since >= 1_000, "the greeting was queued at 1 s: {since}");
        assert_eq!(said[0]["flowed"], flowed.to_string(), "{said:?}");
    }
}

/// A call over a WAV file has no join to wait for: its audio has flowed from
/// the start, so a test rig's first-clip line never reads as a wait that ran
/// out.
#[test]
fn a_wav_call_has_flowed_from_the_start() {
    use theseus_voice::VoiceIo;
    assert!(WavIo::new(Duration::from_secs(1)).flowed());
}
