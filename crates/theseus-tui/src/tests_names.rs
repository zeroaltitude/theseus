//! What the TUI calls a session and what it sends for one (theseus-0n1v): a
//! task by its title in the tree, its notice and its arm prompt; a title its
//! first answer lacked asked again once its turns grow; the input line's
//! turn on the session's last profile; and `Enter` after a filter opening
//! the row shown.

use std::sync::{Arc, Mutex};

use crossterm::event::KeyCode;
use serde_json::{json, Value};
use theseus_protocol::{attention, utc_hm, SessionKind};

use crate::tests::{
    conv, harbour_world, info, open_dm, opened, script, task, Captured, Rig, World, NOW, T0,
};

/// The harbour, its tide task labelled `task` as the store labels every
/// task, and titled.
fn quay_world() -> Arc<Mutex<World>> {
    let world = harbour_world();
    let quay = info(
        "ses_tide01",
        SessionKind::Task,
        Some("task"),
        Some("Survey the north quay"),
    );
    world
        .lock()
        .unwrap()
        .titles
        .insert("ses_tide01".into(), serde_json::to_value(quay).unwrap());
    world
}

fn changed<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap()
}

#[tokio::test]
async fn a_task_is_named_by_its_title_in_the_tree_the_prompt_and_the_notice() {
    let mut rig = Rig::new(80, 24, script(quay_world()));
    let out = Captured::default();
    rig.runner.out = Box::new(out.clone());
    rig.shows("└ ◐ turn 2  Survey the north quay").await;
    assert!(
        !rig.screen().iter().any(|l| l.trim_end().ends_with("task")),
        "no row is named `task`: {:?}",
        rig.screen()
    );
    // The arm prompt: the task by its title.
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('j'), KeyCode::Down])
        .await;
    assert_eq!(rig.runner.app.selected.as_deref(), Some("ses_tide01"));
    rig.press(&[KeyCode::Enter, KeyCode::Char('c')]).await;
    assert!(
        rig.screen()[23].contains("cancel task Survey the north quay (…tide01)? c again"),
        "{:?}",
        rig.screen()
    );
    rig.press(&[KeyCode::Char('x'), KeyCode::Esc]).await;
    // The notice: the task finishes, and its notice names it by its title.
    let mut tide = task(140, "ses_tide01", "ses_dm0001", "complete", 0.12);
    tide.previous = Some("running".into());
    rig.daemon().notify("execution.changed", changed(&tide));
    rig.shows("◆ done  Survey the north quay").await;
    NOW.with(|n| n.set(T0 + 1_000));
    rig.until("the notice", |r| {
        r.screen()[23].contains("Survey the north quay reported")
    })
    .await;
    assert!(
        !out.text().contains("task reported"),
        "the notice's words: {:?}",
        out.text()
    );
}

#[tokio::test]
async fn a_session_whose_first_answer_lacked_its_label_is_asked_again_as_its_turns_grow() {
    let world = harbour_world();
    let mut rig = Rig::new(80, 24, script(world.clone()));
    rig.shows("ready  DM +1").await;
    let d = rig.daemon();
    // Its first view, from `session.open`'s frame, before its record: the
    // daemon knows no session by that id yet.
    d.notify("execution.changed", changed(&opened(103, "ses_rt0001")));
    rig.shows("ses rt0001").await;
    let asks = |d: &crate::tests::Daemon| {
        d.asked("session.list")
            .iter()
            .filter(|p| p["ids"].as_array().unwrap().contains(&json!("ses_rt0001")))
            .count()
    };
    rig.until("its first ask", |r| asks(&r.daemon()) == 1).await;
    world.lock().unwrap().titles.insert(
        "ses_rt0001".into(),
        changed(&info(
            "ses_rt0001",
            SessionKind::Conversation,
            Some("racetest"),
            None,
        )),
    );
    // Another view at the same turns asks nothing.
    let mut same = opened(104, "ses_rt0001");
    same.spent_usd = 0.01;
    d.notify("execution.changed", changed(&same));
    rig.settle().await;
    assert_eq!(asks(&d), 1, "no re-ask while its turns stand");
    // Its first turn starts: asked again, and named.
    let mut turn = conv(105, "ses_rt0001", "running", None, 0.02);
    turn.turns = 1;
    turn.attention = attention(&turn, &utc_hm);
    d.notify("execution.changed", changed(&turn));
    rig.shows("racetest").await;
    // Views after it, named now, ask nothing more: never a re-ask a frame.
    for pos in 106..110 {
        let mut more = turn.clone();
        more.position = pos;
        d.notify("execution.changed", changed(&more));
    }
    rig.settle().await;
    assert_eq!(asks(&d), 2, "{:?}", d.asked("session.list"));
    let filtered = {
        rig.press(&[KeyCode::Char('/')]).await;
        rig.type_text("racetest").await;
        rig.screen()
    };
    assert!(
        filtered[1].contains("racetest"),
        "the filter finds it by its name: {filtered:?}"
    );
}

