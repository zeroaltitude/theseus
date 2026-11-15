//! `self.file` through a real turn (issue 6): the model calls it, the gate asks for the owner's yes,
//! and once approved the item is on the self-improvement backlog, marked as a person's. A child of
//! `tests_m3` for its rig and helpers, beside `pause_turn`.
//!
//! The unit tests call the tool's code past the gate; this is the one that proves the dispatch
//! reaches it. (A tool of the wrong backend, or one matched by a name it never has, is told "not a
//! tool the harness runs" or "does not run async", and nothing is filed.)

use theseus_protocol::rsi::{SelfBacklogParams, SelfMode};

use super::*;

/// One call of `self.file` in a fresh conversation (the owner's alone: no binding, a private place),
/// the gate's ask, the owner's yes, and the call's result text.
async fn chat_filing() -> (Rig, String) {
    let r = rig_with(
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
        |cfg| cfg.self_improve.mode = SelfMode::Act,
    );
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
