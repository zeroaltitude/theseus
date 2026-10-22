//! `c` on the board (theseus-tlhjt): a task that has ended is not cancelled, and while one task's
//! pane is open `c` takes the row under the cursor, not the open task.

use crossterm::event::KeyCode;

use crate::tests_work::{regatta_world, rig};

/// Open the row whose name is typed into the filter.
async fn open(rig: &mut crate::tests::Rig, name: &str) {
    rig.press(&[KeyCode::Char('/')]).await;
    for c in name.chars() {
        rig.press(&[KeyCode::Char(c)]).await;
    }
    rig.press(&[KeyCode::Enter, KeyCode::Enter]).await;
}

#[tokio::test]
async fn c_on_a_finished_task_says_so_and_cancels_nothing() {
    let (mut rig, _) = rig(120, regatta_world(None)).await;
    open(&mut rig, "tide gauge").await;
    rig.press(&[KeyCode::Char('c')]).await;
    let s = rig.screen();
    assert!(
        s.iter()
            .any(|l| l.contains("that task has already finished")),
        "{s:#?}"
    );
    rig.press(&[KeyCode::Char('c')]).await;
    rig.settle().await;
    assert!(rig.daemon().asked("task.cancel").is_empty());
}

#[tokio::test]
async fn c_takes_the_cursors_row_while_another_task_is_open() {
    let (mut rig, _) = rig(120, regatta_world(None)).await;
    open(&mut rig, "Fetch").await;
    // Esc leaves the filter; the cursor moves to another row while Fetch stays open.
    rig.press(&[KeyCode::Esc, KeyCode::Char('k'), KeyCode::Char('c')])
        .await;
    let s = rig.screen();
    assert!(
        s.iter().any(|l| l.contains("cancel task Count the boats")),
        "{s:#?}"
    );
    rig.press(&[KeyCode::Char('c')]).await;
    rig.asked("task.cancel", 1).await;
    assert_eq!(
        rig.daemon().asked("task.cancel"),
        [serde_json::json!({"task": "ses_boats01", "author": "the TUI"})]
    );
}
