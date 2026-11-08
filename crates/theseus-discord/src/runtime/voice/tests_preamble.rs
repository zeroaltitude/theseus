//! A tool turn's preamble, said when its loop ends (theseus-xi1x): the line a
//! loop writes before its tool calls goes to the engine when that loop
//! ends, while the next call runs, and the turn's reply leaves it out.

use serde_json::json;
use theseus_core::provider::{Scripted, WINDOW_EXCEEDED};
use theseus_protocol::Event as CoreEvent;
use theseus_protocol::{LoopEnded, ModelDelta};
use theseus_voice::{Command, TurnId};

use super::super::Control;
use super::tests::{heard, turn_started};
use super::tests_heard::{lounge_scripted, Lounge};
use super::VoiceTurn;

const LINE: &str = "I'll check the figures.";
const ANSWER: &str = "Up four percent.";

fn voice_turn() -> VoiceTurn {
    VoiceTurn {
        serial: 1,
        turn: TurnId(0),
        utterances: vec![heard("How are the figures?")],
    }
}

/// Loop 0 wrote `LINE` and asked for a call; loop 1 answers.
fn tool_turn(dir: &std::path::Path) -> Lounge {
    let read = Scripted::tools(LINE, &[("r1", "fs_read", json!({"path": "figures.txt"}))]);
    lounge_scripted(dir, vec![read, Scripted::text(ANSWER)])
}

fn answered(text: &str) -> CoreEvent {
    CoreEvent::ModelAnswered(ModelDelta {
        turn_id: "turn_voice".into(),
        loop_index: 0,
        text: text.into(),
    })
}

fn loop_ended(stop: &str, calls: u32, decision: &str) -> CoreEvent {
    CoreEvent::LoopEnded(LoopEnded {
        turn_id: "turn_voice".into(),
        loop_index: 0,
        provider_stop_reason: Some(stop.into()),
        tool_calls: calls,
        advancer: "until_no_tool_calls".into(),
        decision: decision.into(),
    })
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

/// The line is said at its loop's end, not before (it is only written) and
/// not with the reply: the reply is the answer alone.
#[tokio::test]
async fn a_tool_turns_line_is_said_when_its_loop_ends_and_the_reply_leaves_it_out() {
    let d = tempfile::tempdir().unwrap();
    let mut l = tool_turn(d.path());
    l.place.voice_turn(voice_turn());
    // The events a place's watch would carry, by hand: the submit has not run
    // yet on this current-thread runtime.
    l.place
        .voice_heard(&turn_started(&l.sid, "turn_voice", false));
    l.place.voice_heard(&answered(LINE));
    assert_eq!(l.said(), [], "written, not said: its loop has not ended");
    l.place.voice_heard(&loop_ended("tool_use", 1, "continue"));
    assert_eq!(l.said(), [say(LINE)], "said at the loop's end");
    l.submit_done().await;
    assert_eq!(l.said(), [reply(ANSWER)], "the reply is only the answer");
}

/// A retry's text is not in the turn: it is never said, and the reply is
/// whatever the turn's output is.
#[tokio::test]
async fn a_retried_loops_line_is_never_said() {
    for stop in ["refusal", WINDOW_EXCEEDED] {
        let d = tempfile::tempdir().unwrap();
        let mut l = tool_turn(d.path());
        l.place.voice_turn(voice_turn());
        l.place
            .voice_heard(&turn_started(&l.sid, "turn_voice", false));
        l.place.voice_heard(&answered(LINE));
        l.place.voice_heard(&loop_ended(stop, 1, "continue"));
        assert_eq!(l.said(), [], "{stop}");
        l.submit_done().await;
        let said = l.said();
        assert!(matches!(&said[..], [Command::Reply { .. }]), "{said:?}");
    }
}

/// A turn a `/stop` ended is silent: its loop's end says nothing.
#[tokio::test]
async fn a_stopped_turns_line_is_not_said() {
    let d = tempfile::tempdir().unwrap();
    let mut l = tool_turn(d.path());
    l.place.voice_turn(voice_turn());
    l.place
        .voice_heard(&turn_started(&l.sid, "turn_voice", false));
    l.place.voice_heard(&answered(LINE));
    l.place.control(Control::Stop, "discord:zeroaltitude").await;
    l.place.voice_heard(&loop_ended("tool_use", 1, "continue"));
    assert_eq!(l.said(), []);
    // The turn ends in silence: the one command it sends is an empty reply.
    l.submit_done().await;
    assert_eq!(l.said(), [reply("")]);
}

/// A loop's end that comes after the turn's reply went has nothing to go
/// ahead of: it is not said.
#[tokio::test]
async fn a_loop_ending_after_the_reply_says_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut l = tool_turn(d.path());
    l.place.voice_turn(voice_turn());
    l.place
        .voice_heard(&turn_started(&l.sid, "turn_voice", false));
    l.place.voice_heard(&answered(LINE));
    l.submit_done().await;
    assert!(matches!(&l.said()[..], [Command::Reply { .. }]));
    l.place.voice_heard(&loop_ended("tool_use", 1, "continue"));
    assert_eq!(l.said(), []);
}
