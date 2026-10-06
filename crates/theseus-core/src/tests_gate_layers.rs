//! The gate's layers held through a turn (theseus-x1jj and its kin). Each
//! layer has its own function and its own test; these run a turn (or the
//! gate's `order`), so a line dropped from `toolrun::order` fails one.

use theseus_lsp::fake::Config as FakeConfig;

use crate::policy::Posture;
use crate::tests_lsp_edits::{edit, rig, session, turn};
use crate::tests_places::rows;

/// The cards a session's turn left waiting, by reason.
fn asked(core: &crate::Core, sid: &str) -> Vec<String> {
    rows(core, "tool.confirm_requested")
        .into_iter()
        .filter(|r| r["session_id"] == sid)
        .map(|r| r["reason"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// L3 (theseus-x1jj): with `start_on_edit` on and no server up, `proc.run`
/// on approve and `fs.edit` open, a private place's `fs.edit` waits for the
/// operator, its reason naming the start; a shared place's edit starts
/// nothing and keeps its own posture. The line in `order` is what it holds:
/// the layer's own test calls `lsp::edits::gate` directly.
#[tokio::test]
async fn an_edit_that_starts_its_server_waits_in_a_turn_as_proc_run_would() {
    let tweak = |cfg: &mut crate::Config, work: &std::path::Path| {
        cfg.policy.tools.insert("fs.edit".into(), Posture::Open);
        cfg.policy.tools.insert("proc.run".into(), Posture::Approve);
        cfg.places.public_paths = vec![work.to_string_lossy().into_owned()];
        cfg.lsp.servers.get_mut("fake").unwrap().start_on_edit = Some(true);
    };
    let r = rig(
        edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n"),
        FakeConfig::default(),
        tweak,
    );
    let sid = session(&r.core, None);
    turn(&r.core, &sid, "edit").await;
    let waiting = asked(&r.core, &sid);
    assert_eq!(waiting.len(), 1, "the edit waits: {waiting:?}");
    assert!(waiting[0].contains("starts fake on"), "{}", waiting[0]);
    assert_eq!(r.spawner.count(), 0, "nothing started before the answer");

    // A shared place: no start is judged, and the edit keeps its posture.
    let r = rig(
        edit("e1", "a.fake", "let y = 2\n", "ERROR 2\n"),
        FakeConfig::default(),
        tweak,
    );
    let shared = session(&r.core, Some("channel:31415926535"));
    turn(&r.core, &shared, "edit").await;
    assert!(asked(&r.core, &shared).is_empty());
    let (text, _) = crate::tests_lsp_edits::result_of(&r.core, &shared, "e1");
    assert!(text.starts_with("Replaced 1 occurrence"), "it ran: {text}");
}
