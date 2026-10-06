//! What a voice turn's end says aloud, and when the next one goes
//! (theseus-ved2, theseus-b6vz, theseus-nthu): a turn queued behind a failed
//! one, a turn held for the operator, and a stopped turn.

use std::time::Duration;

use serde_json::json;
use theseus_core::provider::Scripted;
use theseus_voice::{Command, TurnId};

use super::super::{Control, PlaceMsg};
use super::tests::heard;
use super::tests_heard::{lounge_scripted, Lounge};
use super::{VoiceTurn, FAILED_TURN, HELD_TURN};

const WAIT: Duration = Duration::from_secs(10);

impl Lounge {
    /// The next `SubmitDone` in the place's mailbox, handled as the place
    /// handles it.
    async fn submit_done(&mut self) {
        let rx = &mut self.rx;
        let done = tokio::time::timeout(WAIT, async {
            loop {
                if let Some(m @ PlaceMsg::SubmitDone(_)) = rx.recv().await {
                    break m;
                }
            }
        })
        .await
        .expect("a submit's answer");
        self.place.handle(done).await;
    }

    /// Every command the engine has got so far.
    fn said(&mut self) -> Vec<Command> {
        std::iter::from_fn(|| self.commands.try_recv().ok()).collect()
    }
}

fn voice_turn(turn: u64, text: &str) -> VoiceTurn {
    VoiceTurn {
        serial: 1,
        turn: TurnId(turn),
        utterances: vec![heard(text)],
    }
}

fn reply(turn: u64, text: &str) -> Command {
    Command::Reply {
        turn: TurnId(turn),
        text: text.into(),
    }
}

/// A turn that fails so the place rebinds to a fresh session still lets the
/// voice turn queued behind it go (theseus-ved2): the call takes turns again
/// without anyone typing.
#[tokio::test]
async fn a_voice_turn_queued_behind_a_failed_turn_goes_after_the_rebind() {
    let d = tempfile::tempdir().unwrap();
    let mut l = lounge_scripted(d.path(), vec![Scripted::text("Here it is.")]);
    let old = l.sid.clone();
    // A typed turn is in flight: the voice turn waits for it.
    l.place.inflight = true;
    l.place.voice_turn(voice_turn(1, "What changed today?"));
    // It fails as a session that can't take turns does: the place rebinds.
    let gone = crate::rpc_client::CallError {
        code: theseus_protocol::error_code::NOT_FOUND,
        message: "no such session".into(),
        data: serde_json::Value::Null,
    };
    l.place.handle(PlaceMsg::SubmitDone(Err(gone))).await;
    assert_ne!(l.place.session_id, old, "rebound");
    // The waiting voice turn went, on the new session, and its reply is said.
    l.submit_done().await;
    assert_eq!(l.said(), [reply(1, "Here it is.")]);
}

/// A voice turn held for the operator says so aloud once, after what it
/// said, through the reply path a failed turn's sentence takes
/// (theseus-b6vz).
#[tokio::test]
async fn a_held_voice_turn_says_once_that_the_answer_is_in_text() {
    let d = tempfile::tempdir().unwrap();
    let write = Scripted::tools(
        "Writing it now.",
        &[("w1", "fs_write", json!({"path": "a.txt", "content": "x\n"}))],
    );
    let mut l = lounge_scripted(d.path(), vec![write]);
    // A private place: its session has the tools that ask.
    l.place
        .shared
        .core
        .bind_places(vec![theseus_core::places::BoundPlace {
            target: l.place.target.clone(),
            name: "#lounge".into(),
            private: true,
            ..Default::default()
        }]);
    l.place.voice_turn(voice_turn(0, "Write the file."));
    l.submit_done().await;
    assert_eq!(
        l.said(),
        [reply(0, &format!("Writing it now.\n\n{HELD_TURN}"))]
    );
}

/// A `/stop` on the voice turn in flight ends it in silence: its failure is
/// never spoken as "Sorry, that didn't work" (theseus-b6vz, theseus-nthu).
/// The stop is known before it is sent, so the submit's answer always
/// finds it.
#[tokio::test]
async fn a_stopped_voice_turn_is_not_spoken_as_failed() {
    let d = tempfile::tempdir().unwrap();
    let refused = theseus_core::provider::ProviderError::Auth {
        status: 401,
        message: "invalid x-api-key".into(),
    };
    let mut l = lounge_scripted(d.path(), vec![Scripted::Fail(refused)]);
    l.place.voice_turn(voice_turn(0, "Read me the whole log."));
    // Its answer may find no turn to stop yet: the stop is known either way.
    l.place.control(Control::Stop, "discord:zeroaltitude").await;
    l.submit_done().await;
    assert_eq!(l.said(), [reply(0, "")], "silent, not {FAILED_TURN:?}");
}

/// A stop from another surface is known by its turn's `turn.failed`, class
/// `stopped`: that turn's end is silent too, and the next voice turn's
/// failure is spoken again (theseus-b6vz).
#[tokio::test]
async fn a_turn_failed_as_stopped_ends_its_voice_turn_in_silence() {
    let d = tempfile::tempdir().unwrap();
    let fail = || {
        Scripted::Fail(theseus_core::provider::ProviderError::Auth {
            status: 401,
            message: "invalid x-api-key".into(),
        })
    };
    let mut l = lounge_scripted(d.path(), vec![fail(), fail()]);
    let sid = l.sid.clone();
    l.place.voice_turn(voice_turn(0, "Read me the whole log."));
    l.place.voice_heard(&theseus_protocol::Event::TurnStarted(
        theseus_protocol::TurnStarted {
            session_id: sid.clone(),
            turn_id: "turn_voice".into(),
            ..Default::default()
        },
    ));
    l.place.voice_heard(&theseus_protocol::Event::TurnFailed(
        theseus_protocol::TurnFailed {
            session_id: sid,
            turn_id: Some("turn_voice".into()),
            class: Some("stopped".into()),
            ..Default::default()
        },
    ));
    l.submit_done().await;
    assert_eq!(l.said(), [reply(0, "")]);
    l.place.voice_turn(voice_turn(1, "Try again."));
    l.submit_done().await;
    assert_eq!(
        l.said(),
        [reply(1, FAILED_TURN)],
        "a new turn is not stopped"
    );
}
