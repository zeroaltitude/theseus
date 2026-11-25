//! The Undo pressed on a settings change's note puts the change back by the
//! undo's own write path: the file byte for byte as it was, the rows a card's
//! yes writes, no second note, and a second press changes nothing.

use serde_json::json;

use crate::config_change::tests::{change, rig_told, Rig, FILE};

/// The restart a write asked for is done and healthy: the next may be written.
fn back(r: &Rig) {
    let marker = crate::config_change::restart::marker_path(&r.state);
    let mut p = crate::config_change::restart::load(&marker).expect("a restart was asked");
    p.stage = crate::config_change::restart::Stage::Confirmed;
    crate::config_change::restart::save(&marker, &p).unwrap();
    crate::config_change::restart::finish(&marker, &p);
}

#[tokio::test]
async fn a_change_then_its_undo_pressed_leaves_the_settings_byte_for_byte() {
    let r = rig_told();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    assert_ne!(r.file_text(), FILE, "the change was written");
    let n = r.rows("config.changed")[0]["previous_copy"]
        .as_u64()
        .unwrap();
    back(&r);
    let notes = crate::tests_day_ceiling::posts(&r.core, "notice").len();
    let said = r
        .core
        .undo_change_pressed(n as u32, "owner", "discord", &sid)
        .expect("put back");
    assert!(
        said.starts_with(&format!("Put back settings change #{n}")),
        "{said}"
    );
    assert_eq!(r.file_text(), FILE, "byte for byte as it was");
    let changed = r.rows("config.changed");
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert_eq!(changed[1]["approved_by"], json!("owner"));
    assert_eq!(changed[1]["via"], json!("discord"));
    assert_eq!(
        changed[1]["told"],
        json!(null),
        "a press, not a tool's setting"
    );
    let undone = r.rows("config.undone");
    assert_eq!(undone.len(), 1, "{undone:?}");
    assert_eq!(undone[0]["undone"], json!(n));
    assert_eq!(
        crate::tests_day_ceiling::posts(&r.core, "notice").len(),
        notes,
        "no second note: the pressed note says it"
    );
    // A second press finds the change already undone and writes nothing.
    back(&r);
    let again = r
        .core
        .undo_change_pressed(n as u32, "owner", "discord", &sid)
        .unwrap_err();
    assert!(again.contains("already undone"), "{again}");
    assert_eq!(r.file_text(), FILE);
    assert_eq!(r.rows("config.changed").len(), 2);
}

#[tokio::test]
async fn a_press_for_a_change_not_in_the_history_writes_nothing() {
    let r = rig_told();
    let sid = r.session();
    let why = r
        .core
        .undo_change_pressed(7, "owner", "discord", &sid)
        .unwrap_err();
    assert!(why.contains("no change #7"), "{why}");
    assert_eq!(r.file_text(), FILE);
    assert!(r.rows("config.changed").is_empty());
}
