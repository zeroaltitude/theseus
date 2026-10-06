//! When the next voice turn goes (theseus-ved2): a turn queued behind a
//! failed one.

use std::time::Duration;

use theseus_core::provider::Scripted;
use theseus_voice::{Command, TurnId};

use super::super::PlaceMsg;
use super::tests::heard;
use super::tests_heard::{lounge_scripted, Lounge};
use super::VoiceTurn;

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
