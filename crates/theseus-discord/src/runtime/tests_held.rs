//! theseus-6809, through the place's actor: a turn's start tells the lane the
//! turns the renderer holds, so the ninth turn's start drops the first from
//! it, and `/new`'s rebind drops every held turn.

use theseus_core::secrets::SecretBoard;
use theseus_core::session::SessionRecord;
use theseus_protocol::SessionKind;
use tokio::sync::mpsc;

use super::tests::{core_with, place_for_tests};
use super::{Control, CoreEvent, LaneMsg, PlaceMsg};

/// The `Held` lists the lane got, in order; anything else it got is skipped.
fn helds(lane: &mut mpsc::UnboundedReceiver<LaneMsg>) -> Vec<Vec<String>> {
    let mut out = vec![];
    while let Ok(m) = lane.try_recv() {
        if let LaneMsg::Held(turns) = m {
            out.push(turns);
        }
    }
    out
}

#[tokio::test]
async fn a_turns_start_and_a_rebind_tell_the_lane_the_held_turns() {
    let d = tempfile::tempdir().unwrap();
    let core = core_with(d.path(), SecretBoard::empty(), |_| {});
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    let (mut place, _mailbox) = place_for_tests(&core, &sid);
    let (lane_tx, mut lane) = mpsc::unbounded_channel();
    place.lane = lane_tx;
    let turn = |n: usize| format!("turn_quay_{n}");
    for n in 0..9 {
        let started = theseus_protocol::TurnStarted {
            session_id: sid.clone(),
            turn_id: turn(n),
            ..Default::default()
        };
        place
            .handle(PlaceMsg::Event(Box::new(CoreEvent::TurnStarted(started))))
            .await;
        let got = helds(&mut lane);
        let want: Vec<String> = (n.saturating_sub(7)..=n).map(turn).collect();
        assert_eq!(got, [want], "turn {n}'s start");
    }
    // The ninth turn's start held turns 1 to 8: the first was dropped.
    // `/new` drops them all.
    // `/new` drops them all once the next message opens the fresh session
    // (theseus-emqx).
    let answer = place.control(Control::New, "discord:zeroaltitude").await;
    assert!(answer.starts_with("🆕 A fresh session starts"), "{answer}");
    assert!(place.ready().await);
    assert_ne!(place.session_id, sid);
    assert_eq!(helds(&mut lane), [Vec::<String>::new()], "the rebind");
}

/// A place whose daemon restarted reads the turn a late event names back from
/// its session (issue 8): the renderer holds it, the lane is told it is held,
/// and the event is shown under the keys the first run's messages had. Before,
/// the event named a turn no renderer held, and was dropped.
#[tokio::test]
async fn a_late_event_for_a_turn_before_a_restart_reads_the_turn_back_first() {
    use theseus_core::node::{Body, Node};
    let d = tempfile::tempdir().unwrap();
    let core = core_with(d.path(), SecretBoard::empty(), |_| {});
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    let sid = rec.session_id.clone();
    core.store.put_session(&sid, &rec).unwrap();
    let turn = "turn_quay_late";
    let gate = serde_json::from_value(serde_json::json!({"result": {"gate": "allow"}})).unwrap();
    let call = Node::tool_call(
        &sid,
        Some(turn),
        Some(0),
        Body::ToolCall {
            tool_use_id: "u1".into(),
            tool: "fs.read".into(),
            wire_name: "fs_read".into(),
            input: serde_json::json!({"path": "src/lib.rs"}),
            assistant_node: "msg_1".into(),
            correlation_id: None,
            gate: Some(Box::new(gate)),
        },
    );
    core.store.append(&[call.record().unwrap()]).unwrap();
    let (mut place, _mailbox) = place_for_tests(&core, &sid);
    let (lane_tx, mut lane) = mpsc::unbounded_channel();
    place.lane = lane_tx;
    assert!(
        !place.renderer.knows(turn),
        "a renderer that never saw it start"
    );
    let ended = CoreEvent::from_notification(
        "tool.ended",
        &serde_json::json!({"session_id": sid, "turn_id": turn, "tool_use_id": "u1",
                            "status": "ok", "duration_ms": 12}),
    )
    .unwrap()
    .unwrap();
    place.handle(PlaceMsg::Event(Box::new(ended))).await;
    assert!(place.renderer.knows(turn), "read back from the session");
    assert_eq!(
        helds(&mut lane),
        [vec![turn.to_string()]],
        "the lane keeps it whole"
    );
}
