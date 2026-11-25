//! `self.pause` through a real turn (issue 6, theseus-7glm6): the model calls it, and the pause is
//! set, then cleared the same way. Since issue 57 it asks no question by default: it follows its own
//! posture and the owner is told once (a pause told only; an unpause with its undo, a pause again).
//! Set to "approve", the gate asks for the owner's yes as before. A child of `tests_m3` for its rig
//! and helpers, split out under that file's ceiling.
//!
//! The unit tests call the tool's code past the gate; this is the one that proves the dispatch
//! reaches it. (A tool of the wrong backend is told "not a tool the harness runs", and nothing pauses.)

use theseus_protocol::rsi::SelfMode;

use super::self_file_turn::{notes, press, shared_place};
use super::*;

/// A rig whose model calls `self.pause` once with `action` in a fresh conversation (the owner's
/// alone: no binding, a private place), paused before it when `paused_before`; `approve` sets the
/// tool's own posture to "approve", and without it the tool inherits the shipped template's
/// `enforcement = "notify"`.
fn pause_rig(action: &str, paused_before: bool, approve: bool) -> Rig {
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
        |cfg| {
            cfg.self_improve.mode = SelfMode::Act;
            if approve {
                cfg.policy
                    .tools
                    .insert("self.pause".into(), Posture::Approve);
            } else {
                // The template's own enforcement: this rig's is "approve", to test the gate's stops.
                cfg.policy.enforcement = Posture::Notify;
            }
        },
    );
    if paused_before {
        r.core
            .self_pause_apply(false, Some("an earlier benchmark"), "test");
    }
    r
}

/// One call of `self.pause` under `[policy.tools] "self.pause" = "approve"`: the gate's ask, the
/// owner's yes, and the call's result text.
async fn chat_call(action: &str, paused_before: bool) -> (Rig, String) {
    let r = pause_rig(action, paused_before, true);
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

/// One call of `self.pause` at the default posture: no question is put, and the call's result text.
async fn unasked(action: &str, paused_before: bool) -> (Rig, String) {
    let r = pause_rig(action, paused_before, false);
    let res = turn(&r.core, None, "the self builds").await;
    assert!(res.awaiting_confirm.is_none(), "no question is put");
    assert!(r.core.pending_confirms(&res.session_id).unwrap().is_empty());
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

/// KEY (issue 57): by default a pause runs with no question put, and the owner is told once, with
/// none of the model's words (its reason) and no undo: a pause only does less.
#[tokio::test]
async fn a_chat_pause_runs_unasked_and_is_told_once() {
    let (r, text) = unasked("pause", false).await;
    assert!(text.contains("New self builds are"), "{text}");
    assert!(r.core.self_paused().is_some());
    let notes = notes(&r.core);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].get("undo").is_none(), "told only: {:?}", notes[0]);
    let n = notes[0]["text"].as_str().unwrap();
    assert!(
        n.starts_with("ℹ️ FYI · New self builds are paused\n"),
        "{n}"
    );
    assert!(n.contains("from this chat"), "{n}");
    assert!(!n.contains("Undo"), "told only: {n}");
    assert!(!n.contains("benchmark"), "no word the model wrote: {n}");
}

/// KEY (issue 57): by default an unpause runs with no question put, the owner is told once with its
/// Undo, and the owner's press (`told.undo`, which pauses again) puts the builds back on hold, once.
/// Someone else's press from a shared channel is refused by the note's door and pauses nothing,
/// though a pause by itself is anyone's: the note's rule decides, not the method's.
#[tokio::test]
async fn a_chat_unpause_runs_unasked_and_its_undo_pauses_again() {
    let (r, text) = unasked("unpause", true).await;
    assert_eq!(text, "Unpaused: new self builds may start again.");
    assert!(r.core.self_paused().is_none());
    let notes = notes(&r.core);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(
        notes[0]["undo"],
        json!({"method": "self.pause", "id": "paused again from its note"})
    );
    let n = notes[0]["text"].as_str().unwrap();
    assert!(
        n.starts_with("ℹ️ FYI · New self builds may start again\n"),
        "{n}"
    );
    assert!(
        n.ends_with("Undo: `theseus self pause paused again from its note`"),
        "{n}"
    );
    let theirs = press(
        &r.core,
        &notes[0],
        crate::approval::Surface::Discord,
        "someone",
        shared_place(),
    )
    .await;
    assert_eq!(
        theirs.error.map(|e| e.code),
        Some(theseus_protocol::error_code::REFUSED)
    );
    assert!(r.core.self_paused().is_none(), "nothing was paused");
    let mine = press(
        &r.core,
        &notes[0],
        crate::approval::Surface::Cli,
        "the owner",
        None,
    )
    .await;
    assert!(mine.error.is_none(), "{:?}", mine.error);
    let p = r.core.self_paused().expect("paused again");
    assert!(
        p.words().contains("paused again from its note"),
        "{}",
        p.words()
    );
    // A second press is refused by the core: the note was undone once.
    r.core.self_pause_apply(true, None, "test");
    let again = press(
        &r.core,
        &notes[0],
        crate::approval::Surface::Cli,
        "the owner",
        None,
    )
    .await;
    assert!(again.error.is_some(), "{:?}", again.result);
    assert!(r.core.self_paused().is_none(), "undone once");
}

/// A call that changes nothing (an unpause with nothing paused) is no act, and tells nothing.
#[tokio::test]
async fn a_call_that_changes_nothing_tells_nothing() {
    let (r, text) = unasked("unpause", false).await;
    assert_eq!(text, "Nothing was paused.");
    assert!(notes(&r.core).is_empty());
}

/// KEY (issue 57): a pause or an unpause whose note cannot be written is not made (the note is
/// written first, and the change only after it), and the call says so. The retry, the same call
/// past the gate, makes it once, with its one note; a third call changes nothing and tells nothing.
#[tokio::test]
async fn a_change_whose_note_cannot_be_written_is_not_made_and_a_retry_makes_it_once() {
    use crate::rsi::backlog::tool::fail;
    use crate::rsi::pause_tool::{paused, Args};
    for (action, before) in [("pause", false), ("unpause", true)] {
        let r = pause_rig(action, before, false);
        fail::NEXT.store(true, std::sync::atomic::Ordering::SeqCst);
        let res = turn(&r.core, None, "the self builds").await;
        let rs = results(&r.core, &res.session_id);
        assert_eq!(rs.len(), 1, "{action}: {rs:?}");
        assert_eq!(rs[0].0, ResultStatus::Error, "{action}: {rs:?}");
        assert!(rs[0].1.contains("nothing changed"), "{action}: {}", rs[0].1);
        assert_eq!(r.core.self_paused().is_some(), before, "{action}: as it was");
        assert!(notes(&r.core).is_empty(), "{action}");
        let a = Args::parse(&json!({"action": action})).unwrap();
        paused(&r.core, true, &res.session_id, &a).unwrap();
        assert_eq!(r.core.self_paused().is_some(), !before, "{action}: made");
        assert_eq!(notes(&r.core).len(), 1, "{action}");
        paused(&r.core, true, &res.session_id, &a).unwrap();
        assert_eq!(notes(&r.core).len(), 1, "{action}: not told again");
    }
}
