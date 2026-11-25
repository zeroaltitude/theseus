//! `self.file` through a real turn (issue 6): the model calls it, and the item is on the
//! self-improvement backlog, marked as a person's. Since issue 57 it asks no question by default: it
//! follows its own posture, the owner is told once with how to undo it (`self.drop`), and the undo
//! puts the backlog back. Set to "approve", the gate asks for the owner's yes as before. A child of
//! `tests_m3` for its rig and helpers, beside `pause_turn`.
//!
//! The unit tests call the tool's code past the gate; this is the one that proves the dispatch
//! reaches it. (A tool of the wrong backend, or one matched by a name it never has, is told "not a
//! tool the harness runs" or "does not run async", and nothing is filed.)

use theseus_protocol::rsi::{SelfBacklogParams, SelfDropParams, SelfMode};

use super::*;
use crate::approval::Answerer;

/// A rig whose model calls `self.file` once in a fresh conversation (the owner's alone: no
/// binding, a private place); `approve` sets the tool's own posture to "approve", and without it
/// the tool inherits the shipped template's `enforcement = "notify"`.
fn filing_rig(approve: bool) -> Rig {
    rig_with(
        vec![
            Scripted::tools(
                "",
                &[(
                    "t1",
                    "self_file",
                    json!({
                        "finding": "The digest folds two kinds into one line.",
                        "title": "digest folds two kinds",
                    }),
                )],
            ),
            Scripted::text("Noted."),
        ],
        |cfg| {
            cfg.self_improve.mode = SelfMode::Act;
            if approve {
                cfg.policy
                    .tools
                    .insert("self.file".into(), Posture::Approve);
            } else {
                // The template's own enforcement: this rig's is "approve", to test the gate's stops.
                cfg.policy.enforcement = Posture::Notify;
            }
        },
    )
}

/// One call of `self.file` under `[policy.tools] "self.file" = "approve"`: the gate's ask, the
/// owner's yes, and the call's result text.
async fn chat_filing() -> (Rig, String) {
    let r = filing_rig(true);
    let res = turn(&r.core, None, "file what I just found").await;
    let corr = res
        .awaiting_confirm
        .clone()
        .expect("the owner's yes is asked for");
    let pending = r.core.pending_confirms(&res.session_id).unwrap();
    assert_eq!(pending[0].tool, "self.file");
    assert!(
        r.core
            .self_backlog(&SelfBacklogParams { all: true })
            .unwrap()
            .items
            .is_empty(),
        "nothing is filed before the owner says yes"
    );
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

/// The FYI notes staged to the operator.
pub(super) fn notes(core: &Core) -> Vec<String> {
    core.outbox
        .open_for(crate::outbox::OPERATOR_TARGET)
        .iter()
        .map(|a| crate::outbox::body_of(a).clone())
        .filter(|b| b["kind"] == "notice" && b[crate::outbox::ANNOUNCE_KEY] == true)
        .filter_map(|b| b["text"].as_str().map(str::to_string))
        .filter(|t| t.starts_with("ℹ️ FYI · "))
        .collect()
}

/// KEY: an approved `self.file` is run by the tool, not refused by the dispatch, and the item is on the
/// backlog as a person's, filed by the owner.
#[tokio::test]
async fn an_approved_chat_filing_puts_the_item_on_the_backlog() {
    let (r, text) = chat_filing().await;
    assert!(text.starts_with("Filed "), "{text}");
    let items = r
        .core
        .self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items;
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].title, "digest folds two kinds");
    assert!(items[0].cause.starts_with("person:"), "{}", items[0].cause);
    assert!(text.contains(&items[0].id), "{text}");
    assert!(
        items[0].evidence.iter().any(|e| e.contains("the owner")),
        "{:?}",
        items[0].evidence
    );
}

/// KEY (issue 57): by default `self.file` runs with no question put, the item is filed as the
/// owner's, and the owner is told once: which conversation, what it did, and how to drop it,
/// with none of the model's words.
#[tokio::test]
async fn a_chat_filing_runs_unasked_and_tells_the_owner_once() {
    let r = filing_rig(false);
    let res = turn(&r.core, None, "file what I just found").await;
    assert!(res.awaiting_confirm.is_none(), "no question is put");
    assert!(r.core.pending_confirms(&res.session_id).unwrap().is_empty());
    let rs = results(&r.core, &res.session_id);
    assert_eq!(rs.len(), 1, "{rs:?}");
    assert_eq!(rs[0].0, ResultStatus::Ok, "{rs:?}");
    let items = r
        .core
        .self_backlog(&SelfBacklogParams { all: true })
        .unwrap()
        .items;
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(items[0].cause.starts_with("person:"), "{}", items[0].cause);
    let notes = notes(&r.core);
    assert_eq!(notes.len(), 1, "{notes:?}");
    let n = &notes[0];
    assert!(
        n.starts_with("ℹ️ FYI · A finding was filed for the self loop\n"),
        "{n}"
    );
    assert!(n.contains("from this chat"), "{n}");
    assert!(
        n.contains(&format!("Undo: `theseus self drop {}`", items[0].id)),
        "{n}"
    );
    assert!(
        !n.contains("digest folds") && !n.contains("two kinds"),
        "no word the model wrote: {n}"
    );
}

/// The undo (`self.drop`, the owner's word) puts the open backlog back as it was before the
/// filing: the same items at the same ranks. Someone else's drop, from a shared channel, is
/// refused and undoes nothing.
#[tokio::test]
async fn the_filings_undo_puts_the_backlog_back() {
    let r = filing_rig(false);
    r.core
        .self_file_in_chat("the gate runs clippy twice", None, "the owner")
        .unwrap();
    let open = |core: &Core| {
        let items = core
            .self_backlog(&SelfBacklogParams { all: false })
            .unwrap()
            .items;
        items
            .into_iter()
            .map(|i| (i.id, i.title, i.rank))
            .collect::<Vec<_>>()
    };
    let before = open(&r.core);
    assert_eq!(before.len(), 1, "{before:?}");
    turn(&r.core, None, "file what I just found").await;
    let after = open(&r.core);
    assert_eq!(after.len(), 2, "{after:?}");
    let filed = after
        .iter()
        .find(|i| i.1 == "digest folds two kinds")
        .expect("filed")
        .0
        .clone();
    let theirs = crate::tests_capture::call_on(
        &r.core,
        crate::approval::Surface::Discord,
        "self.drop",
        json!({"item": filed, "author": "someone",
               "discord": {"user_id": "7", "channel_id": "900000000000000123",
                           "guild_id": "900000000000000001"}}),
    )
    .await;
    assert!(theirs.error.is_some(), "{:?}", theirs.result);
    assert_eq!(open(&r.core), after, "nothing was undone");
    r.core
        .self_drop(
            &SelfDropParams {
                item: filed,
                ..Default::default()
            },
            &Answerer::from("the CLI"),
        )
        .unwrap();
    assert_eq!(open(&r.core), before, "the backlog is as it was");
}
