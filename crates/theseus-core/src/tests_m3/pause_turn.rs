//! `self.pause` through a real turn (issue 6, theseus-7glm6): the model calls it, the gate asks for
//! the owner's yes, and once approved the pause is set, then cleared the same way. A child of
//! `tests_m3` for its rig and helpers, split out under that file's ceiling.
//!
//! The unit tests call the tool's code past the gate; this is the one that proves the dispatch
//! reaches it. (A tool of the wrong backend is told "not a tool the harness runs", and nothing pauses.)

use theseus_protocol::rsi::SelfMode;

use super::*;

/// One call of `self.pause` in a fresh conversation (the owner's alone: no binding, a private place), the
/// gate's ask, the owner's yes, and the call's result text.
async fn chat_call(action: &str, paused_before: bool) -> (Rig, String) {
    let r = rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "self_pause",
                    json!({"action": action, "why": "a benchmark"}),
                )],
            ),
            Scripted::text("Noted."),
        ],
        |cfg| cfg.self_improve.mode = SelfMode::Act,
    );
    if paused_before {
        r.core
            .self_pause_apply(false, Some("an earlier benchmark"), "test");
    }
    let res = turn(&r.core, None, "the self builds").await;
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
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    (r, rs[0].1.clone())
}

/// KEY: an approved `self.pause` is run by the tool, not refused by the dispatch, and sets the pause.
#[tokio::test]
async fn an_approved_chat_pause_sets_the_pause() {
    let (r, text) = chat_call("pause", false).await;
    assert!(text.contains("New self builds are"), "{text}");
    let p = r.core.self_paused().expect("the pause is set");
    assert!(p.words().contains("a benchmark"), "{}", p.words());
}

/// KEY: an approved `unpause` clears a pause set before it.
#[tokio::test]
async fn an_approved_chat_unpause_clears_it() {
    let (r, text) = chat_call("unpause", true).await;
    assert_eq!(text, "Unpaused: new self builds may start again.");
    assert!(r.core.self_paused().is_none());
}
