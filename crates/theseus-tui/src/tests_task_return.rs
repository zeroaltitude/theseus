//! Inspect, join and return in the board (theseus-mu0e6): a task's breadcrumb, `p` and Backspace to the
//! conversation that started it, `I` for the inspector link, the steer's words on `i`, and a message
//! for a task that has reported going to its parent with the shared redirect line.

use std::sync::Arc;

use crossterm::event::KeyCode;
use serde_json::json;
use theseus_protocol::{attention, error_code, utc_hm};

use crate::tests::*;

/// The harbour's task `ses_tide01` open in a wide pane, its history holding one turn.
async fn in_the_task() -> Rig {
    let world = harbour_world();
    {
        let mut w = world.lock().unwrap();
        let mut history = dm_history();
        history["session"] = serde_json::to_value(info(
            "ses_tide01",
            theseus_protocol::SessionKind::Task,
            None,
            Some("check the tide tables"),
        ))
        .unwrap();
        for n in history["nodes"].as_array_mut().unwrap() {
            n["session_id"] = json!("ses_tide01");
            n["turn_id"] = json!("turn_t1");
        }
        w.histories.insert("ses_tide01".into(), history);
        w.answers.insert(
            "turn.inspect".into(),
            Ok(json!({"session_id": "ses_tide01", "turn_id": "turn_t1",
                      "url": "http://harbour.test/session/ses_tide01/turn/turn_t1"})),
        );
    }
    let mut rig = task_rig(script(world)).await;
    rig.until("the task's history", |r| {
        r.runner
            .app
            .detail
            .as_ref()
            .is_some_and(|d| d.turn.is_some())
    })
    .await;
    rig
}

async fn task_rig(script: crate::tests::Script) -> Rig {
    let mut rig = Rig::new(120, 24, script);
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Char('j'), KeyCode::Enter]).await;
    rig.shows("task tide01 of DM").await;
    rig
}

/// The task has reported: the push says complete, the pane stays where it is.
async fn ends(rig: &mut Rig) {
    let mut v = task(120, "ses_tide01", "ses_dm0001", "complete", 0.12);
    v.attention = attention(&v, &utc_hm);
    rig.daemon().notify("execution.changed", changed(&v));
    rig.settle().await;
}

/// A task's pane is headed by whose it is and the way back; a conversation's is not.
#[tokio::test]
async fn a_task_pane_names_its_parent_and_the_way_back() {
    let mut rig = in_the_task().await;
    rig.shows("p parent · I inspector").await;
    rig.press(&[KeyCode::Char('p')]).await;
    rig.shows("ses …dm0001").await;
    assert!(!rig.screen().join("\n").contains("p parent · I inspector"));
}

/// `p` and Backspace focus the parent conversation, and the footer says where you are.
#[tokio::test]
async fn p_and_backspace_return_to_the_parent() {
    for key in [KeyCode::Char('p'), KeyCode::Backspace] {
        let mut rig = in_the_task().await;
        assert_eq!(focused(&rig).as_deref(), Some("ses_tide01"));
        rig.press(&[key]).await;
        assert_eq!(focused(&rig).as_deref(), Some("ses_dm0001"), "{key:?}");
        rig.shows("back in DM · task tide01 running").await;
    }
}

/// In a conversation `p` is still the standing records' key, and Backspace does nothing.
#[tokio::test]
async fn in_a_conversation_p_keeps_its_meaning() {
    let mut rig = harbour_rig(120, 24);
    open_dm(&mut rig).await;
    rig.press(&[KeyCode::Backspace]).await;
    assert_eq!(focused(&rig).as_deref(), Some("ses_dm0001"));
    rig.press(&[KeyCode::Char('p')]).await;
    rig.asked("standing.list", 2).await;
    assert_eq!(focused(&rig).as_deref(), Some("ses_dm0001"));
}

/// `I` asks `turn.inspect` for the newest turn in the pane and prints the link.
#[tokio::test]
async fn capital_i_prints_the_inspector_link() {
    let mut rig = in_the_task().await;
    rig.press(&[KeyCode::Char('I')]).await;
    rig.asked("turn.inspect", 1).await;
    assert_eq!(
        rig.daemon().asked("turn.inspect"),
        [json!({"session_id": "ses_tide01", "turn_id": "turn_t1"})]
    );
    rig.shows("inspect http://harbour.test/session/ses_tide01/turn/turn_t1")
        .await;
}

/// Typing to a task that works says the task reads it at its next step, in the prompt and after.
#[tokio::test]
async fn a_message_to_a_working_task_says_when_it_reads_it() {
    let mut rig = in_the_task().await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.shows("task tide01 reads it at its next step>").await;
    rig.type_text("use the north channel").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    assert_eq!(
        rig.daemon().asked("turn.submit")[0]["session_id"],
        json!("ses_tide01")
    );
    rig.shows("↪ the task reads this at its next step").await;
}

/// `i` on a task that has reported offers main: the prompt says so, and the message goes to the
/// parent with the shared redirect line, the pane following it.
#[tokio::test]
async fn i_on_an_ended_task_sends_to_main() {
    let mut rig = in_the_task().await;
    ends(&mut rig).await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.shows("task tide01 reported; to main>").await;
    rig.type_text("and the south?").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 1).await;
    let sent = rig.daemon().asked("turn.submit");
    assert_eq!(sent[0]["session_id"], json!("ses_dm0001"));
    assert_eq!(
        sent[0]["input"],
        json!("[Written to the task “check the tide tables” (…tide01), and the task is over] and the south?")
    );
    assert_eq!(focused(&rig).as_deref(), Some("ses_dm0001"));
}

/// The task ends as the message leaves: the daemon refuses it, and it goes to main instead of
/// being lost.
#[tokio::test]
async fn a_message_that_loses_the_race_goes_to_main() {
    let inner = script(harbour_world());
    let racing: crate::tests::Script = Arc::new(move |m, p| {
        if m == "turn.submit" && p["session_id"] == "ses_tide01" {
            return Err((error_code::NOT_FOUND, "the execution is complete".into()));
        }
        inner(m, p)
    });
    let mut rig = task_rig(racing).await;
    rig.press(&[KeyCode::Char('i')]).await;
    rig.type_text("hurry").await;
    rig.press(&[KeyCode::Enter]).await;
    rig.asked("turn.submit", 2).await;
    let sent = rig.daemon().asked("turn.submit");
    assert_eq!(sent[0]["session_id"], json!("ses_tide01"));
    assert_eq!(sent[1]["session_id"], json!("ses_dm0001"));
    assert!(
        sent[1]["input"]
            .as_str()
            .unwrap()
            .ends_with("and the task is over] hurry"),
        "{sent:?}"
    );
    assert_eq!(focused(&rig).as_deref(), Some("ses_dm0001"));
}
