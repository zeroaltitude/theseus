//! Which holds matter, and the owner's dismissal of a notice
//! (theseus-g00vv): `session.list` says whether a hold waits and whether it
//! has told the owner; health leaves out a session whose execution has ended
//! and names the mode; `policy.notice_seen` marks a notify hold told without a
//! call, and writes nothing else.

use super::*;
use crate::approval::{Client, Surface};
use crate::external::Mode;
use theseus_protocol::method;

fn text(since_ms: u64) -> ExternalText {
    ExternalText {
        since_ms,
        tool: "http.fetch".into(),
        url: "https://example.test/a".into(),
        node_id: "trs_a".into(),
        ..Default::default()
    }
}

/// A core under `mode`, and a session with one finished turn (its execution
/// waits on input) that holds external text since `since_ms`.
async fn held_session(dir: &Path, mode: Mode, since_ms: u64) -> (Arc<Core>, String) {
    let core = core_mode(
        dir,
        0,
        |_| Scripted::text("ok"),
        false,
        (mode, Posture::Open),
    );
    let sid = session(&core);
    turn(&core, &sid, "hello").await;
    set_hold(&core, &sid, Some(text(since_ms)));
    (core, sid)
}

fn set_hold(core: &Core, sid: &str, h: Option<ExternalText>) {
    let mut rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    rec.external = h;
    core.store.put_session(sid, &rec).unwrap();
}

fn info(core: &Core, sid: &str) -> theseus_protocol::SessionInfo {
    core.session_list()
        .unwrap()
        .into_iter()
        .find(|i| i.session_id == sid)
        .unwrap()
}

fn mark_told(core: &Core, sid: &str, since_ms: u64) {
    core.store
        .append(&[theseus_store::NewRecord::json(
            theseus_store::kinds::META,
            Some(&crate::external::told_key(sid)),
            &since_ms,
        )
        .unwrap()])
        .unwrap();
}

#[tokio::test]
async fn under_notify_a_hold_never_waits_and_told_follows_the_mark() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Notify, 1_000).await;
    let i = info(&core, &sid);
    assert!(i.external_text.is_some());
    assert!(!i.external_waits, "a notify hold waits for nothing");
    assert!(!i.external_told);
    mark_told(&core, &sid, 1_000);
    let i = info(&core, &sid);
    assert!(i.external_told && !i.external_waits);
    // A new hold has a new since_ms: untold again.
    set_hold(&core, &sid, Some(text(2_000)));
    assert!(!info(&core, &sid).external_told);
    // No hold: neither flag.
    set_hold(&core, &sid, None);
    let i = info(&core, &sid);
    assert!(!i.external_waits && !i.external_told && i.external_text.is_none());
}

#[tokio::test]
async fn under_ask_a_hold_waits_while_its_execution_is_live_and_not_after_it_ends() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Ask, 1_000).await;
    assert!(info(&core, &sid).external_waits, "a live ask hold waits");
    let exec = core
        .store
        .get_session::<SessionRecord>(&sid)
        .unwrap()
        .unwrap()
        .execution_id
        .unwrap();
    core.cancel_execution(&exec, "test").await.unwrap();
    let i = info(&core, &sid);
    assert!(i.external_text.is_some(), "the hold stays on the record");
    assert!(!i.external_waits, "an ended execution makes no call");
}

#[tokio::test]
async fn health_lists_only_holds_that_can_still_matter_and_names_the_mode() {
    let dir = tempfile::tempdir().unwrap();
    let (core, live) = held_session(dir.path(), Mode::Notify, 1_000).await;
    let done = session(&core);
    turn(&core, &done, "hello").await;
    set_hold(&core, &done, Some(text(500)));
    let exec = core
        .store
        .get_session::<SessionRecord>(&done)
        .unwrap()
        .unwrap()
        .execution_id
        .unwrap();
    core.cancel_execution(&exec, "test").await.unwrap();
    let h = core.health();
    let ids: Vec<&str> = h
        .external_text
        .iter()
        .map(|i| i.session_id.as_str())
        .collect();
    assert_eq!(ids, [live.as_str()], "the ended one is left out");
    assert_eq!(h.external_text_mode, "notify");
    assert!(hold(&core, &done).is_some(), "and kept on the record");
}

fn mcp() -> Client {
    Client::new("sock#7", Surface::Mcp)
}

#[tokio::test]
async fn notice_seen_marks_the_hold_told_and_writes_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Notify, 1_000).await;
    let h = hold(&core, &sid).unwrap();
    assert!(
        crate::external::held_untold(&core.store, &sid, Mode::Notify)
            .unwrap()
            .is_some()
    );
    let ledger = core.store.ledger_len().unwrap();
    let r = core.notice_seen(&sid, &"test".into()).unwrap();
    assert!(r.wrote);
    assert_eq!(r.held, h);
    assert!(
        crate::external::held_untold(&core.store, &sid, Mode::Notify)
            .unwrap()
            .is_none()
    );
    assert_eq!(hold(&core, &sid), Some(h), "the hold stays");
    assert_eq!(core.store.ledger_len().unwrap(), ledger, "no ledger row");
    // Again: nothing to write.
    assert!(!core.notice_seen(&sid, &"test".into()).unwrap().wrote);
    // A later hold tells again.
    set_hold(&core, &sid, Some(text(2_000)));
    assert!(
        crate::external::held_untold(&core.store, &sid, Mode::Notify)
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn notice_seen_is_an_error_for_no_hold_and_for_a_hold_that_waits() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Notify, 1_000).await;
    set_hold(&core, &sid, None);
    let ledger = core.store.ledger_len().unwrap();
    let e = core.notice_seen(&sid, &"test".into()).unwrap_err();
    assert!(e.to_string().contains("holds no external text"), "{e}");

    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Ask, 1_000).await;
    let before = core.store.ledger_len().unwrap();
    let e = core.notice_seen(&sid, &"test".into()).unwrap_err();
    assert!(e.to_string().contains("theseus policy trust"), "{e}");
    assert!(core
        .store
        .get_meta::<u64>(&crate::external::told_key(&sid))
        .unwrap()
        .is_none());
    assert_eq!(core.store.ledger_len().unwrap(), before);
    let _ = ledger;
}

#[tokio::test]
async fn notice_seen_from_mcp_is_refused_and_writes_nothing_but_the_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Notify, 1_000).await;
    assert!(crate::mcp_server::JUDGED.contains(&method::POLICY_NOTICE_SEEN));
    let e = rpc_as(
        &core,
        mcp(),
        method::POLICY_NOTICE_SEEN,
        json!({"session_id": sid}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("does not change"), "{e:?}");
    assert!(core
        .store
        .get_meta::<u64>(&crate::external::told_key(&sid))
        .unwrap()
        .is_none());
    assert!(hold(&core, &sid).is_some());
}
