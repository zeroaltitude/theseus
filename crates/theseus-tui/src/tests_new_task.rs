//! `N` (theseus-7oq0w): a one-line prompt for a brief, and one `task.start` under the
//! conversation the cursor is on, or Home when it is on a task or nothing. Esc
//! starts nothing, an empty brief starts nothing, and the daemon's answer and its
//! refusal reach the footer.

use crossterm::event::KeyCode;
use serde_json::json;

use crate::tests::{harbour_world, open_dm, script, Answer, Rig};

fn started() -> serde_json::Value {
    json!({"task_id": "ses_0000ab1c2d3e4f", "session_id": "ses_0000ab1c2d3e4f",
           "short": "e4f9a1", "home_session": "ses_dm0001", "limit_usd": 0.5})
}

/// The brief typed after `N`, and enter: one `task.start` under the DM the
/// cursor is on, as the TUI.
#[tokio::test]
async fn n_then_a_brief_starts_a_task_under_the_selected_conversation() {
    let world = harbour_world();
    world
        .lock()
        .unwrap()
        .answers
        .insert("task.start".into(), Ok(started()));
    let mut rig = Rig::new(100, 24, script(world));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('N')]).await;
    rig.type_text("Count the boats").await;
    rig.shows("new task: the brief").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("task.start", 1).await;
    assert_eq!(
        rig.daemon().asked("task.start"),
        [json!({"brief": "Count the boats", "home": "ses_dm0001", "author": "the TUI"})]
    );
    rig.shows("started task e4f9a1").await;
}

/// On a task's row, or with nothing selected, the task goes under Home: the
/// call names no `home`.
#[tokio::test]
async fn n_on_a_task_or_nothing_starts_it_under_home() {
    let world = harbour_world();
    let mut rig = Rig::new(100, 24, script(world));
    rig.shows("ready  DM +1").await;
    rig.runner.app.selected = Some("ses_tide01".into());
    rig.press(&[KeyCode::Char('N')]).await;
    rig.type_text("Survey the north quay").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("task.start", 1).await;
    assert_eq!(
        rig.daemon().asked("task.start"),
        [json!({"brief": "Survey the north quay", "author": "the TUI"})]
    );
}

/// Esc leaves the prompt with nothing sent; an empty brief sends nothing and
/// stays in the prompt.
#[tokio::test]
async fn esc_and_an_empty_brief_start_nothing() {
    let mut rig = Rig::new(100, 24, script(harbour_world()));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('N'), KeyCode::Enter]).await;
    rig.shows("new task: the brief").await;
    rig.type_text("abandon this").await;
    rig.press(&[KeyCode::Esc]).await;
    rig.shows("no task started").await;
    // The keys are the board's again: `q` would quit, `?` opens help.
    rig.press(&[KeyCode::Char('?')]).await;
    rig.shows("start a task").await;
    rig.settle().await;
    assert!(rig.daemon().asked("task.start").is_empty());
}

/// A refusal is said in the footer with the daemon's reason.
#[tokio::test]
async fn a_refused_start_says_why_in_the_footer() {
    let world = harbour_world();
    let refused: Answer = Err((
        -32005,
        "a task start from discord:777 does not count".into(),
    ));
    world
        .lock()
        .unwrap()
        .answers
        .insert("task.start".into(), refused);
    let mut rig = Rig::new(120, 24, script(world));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('N')]).await;
    rig.type_text("Count the boats").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.shows("task.start: a task start from discord:777 does not count")
        .await;
}
