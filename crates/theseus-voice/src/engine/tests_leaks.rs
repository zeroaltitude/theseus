//! `first_words` holds a turn's end only until its reply's first audio plays;
//! a turn that never gets that audio must not leave its entry behind
//! (theseus-v774k).

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::{Audio, Engine, ListenLog, Listened, StandInListen, StandInSpeech, WavIo};

const OWNER: Speaker = Speaker(101);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn end(text: &str) -> Listened {
    Listened::End {
        speaker: OWNER,
        text: text.into(),
        confidence: Some(0.9),
        streamed: ms(500),
    }
}

/// A call run to its end, the session answering each turn with `reply`.
async fn leftover(listen: StandInListen, eager: bool, reply: &'static str) -> usize {
    let mut io = WavIo::new(ms(8_000));
    io.say(OWNER, ms(200), Audio::tone(220.0, ms(1_000), 0.3));
    let mut config = Config::new([OWNER]);
    config.acknowledge_after = Duration::from_secs(60);
    let (engine, mut handle) = Engine::new(config, Box::new(io), Arc::new(StandInSpeech::new()));
    let mut engine = engine.with_listen(Box::new(listen), "flux-general-en", eager);
    let commands = handle.commands.clone();
    let answers = async {
        while let Some(event) = handle.events.recv().await {
            if let Event::Turn { id, .. } = event {
                let _ = commands.send(Command::Reply {
                    turn: id,
                    text: reply.into(),
                });
            }
        }
    };
    tokio::select! {
        () = engine.serve() => {}
        () = answers => {}
    }
    engine.first_words.len()
}

#[tokio::test(start_paused = true)]
async fn a_turn_whose_reply_makes_no_audio_leaves_no_entry() {
    let (listen, _log): (StandInListen, ListenLog) = StandInListen::new();
    let listen = listen
        .at(10, Listened::Start { speaker: OWNER })
        .at(70, end("Thanks."));
    assert_eq!(leftover(listen, false, "").await, 0);
}

/// A turn whose reply is empty never gets audio, so its `first_words` entry
/// is removed with the empty reply (`speakable`), not left behind.
#[tokio::test(start_paused = true)]
async fn an_eager_turn_superseded_by_the_whole_sentence_leaves_no_entry() {
    let (listen, _log): (StandInListen, ListenLog) = StandInListen::new();
    let listen = listen
        .at(10, Listened::Start { speaker: OWNER })
        .at(
            40,
            Listened::Eager {
                speaker: OWNER,
                text: "Book a table".into(),
                confidence: Some(0.9),
            },
        )
        .at(50, Listened::Resumed { speaker: OWNER })
        .at(70, end("Book a table for two."));
    assert_eq!(leftover(listen, true, "").await, 0);
}
