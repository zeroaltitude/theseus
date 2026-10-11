//! A person's first keystroke in the input line (theseus-tnky): one
//! `session.typing` per session per idle spell, never one a key, none from
//! another mode's keys, and no word of it to the operator when an older
//! daemon does not know it.

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::warm::SPELL_SECS;

use crate::tests::{harbour_world, open_dm, script, Rig, NOW, T0};

#[tokio::test]
async fn the_first_key_of_an_idle_spell_tells_the_daemon_once() {
    let mut rig = Rig::new(80, 24, script(harbour_world()));
    open_dm(&mut rig).await;
    // `i` opens the line: no key typed yet, and nothing sent.
    rig.press(&[KeyCode::Char('i')]).await;
    rig.settle().await;
    assert!(rig.daemon().asked("session.typing").is_empty());
    // The first character tells, once, however many follow.
    rig.type_text("and tomorrow").await;
    rig.asked("session.typing", 1).await;
    rig.settle().await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("session.typing"),
        [json!({"session_id": "ses_dm0001", "author": "the TUI"})]
    );
    // Sending the message and typing the next one, inside the spell: nothing.
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("and Sunday").await;
    rig.settle().await;
    assert_eq!(d.asked("session.typing").len(), 1, "one per idle spell");
    rig.press(&[KeyCode::Esc]).await;
    // Past the spell, the next first key tells again.
    NOW.with(|n| n.set(T0 + SPELL_SECS * 1000));
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("x").await;
    rig.asked("session.typing", 2).await;
}

#[tokio::test]
async fn keys_in_other_modes_tell_nothing() {
    let mut rig = Rig::new(80, 24, script(harbour_world()));
    open_dm(&mut rig).await;
    // The filter's characters are no message.
    rig.press(&[KeyCode::Char('/')]).await;
    rig.type_text("tide").await;
    rig.press(&[KeyCode::Esc]).await;
    rig.settle().await;
    assert!(rig.daemon().asked("session.typing").is_empty());
}

#[tokio::test]
async fn a_daemon_that_does_not_know_the_notice_costs_the_operator_no_word() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "session.typing".into(),
        Err((-32601, "unknown method \"session.typing\"".into())),
    );
    let mut rig = Rig::new(80, 24, script(world));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("hello").await;
    rig.asked("session.typing", 1).await;
    rig.settle().await;
    assert!(
        rig.runner.app.flash.is_none(),
        "no footer line: {:?}",
        rig.runner.app.flash
    );
}
