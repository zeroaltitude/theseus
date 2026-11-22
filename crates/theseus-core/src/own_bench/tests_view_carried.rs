//! own-benchmark on a view turn whose window holds words a tool brought in from elsewhere (issue 44): a
//! history read's result is refused for a replay in words, before anything is rebuilt or sent, since its
//! sources cannot be checked as they stand now; a window of ordinary tool results replays. And a replay to
//! another provider reads every session the view laid, not only its recall's.

use serde_json::json;
use theseus_protocol::own_bench::TurnReplayParams;

use super::rebuild::view_carried;
use super::tests::board;
use crate::config::assembly::{Assembly, AssemblySetting};
use crate::config::MemoryMode;
use crate::provider::Scripted;
use crate::tests_recall::{session, turn, Rig};

fn view_rig() -> Rig {
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board(), |c| {
        c.context.assembly = Some(AssemblySetting::All(Assembly::View));
    })
}

/// A conversation whose first turn called `tool` once, and its second turn, a view turn whose window holds
/// the first: the rig, the session and the second turn.
async fn after_a_call(tool: &str, input: serde_json::Value) -> (Rig, String, String) {
    let rig = view_rig();
    {
        let mut s = rig.model.script.lock().unwrap();
        let wire = theseus_tools::wire_name(tool);
        s.push_back(Scripted::tools("", &[("call_1", wire.as_str(), input)]));
        s.push_back(Scripted::text("Read it."));
        s.push_back(Scripted::text("And so it stands."));
    }
    let sid = session(&rig.core, None, &[]);
    turn(&rig.core, &sid, "What did we say about the kestrel relay?").await;
    let t = turn(&rig.core, &sid, "And then?").await.turn_id;
    let read = (rig.core.store.transcript(&sid).unwrap().into_iter())
        .any(|(_, n)| matches!(&n.body, crate::node::Body::ToolResult { .. }));
    assert!(read, "the first turn holds {tool}'s result");
    (rig, sid, t)
}

/// Every request the model was sent, once none has come for a while (a view place's ladder writer calls
/// after a turn, in the background).
async fn settled(rig: &Rig) -> usize {
    let mut seen = rig.model.requests().len();
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let now = rig.model.requests().len();
        if now == seen {
            return now;
        }
        seen = now;
    }
    seen
}

fn replay(sid: &str, t: &str) -> TurnReplayParams {
    TurnReplayParams {
        session: sid.into(),
        turn: t.into(),
        profile: "haiku".into(),
        effort: None,
        scope: None,
        compare: Some(false),
    }
}

/// A view turn whose window holds a page of the history tool is refused in words (part `view`, naming the
/// history tool), and `turn.replay` of it calls no model of any kind.
#[tokio::test]
async fn a_view_turn_whose_window_holds_a_history_page_is_not_replayed() {
    let tool = crate::view::history::HISTORY_TOOL;
    let (rig, sid, t) = after_a_call(tool, json!({})).await;
    let Err(e) = rig.core.rebuild_first(&sid, &t) else {
        panic!("a view turn carrying a history read was rebuilt");
    };
    assert_eq!((e.part, e.why.clone()), ("view", view_carried(tool)));
    assert!(e.why.contains("read by the history tool"), "{e}");
    let sent = settled(&rig).await;
    assert!(rig.core.turn_replay(replay(&sid, &t), "cli").await.is_err());
    assert_eq!(rig.model.requests().len(), sent, "no model was called");
}

/// The same for a history open of a handle (a released piece is read this way): refused in words.
#[tokio::test]
async fn a_view_turn_whose_window_holds_a_history_open_is_not_replayed() {
    let tool = crate::view::history::HISTORY_TOOL;
    let (rig, sid, t) = after_a_call(tool, json!({"open": "rel_0123456789abcdef"})).await;
    let Err(e) = rig.core.rebuild_first(&sid, &t) else {
        panic!("a view turn carrying a history open was rebuilt");
    };
    assert_eq!((e.part, e.why), ("view", view_carried(tool)));
    let sent = settled(&rig).await;
    assert!(rig.core.turn_replay(replay(&sid, &t), "cli").await.is_err());
    assert_eq!(rig.model.requests().len(), sent, "no model was called");
}

/// A view turn whose window holds only an ordinary tool's result (here one the turn could not run) is
/// rebuilt and replays.
#[tokio::test]
async fn a_view_turn_whose_window_holds_ordinary_results_replays() {
    let (rig, sid, t) = after_a_call("nothing.here", json!({})).await;
    rig.core.rebuild_first(&sid, &t).expect("rebuildable");
    let r = rig.core.turn_replay(replay(&sid, &t), "cli").await.unwrap();
    assert!(!r.replay.text.is_empty(), "the replay answered");
}

/// A replay to another provider reads every session a view laid (its window's, its ladder's, its cards'),
/// not only its recall's: one imported as `personal` among them refuses the crossing, with the rule's own
/// words; the same view without it crosses.
#[tokio::test]
async fn a_view_turns_crossing_reads_every_session_it_laid() {
    let rig = view_rig();
    super::tests::script(&rig, &["The kestrel relay listens on port 7714."]);
    let sid = session(&rig.core, None, &[]);
    let t = turn(&rig.core, &sid, "Which port?").await.turn_id;
    let mut original = rig.core.rebuild_first(&sid, &t).expect("rebuildable");
    assert_eq!(rig.core.crossing(&original, "zai"), None, "nothing veiled");
    let mut imp =
        crate::session::SessionRecord::new(theseus_protocol::SessionKind::Conversation, None);
    imp.imported = Some(Box::new(
        serde_json::from_value(json!({
            "tag": "tern-2026", "episode_id": "ep1", "hash": "h", "source": "wiki",
            "place": {"kind": "cli"}, "labels": {"sensitivity": "personal"},
            "as_of": {"start_ms": 1, "end_ms": 2}, "messages": 1, "summary": false,
            "file": "f", "line": 1, "imported_at_ms": 3
        }))
        .unwrap(),
    ));
    rig.core.store.put_session(&imp.session_id, &imp).unwrap();
    let view = original.view.as_mut().expect("a view turn");
    view.sessions.push(imp.session_id.clone());
    assert_eq!(
        rig.core.crossing(&original, "zai").as_deref(),
        Some("its recall carries a personal source")
    );
    assert_eq!(
        rig.core.crossing(&original, "anthropic"),
        None,
        "the same provider"
    );
}
