//! Which holds matter, and the owner's dismissal of a notice
//! (theseus-g00vv): `session.list` says whether a hold waits and whether it
//! has told the owner; health leaves out a session whose execution has ended
//! and names the mode; `policy.notice_seen` marks a notify hold told without a
//! call, and writes nothing else.

use super::*;
use crate::approval::{Answerer, Client, Surface};
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
    held_session_in(dir, mode, since_ms, |_| {}).await
}

/// [`held_session`] over a config `tune` has changed.
async fn held_session_in(
    dir: &Path,
    mode: Mode,
    since_ms: u64,
    tune: impl FnOnce(&mut crate::Config),
) -> (Arc<Core>, String) {
    let core = core_tuned(
        dir,
        0,
        |_| Scripted::text("ok"),
        false,
        (mode, Posture::Open),
        tune,
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
    assert_eq!(core.store.ledger_len().unwrap(), ledger, "no ledger row");
    assert!(core
        .store
        .get_meta::<u64>(&crate::external::told_key(&sid))
        .unwrap()
        .is_none());

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

/// A person who is not an owner is refused a dismissal, the refusal is
/// ledgered under the session it was about (theseus-tx1vj: it was ledgered
/// with no session, as `policy.trust`'s is not), and nothing else is written.
#[tokio::test]
async fn a_refused_dismiss_is_ledgered_with_its_session_id() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session(dir.path(), Mode::Notify, 1_000).await;
    let ledger = core.store.ledger_len().unwrap();
    let e = core
        .notice_seen(&sid, &crate::test_support::stranger())
        .unwrap_err();
    let r = e
        .downcast_ref::<crate::approval::Refusal>()
        .unwrap_or_else(|| panic!("not a refusal: {e:#}"));
    assert!(r.why.contains("is not an owner"), "{}", r.why);
    let refused = crate::test_support::ledger_rows_2k(&core, "approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(
        refused[0].session_id.as_deref(),
        Some(sid.as_str()),
        "the refusal is the session's row: {:?}",
        refused[0]
    );
    assert_eq!(refused[0].data["act"], method::POLICY_NOTICE_SEEN);
    assert_eq!(refused[0].data["session_id"], sid.as_str());
    // The refusal is the one row written: no mark, the hold stays.
    assert_eq!(core.store.ledger_len().unwrap(), ledger + 1);
    assert!(core
        .store
        .get_meta::<u64>(&crate::external::told_key(&sid))
        .unwrap()
        .is_none());
    assert!(hold(&core, &sid).is_some());
}

/// The owner on Discord, in a DM with them (`guild` none) or in a guild
/// channel.
fn owner_on_discord(guild: Option<&str>) -> Answerer {
    Answerer {
        label: "discord:42".into(),
        surface: Surface::Discord,
        discord: Some(theseus_protocol::DiscordOrigin {
            user_id: "42".into(),
            channel_id: "9001".into(),
            guild_id: guild.map(str::to_string),
        }),
        agent: None,
    }
}

/// The owner is refused a dismissal from a shared place (a guild channel
/// nobody bound private), ledgered under the session, and nothing is marked;
/// from a DM with them, the same dismissal counts and marks the hold told.
#[tokio::test]
async fn notice_seen_is_refused_in_a_shared_place_and_allowed_in_the_owners_dm() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = held_session_in(dir.path(), Mode::Notify, 1_000, |c| {
        c.places.owner = Some(vec!["discord:42".into()]);
    })
    .await;
    let ledger = core.store.ledger_len().unwrap();
    let e = core
        .notice_seen(&sid, &owner_on_discord(Some("7")))
        .unwrap_err();
    let r = e
        .downcast_ref::<crate::approval::Refusal>()
        .unwrap_or_else(|| panic!("not a refusal: {e:#}"));
    assert!(r.why.contains("shared place"), "{}", r.why);
    let refused = crate::test_support::ledger_rows_2k(&core, "approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0].session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(refused[0].data["via"], "discord:9001");
    assert_eq!(core.store.ledger_len().unwrap(), ledger + 1);
    let told = |core: &Core| {
        core.store
            .get_meta::<u64>(&crate::external::told_key(&sid))
            .unwrap()
    };
    assert!(told(&core).is_none(), "a refusal marks nothing");

    let r = core.notice_seen(&sid, &owner_on_discord(None)).unwrap();
    assert!(r.wrote);
    assert_eq!(told(&core), Some(1_000));
    assert_eq!(
        crate::test_support::ledger_rows_2k(&core, "approval.refused").len(),
        1,
        "the DM's dismissal is no refusal"
    );
}
