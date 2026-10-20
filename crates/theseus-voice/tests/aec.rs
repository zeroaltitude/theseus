//! Echo cancellation through the seam (theseus-yaxcp), in virtual time: on
//! speakers the listener's stream carries Theseus's own voice back, 180 ms
//! late, 12 dB down and low-passed. A barge-in is judged on what is left once
//! that is taken out, so the echo alone never stops the reply and a voice over
//! it does.

use std::sync::Arc;
use std::time::Duration;

use theseus_voice::{
    Audio, Command, Config, Engine, Event, Played, Speaker, Speech, SpeechFuture, StandInSpeech,
    Synthesis, Transcript, TurnId, Usage, WavIo,
};
use tokio::time::{sleep, Instant};

const OWNER: Speaker = Speaker(101);

const S1: &str = "This first sentence runs on for quite a while, long enough to talk over.";
const S2: &str = "The second sentence is long enough to be cut late, if it comes to that.";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The stand-in, with a voice in place of its tones: noise in syllables, as
/// long as a tone would be, so an echo of it has an envelope to be found by.
struct Voiced(StandInSpeech);

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
            let seed = text.len() as u64;
            Ok(Synthesis {
                audio: Audio::babble(seed, length, 0.1),
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

/// What a call gave: the events with their times, and what was played.
struct Call {
    events: Vec<(Duration, Event)>,
    played: Vec<Played>,
}

impl Call {
    fn cut(&self) -> bool {
        self.events
            .iter()
            .any(|(_, e)| matches!(e, Event::Cut { .. } | Event::BargeIn { .. }))
    }

    /// When the first clip that was stopped stopped.
    fn stopped(&self) -> Option<Duration> {
        self.played.iter().find(|p| p.stopped).and_then(|p| p.ended)
    }
}

/// The owner says something at 0.2 s; the reply (two long sentences, about
/// 8 s) begins at about 1.5 s, and the owner's speakers send it back. From
/// `talks` the owner talks over it too, if they do.
async fn call(echo_cancel: bool, talks: Option<Duration>, reply: bool) -> Call {
    let mut io = WavIo::new(ms(14_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(600), 0.3));
    io.echo(OWNER, ms(180), 12.0, 0.6);
    if let Some(at) = talks {
        io.say(OWNER, at, Audio::tone(310.0, ms(2_000), 0.15));
    }
    let played = io.played();
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    config.echo_cancel = echo_cancel;
    let speech = Arc::new(Voiced(StandInSpeech::new()));
    let origin = Instant::now();
    let (engine, handle) = Engine::new(config, Box::new(io), speech);
    let engine = tokio::spawn(engine.run());
    let mut events = handle.events;
    let mut seen = Vec::new();
    while let Some(event) = events.recv().await {
        if let (true, Event::Turn { id: TurnId(0), .. }) = (reply, &event) {
            let commands = handle.commands.clone();
            tokio::spawn(async move {
                sleep(Duration::ZERO).await;
                let _ = commands.send(Command::Reply {
                    turn: TurnId(0),
                    text: format!("{S1} {S2}"),
                });
            });
        }
        seen.push((origin.elapsed(), event));
    }
    engine.await.expect("the engine ran to the call's end");
    Call {
        events: seen,
        played: played.get(),
    }
}

#[tokio::test(start_paused = true)]
async fn the_echo_alone_never_stops_the_reply() {
    let call = call(true, None, true).await;
    assert!(!call.cut(), "{:#?}", call.events);
    assert!(call.stopped().is_none(), "{:#?}", call.played);
    assert_eq!(call.played.len(), 2, "both sentences played to their end");
}

#[tokio::test(start_paused = true)]
async fn a_voice_over_the_echo_stops_it_within_300_ms_and_a_frame() {
    let start = ms(6_000);
    let call = call(true, Some(start), true).await;
    let stopped = call.stopped().expect("the reply was stopped");
    assert!(
        stopped >= start + ms(280) && stopped <= start + ms(320),
        "the voice began at {start:?} and the reply stopped at {stopped:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn with_it_off_the_echo_alone_stops_the_reply_as_it_did() {
    let call = call(false, None, true).await;
    let stopped = call.stopped().expect("the echo stopped the reply");
    assert!(stopped < ms(3_000), "{stopped:?}");
}

/// With it off, the call is what it was before: the events and the clips at
/// their times, recorded from the engine without echo cancellation in it.
#[tokio::test(start_paused = true)]
async fn with_it_off_the_call_is_as_it_was() {
    let call = call(false, None, true).await;
    let mut lines: Vec<String> = call
        .events
        .iter()
        .map(|(at, e)| format!("{at:?} {e:?}"))
        .collect();
    lines.extend(call.played.iter().map(|p| format!("{p:?}")));
    let golden = include_str!("aec_off.golden");
    assert_eq!(lines.join("\n") + "\n", golden);
}

/// Theseus says nothing: no echo is cancelled, and the call is what it is with
/// the canceller off, event for event.
#[tokio::test(start_paused = true)]
async fn while_theseus_is_silent_the_listener_passes_through_untouched() {
    let on = call(true, Some(ms(2_000)), false).await;
    let off = call(false, Some(ms(2_000)), false).await;
    assert!(on.played.is_empty());
    assert_eq!(format!("{:?}", on.events), format!("{:?}", off.events));
}
