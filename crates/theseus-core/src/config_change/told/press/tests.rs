//! The Undo pressed on a settings change's note puts the change back by the
//! undo's own write path: the file byte for byte as it was, the rows a card's
//! yes writes, no second note, and a second press changes nothing. Through
//! `told.undo`, only the owner's press from a private place counts.

use serde_json::json;
use theseus_protocol::DiscordOrigin;

use crate::approval::{Answerer, Surface};
use crate::config_change::tests::{change, rig_told, Rig, FILE, OWNER};

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
    let notes = || {
        crate::tests_day_ceiling::posts(&r.core, crate::owner_note::told::KIND).len()
            + crate::tests_day_ceiling::posts(&r.core, "notice").len()
    };
    let before = notes();
    assert_eq!(before, 1, "the change's own note");
    let said = r
        .core
        .undo_change_pressed(n as u32, "owner", "discord")
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
    assert_eq!(notes(), before, "no second note: the pressed note says it");
    // A second press finds the change already undone and writes nothing.
    back(&r);
    let again = r
        .core
        .undo_change_pressed(n as u32, "owner", "discord")
        .unwrap_err();
    assert!(again.contains("already undone"), "{again}");
    assert_eq!(r.file_text(), FILE);
    assert_eq!(r.rows("config.changed").len(), 2);
}

#[tokio::test]
async fn a_press_for_a_change_not_in_the_history_writes_nothing() {
    let r = rig_told();
    let why = r
        .core
        .undo_change_pressed(7, "owner", "discord")
        .unwrap_err();
    assert!(why.contains("no change #7"), "{why}");
    assert_eq!(r.file_text(), FILE);
    assert!(r.rows("config.changed").is_empty());
}

/// The change's note, as the owner's notices place gets it: its post's id.
fn note_post(r: &Rig) -> String {
    r.core
        .outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .find(|a| crate::outbox::kind_of(a) == crate::owner_note::told::KIND)
        .expect("the change's note")
        .correlation_id
        .clone()
}

fn pressed_in(user: u64, guild: Option<&str>) -> Answerer {
    Answerer {
        label: format!("discord:{user}"),
        surface: Surface::Discord,
        discord: Some(DiscordOrigin {
            user_id: user.to_string(),
            channel_id: "777".into(),
            guild_id: guild.map(str::to_string),
        }),
        agent: None,
    }
}

/// Through the one door every Undo goes through (`told.undo`): a guest's
/// press, and the owner's from a shared place, put nothing back; the owner's
/// own from a private place puts the change back, the file byte for byte, and
/// the note says who undid it; a second press is refused by the core.
#[tokio::test]
async fn the_notes_undo_through_told_undo_puts_the_settings_back_byte_for_byte() {
    let r = rig_told();
    let sid = r.session();
    r.turn(&sid, change("import.notes.enabled", json!(false)))
        .await;
    let changed = r.file_text();
    assert_ne!(changed, FILE, "the change was written");
    back(&r);
    let post = note_post(&r);
    for who in [pressed_in(999, None), pressed_in(OWNER, Some("7"))] {
        let e = r.core.told_undo(&post, &who).await.unwrap_err();
        assert!(e.message.contains("does not count"), "{}", e.message);
        assert_eq!(r.file_text(), changed, "nothing was put back");
    }
    assert!(r.rows("config.undone").is_empty());
    let owner = Answerer {
        label: "cli".into(),
        surface: Surface::Cli,
        discord: None,
        agent: None,
    };
    let done = r.core.told_undo(&post, &owner).await.expect("undone");
    assert_eq!(r.file_text(), FILE, "byte for byte as it was");
    assert!(done.text.ends_with("\n↩️ Undone by cli."), "{}", done.text);
    assert!(!done.text.contains("Undo: "), "{}", done.text);
    assert_eq!(r.rows("config.undone").len(), 1);
    back(&r);
    let again = r.core.told_undo(&post, &owner).await.unwrap_err();
    assert!(
        again.message.contains("already undone"),
        "{}",
        again.message
    );
    assert_eq!(r.file_text(), FILE);
    assert_eq!(r.rows("config.changed").len(), 2, "put back once");
}
