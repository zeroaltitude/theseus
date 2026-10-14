//! `S` stops a conversation and cancels every task under it (theseus-cdcwr): it asks once more in the status line,
//! `S` again or `y` confirms, and anything else keeps the conversation as it was. `s` is unchanged.

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::ExecutionStopResult;

use crate::tests::{harbour_world, open_dm, script, Rig, T0};

fn stopped_all() -> ExecutionStopResult {
    ExecutionStopResult {
        execution: serde_json::from_value(json!({
            "execution_id": "exe_ses_dm0001", "session_id": "ses_dm0001", "kind": "conversation",
            "state": "waiting", "turns": 3, "interrupted": 0, "outstanding": 0, "queued_results": 0,
            "budget": {"limit_usd": 25.0, "spent_usd": 0.42, "reserved_usd": 0.0,
                       "held_unknown_usd": 0.0, "available_usd": 24.58},
            "created_at_ms": T0, "updated_at_ms": T0,
        }))
        .unwrap(),
        stopped: true,
        stopped_actions: vec![],
        verdicts: vec![],
        declined: vec![],
        turn_running: false,
        tasks_running: 0,
        wakes_pending: 0,
        cancelled: vec!["exe_ses_aaa111".into(), "exe_ses_bbb222".into()],
    }
}

/// `S` alone sends nothing; a second `S`, or `y`, sends `execution.stop` with `tasks`; any other key sends nothing.
#[tokio::test(start_paused = true)]
async fn shift_s_needs_a_second_key() {
    let world = harbour_world();
    world.lock().unwrap().answers.insert(
        "execution.stop".into(),
        Ok(serde_json::to_value(stopped_all()).unwrap()),
    );
    let mut rig = Rig::new(120, 24, script(world));
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('S')]).await;
    assert!(
        rig.screen()[23].contains("and cancel all its tasks? S or y again"),
        "{:?}",
        rig.screen()
    );
    let d = rig.daemon();
    assert!(
        d.asked("execution.stop").is_empty(),
        "one key stops nothing"
    );
    // Anything else keeps it.
    rig.press(&[KeyCode::Char('x')]).await;
    assert!(rig.screen()[23].contains("not stopped"));
    assert!(d.asked("execution.stop").is_empty());
    // The second `S` does it, with `tasks`.
    rig.press(&[KeyCode::Char('S'), KeyCode::Char('S')]).await;
    rig.asked("execution.stop", 1).await;
    assert_eq!(
        d.asked("execution.stop"),
        [json!({"execution_id": "exe_ses_dm0001", "author": "the TUI", "tasks": true})]
    );
    rig.shows("cancelled 2 task(s): aaa111 bbb222").await;
    // `y` confirms too.
    rig.press(&[KeyCode::Char('S'), KeyCode::Char('y')]).await;
    rig.asked("execution.stop", 2).await;
    // And the plain `s` is as it was: no `tasks` in the request.
    rig.press(&[KeyCode::Char('s'), KeyCode::Char('s')]).await;
    rig.asked("execution.stop", 3).await;
    assert_eq!(
        d.asked("execution.stop")[2],
        json!({"execution_id": "exe_ses_dm0001", "author": "the TUI"})
    );
}
