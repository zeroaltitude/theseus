//! The conversation's mode in the board (theseus-lyxy): Shift+Tab at the input
//! line asks for the next mode (ask → edits → plan → auto), the pane's header
//! names it, and an approved plan's checklist is in the pane and its count in
//! the header. In the list, Shift+Tab still walks back through the queue.

use crossterm::event::KeyCode;
use serde_json::json;

use crate::tests::{harbour_world, open_dm, script, Rig};

fn mode_is(mode: &str, plan: serde_json::Value) -> serde_json::Value {
    json!({"session_id": "ses_dm0001", "mode": mode, "plan": plan})
}

/// Reading the mode when a conversation opens, and Shift+Tab at the input line.
#[tokio::test(start_paused = true)]
async fn shift_tab_at_the_input_line_asks_for_the_next_mode() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("session.mode".into(), Ok(mode_is("edits", json!([]))));
    let mut rig = Rig::new(120, 24, script(world));
    open_dm(&mut rig).await;
    // The mode was read when the conversation opened, with no mode named.
    rig.asked("session.mode", 1).await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("session.mode"),
        [json!({"session_id": "ses_dm0001"})]
    );
    rig.shows("edits (shift+tab)").await;
    // At the prompt it asks for the next one: edits → plan.
    rig.press(&[KeyCode::Char('i'), KeyCode::BackTab]).await;
    rig.asked("session.mode", 2).await;
    assert_eq!(
        d.asked("session.mode")[1],
        json!({"session_id": "ses_dm0001", "mode": "plan"})
    );
}

/// In the list, Shift+Tab is still the walk back through the queue: it sends
/// no mode, and reads the conversation it lands on.
#[tokio::test(start_paused = true)]
async fn in_the_list_shift_tab_still_walks_back_and_sets_no_mode() {
    let mut rig = Rig::new(120, 24, script(harbour_world()));
    open_dm(&mut rig).await;
    rig.asked("session.mode", 1).await;
    rig.press(&[KeyCode::BackTab]).await;
    rig.asked("session.mode", 2).await;
    let asked = rig.daemon().asked("session.mode");
    assert!(
        asked.iter().all(|p| p.get("mode").is_none()),
        "only reads: {asked:?}"
    );
}

/// The notification moves the header and puts the checklist in the pane.
#[tokio::test(start_paused = true)]
async fn the_header_names_the_mode_and_the_pane_holds_the_checklist() {
    let world = harbour_world();
    let mut rig = Rig::new(120, 24, script(world));
    open_dm(&mut rig).await;
    rig.shows("ask (shift+tab)").await;
    rig.daemon().notify(
        "session.mode",
        json!({"session_id": "ses_dm0001", "mode": "edits", "by": "plan", "plan": [
            {"id": "tsk_a1", "title": "add the function", "state": "done"},
            {"id": "tsk_a2", "title": "test it", "state": "doing"},
            {"id": "tsk_a3", "title": "run all", "state": "todo"}]}),
    );
    rig.shows("plan 1/3").await;
    let screen = rig.screen().join("\n");
    assert!(screen.contains("edits (shift+tab)"), "{screen}");
    for line in ["✔ 1. add the function", "◼ 2. test it", "☐ 3. run all"] {
        assert!(screen.contains(line), "{line} in {screen}");
    }
}

/// A plan's card says its other yes in words that name the mode.
#[test]
fn a_plans_card_offers_edits_mode_on_e() {
    let line = crate::card::offer_line(&theseus_protocol::grants::GrantOffer::mode_edits());
    assert_eq!(
        line,
        "  [e] approve the plan, and accept edits: file edits here run with a notice"
    );
}
