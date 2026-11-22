//! own-benchmark's crossing on a view turn (issue 44): a replay to another provider reads every session the
//! view laid, not only its recall's.

use serde_json::json;

use super::tests::board;
use crate::config::MemoryMode;
use crate::tests_recall::{session, turn, Rig};

fn view_rig() -> Rig {
    crate::tests_recall::rig_with_secrets(MemoryMode::Off, board(), |_| {})
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
    let laid = original.laid.as_mut().expect("a view turn's provenance");
    laid.sessions.push(imp.session_id);
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