#[tokio::test]
async fn the_input_line_sends_the_sessions_last_profile_carried() {
    let world = harbour_world();
    let set_profile = |profile: &str, turns: u64| {
        let mut dm = info("ses_dm0001", SessionKind::Conversation, Some("DM"), None);
        dm.profile = Some(profile.into());
        dm.turns = turns;
        // `session.list` and the history's session say the same.
        let mut w = world.lock().unwrap();
        if let Some(h) = w.histories.get_mut("ses_dm0001") {
            h["session"] = changed(&dm);
        }
        w.titles.insert("ses_dm0001".into(), changed(&dm));
    };
    set_profile("glm", 3);
    let mut rig = Rig::new(80, 24, script(world.clone()));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("and tomorrow?").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    let d = rig.daemon();
    assert_eq!(
        d.asked("turn.submit")[0],
        json!({"session_id": "ses_dm0001", "input": "and tomorrow?", "author": "the TUI",
               "profile": "glm", "carried": true})
    );
    // Its next turn ran on another profile (another surface chose it): once
    // its turn settles, the TUI reads it again, and the next line carries it.
    set_profile("sonnet", 4);
    let mut dm = conv(120, "ses_dm0001", "waiting", None, 0.50);
    dm.turns = 4;
    dm.attention = attention(&dm, &utc_hm);
    d.notify("execution.changed", changed(&dm));
    rig.until("the session read again", |r| {
        r.runner
            .app
            .board
            .info("ses_dm0001")
            .is_some_and(|i| i.profile.as_deref() == Some("sonnet"))
    })
    .await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("and Sunday?").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 2).await;
    assert_eq!(d.asked("turn.submit")[1]["profile"], "sonnet");
    assert_eq!(d.asked("turn.submit")[1]["carried"], true);
}

#[tokio::test]
async fn enter_after_a_filter_opens_the_row_shown() {
    let mut rig = Rig::new(80, 24, script(quay_world()));
    rig.shows("Survey the north quay").await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('j')]).await;
    assert_eq!(rig.runner.app.selected.as_deref(), Some("ses_dm0001"));
    rig.press(&[KeyCode::Char('/')]).await;
    rig.type_text("quay").await;
    rig.press(&[KeyCode::Enter, KeyCode::Enter]).await;
    rig.shows("ses …tide01").await;
    assert_eq!(
        rig.runner.app.shown(),
        Some("ses_tide01"),
        "the row the filter showed is open, not the one it hid"
    );
}

/// The session in focus, its answer lagging its view for 50 turns (the
/// daemon writes the session's record after the turn's last view): asked
/// again at most twice a turn, never once a view (the review of theseus-0n1v).
#[tokio::test]
async fn a_lagging_answer_is_asked_again_at_most_twice_a_turn() {
    let mut rig = Rig::new(80, 24, script(harbour_world()));
    open_dm(&mut rig).await;
    let d = rig.daemon();
    let asks = |d: &crate::tests::Daemon| {
        d.asked("session.list")
            .iter()
            .filter(|p| {
                p["ids"]
                    .as_array()
                    .is_some_and(|a| a.contains(&json!("ses_dm0001")))
            })
            .count()
    };
    rig.settle().await;
    let before = asks(&d);
    let mut position = 200;
    for turns in 4..54 {
        // Two views a stage: its start, and its settling.
        for state in ["running", "running", "waiting", "waiting"] {
            let mut v = conv(position, "ses_dm0001", state, None, 0.5);
            position += 1;
            v.turns = turns;
            v.attention = attention(&v, &utc_hm);
            d.notify("execution.changed", changed(&v));
        }
        rig.settle().await;
    }
    let asked = asks(&d) - before;
    assert!(
        (50..=100).contains(&asked),
        "{asked} asks over 50 turns of 200 views"
    );
}

/// `Enter` after a filter that shows nothing opens nothing and reads nothing.
#[tokio::test]
async fn enter_after_a_filter_that_shows_nothing_does_nothing() {
    let mut rig = Rig::new(80, 24, script(quay_world()));
    rig.shows("Survey the north quay").await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Char('j')]).await;
    let selected = rig.runner.app.selected.clone();
    rig.press(&[KeyCode::Char('/')]).await;
    rig.type_text("zzzz").await;
    let reads = rig.daemon().asked("session.history").len();
    rig.press(&[KeyCode::Enter, KeyCode::Enter]).await;
    rig.settle().await;
    assert_eq!(rig.runner.app.shown(), None);
    assert_eq!(rig.runner.app.selected, selected);
    assert_eq!(rig.daemon().asked("session.history").len(), reads);
}

/// A title of 200 characters, wide ones and control characters among them:
/// the tree cuts it at a character's edge, within its width, and draws no
/// control character.
#[tokio::test]
async fn a_long_title_with_control_characters_is_cut_cleanly_in_the_tree() {
    let world = harbour_world();
    let long: String = format!("Survey\tthe \u{1b}[31mnorth quay {}", "港é".repeat(90));
    let quay = info("ses_tide01", SessionKind::Task, Some("task"), Some(&long));
    world
        .lock()
        .unwrap()
        .titles
        .insert("ses_tide01".into(), serde_json::to_value(quay).unwrap());
    let mut rig = Rig::new(80, 24, script(world));
    rig.shows("north quay").await;
    for line in rig.screen() {
        assert!(!line.chars().any(char::is_control), "{line:?}");
        // A cell a character: a wide one's second cell is drawn blank.
        let cells = line.chars().count();
        assert!(cells <= 80, "{cells} cells: {line:?}");
    }
}
