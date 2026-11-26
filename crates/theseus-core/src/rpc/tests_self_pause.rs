//! Pausing the loop's new builds (issue 6, theseus-7glm6), through the whole core: setting a pause is
//! anyone's, clearing it the owner's (refused for a stranger and from a job's shell, with an
//! `approval.refused` row and the pause as it was); the chat tool is walled to the owner's own place
//! and the gate asks for his yes; and the pause shows in the pipeline and ends with the process.

use serde_json::{json, Value};
use theseus_protocol::rsi::SelfPauseParams;
use theseus_tools::{Tool, ToolCtx};

use super::tests_self_backlog::{acting, cli, stranger};
use crate::approval::Refusal;
use crate::rsi::pause_tool::{paused, Args, SelfPause};
use crate::test_support::rows;

fn set(why: &str) -> SelfPauseParams {
    SelfPauseParams {
        why: Some(why.into()),
        ..Default::default()
    }
}

fn clear() -> SelfPauseParams {
    SelfPauseParams {
        clear: true,
        ..Default::default()
    }
}

/// KEY: anyone sets it; the owner clears it; set, cleared and already-cleared each answer what they did.
#[test]
fn anyone_sets_a_pause_and_the_owner_clears_it() {
    let c = acting(0, 10);
    let r = c.self_pause(&set("a benchmark runs"), &stranger()).unwrap();
    assert!(r.paused && r.changed, "{r:?}");
    let words = r.words.unwrap();
    assert!(words.contains("a benchmark runs"), "{words}");
    assert!(c.self_paused().is_some());
    // Said again: nothing changes.
    assert!(!c.self_pause(&set("again"), &cli()).unwrap().changed);
    let r = c.self_pause(&clear(), &cli()).unwrap();
    assert!(!r.paused && r.changed && r.words.is_none(), "{r:?}");
    assert!(c.self_paused().is_none());
    assert!(
        !c.self_pause(&clear(), &cli()).unwrap().changed,
        "nothing to clear"
    );
}

/// KEY: a stranger's clear, and a job's, are refused and ledgered, and the pause is as it was.
#[test]
fn a_strangers_clear_and_a_jobs_are_refused_and_the_pause_stands() {
    let c = acting(0, 10);
    c.self_pause(&set("not now"), &cli()).unwrap();
    let e = c.self_pause(&clear(), &stranger()).unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    let e = c
        .self_pause(
            &SelfPauseParams {
                clear: true,
                from_job: Some("ses_0000aa".into()),
                ..Default::default()
            },
            &cli(),
        )
        .unwrap_err();
    assert!(e.downcast_ref::<Refusal>().is_some(), "{e:#}");
    assert!(c.self_paused().is_some(), "the pause stands");
    let refused: Vec<Value> = rows(&c, "approval.refused")
        .into_iter()
        .filter(|r| r["act"] == "self.pause")
        .collect();
    assert_eq!(refused.len(), 2, "{refused:?}");
}

/// The chat tool: its schema is closed, it follows its own posture (no ask at every posture, issue
/// 57), and its wall refuses a place that is not the owner's own and a session that is not a
/// conversation, changing nothing.
#[test]
fn the_chat_tool_is_closed_follows_its_posture_and_is_walled() {
    let s = SelfPause.input_schema();
    assert_eq!(s["additionalProperties"], json!(false));
    assert_eq!(s["required"], json!(["action"]));
    assert_eq!(
        s["properties"]["action"]["enum"],
        json!(["pause", "unpause"])
    );
    let plan = SelfPause
        .plan(&json!({"action": "pause"}), &ToolCtx::default())
        .unwrap();
    assert!(plan.authority.is_none(), "it follows its own posture");
    assert!(Args::parse(&json!({"action": "stop"})).is_err());
    assert!(Args::parse(&json!({})).is_err());
    let c = acting(0, 10);
    let a = Args::parse(&json!({"action": "pause", "why": "benchmarks"})).unwrap();
    // A shared place, and a private one whose session is no conversation of the owner's, are refused.
    let e = paused(&c, false, "ses_none", &a).unwrap_err();
    assert!(e.contains("only for a place he is alone in"), "{e}");
    let e = paused(&c, true, "ses_none", &a).unwrap_err();
    assert!(e.contains("not for a task's session"), "{e}");
    assert!(c.self_paused().is_none(), "nothing was paused");
}

/// The pause shows in the pipeline's own view, with its words, and a restart (a core built over the
/// same store) is not paused.
#[test]
fn a_pause_shows_in_the_pipeline_and_a_restart_clears_it() {
    let c = acting(0, 10);
    c.self_pause(&set("benchmarks"), &cli()).unwrap();
    let v = c.self_pipeline(&Default::default()).unwrap();
    assert_eq!(v.room.hold.as_deref(), Some("paused"), "{:?}", v.room);
    assert!(v.room.why.as_deref().unwrap().contains("benchmarks"));
    let again = super::tests_self_backlog::acting_over(&c);
    assert!(
        again.self_paused().is_none(),
        "a restarted daemon is not paused"
    );
}
