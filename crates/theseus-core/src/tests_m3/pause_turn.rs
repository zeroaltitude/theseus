//! `self.pause` through a real turn (issue 6, theseus-7glm6): the model calls it, the gate asks for
//! the owner's yes, and once approved the call reaches the tool's own code. A child of `tests_m3` for
//! its rig and helpers, split out under that file's ceiling.
//!
//! The unit tests call the tool's code past the gate; this is the one that proves the dispatch
//! reaches it. (A tool of the wrong backend is told "not a tool the harness runs", and nothing pauses.)

use theseus_protocol::rsi::SelfMode;

use super::*;

/// KEY: an approved `self.pause` is run by the tool, not refused by the dispatch. This session has no
/// owner binding, so the tool's wall answers in its own words and nothing is paused: the wall's
/// words are how the test sees the tool ran.
#[tokio::test]
async fn an_approved_chat_pause_reaches_the_tool() {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "self_pause",
                    json!({"action": "pause", "why": "a benchmark"}),
                )],
            ),
            Scripted::text("Noted."),
        ],
        |cfg| cfg.self_improve.mode = SelfMode::Act,
    );
    let res = turn(&r.core, None, "pause the self builds").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the owner's yes is asked for");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending[0].tool, "self.pause");
    r.core.confirm_action(&corr, true, None, "test").unwrap();
    r.core
        .continue_execution(res.execution_id.as_deref().unwrap())
        .await
        .unwrap()
        .unwrap();
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs.len(), 1, "{rs:?}");
    assert!(
        !rs[0].1.contains("not a tool the harness runs"),
        "the dispatch does not reach the tool: {rs:?}"
    );
    assert!(rs[0].1.contains("self.pause acts for the owner"), "{rs:?}");
    assert!(r.core.self_paused().is_none(), "the wall held");
}
