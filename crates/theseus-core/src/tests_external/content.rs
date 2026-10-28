//! The operator's removal of a content label (theseus-9ymz0, design §6.3,
//! tests 24 to 26): `content.unlabel` clears one flag from a session, as
//! `policy.trust` clears a hold. It loosens, so it is judged as an answer is
//! (`judge_act`): from the owner, from a private place, never from a job's
//! process. The record and the `content.removed` row are one frame, and the
//! session's watchers hear `session.unlabelled`.

use super::*;
use crate::approval::Client;
use crate::approval::Surface;
use crate::content::{ContentFlags, Flag, ItemKind, Label};
use crate::external::Mode;
use theseus_protocol::{method, Message};

fn label(flag: Flag, at_ms: u64) -> Label {
    Label {
        flag,
        node: Some("nod_e1".into()),
        kind: ItemKind::FetchedPage,
        source: "http.fetch notes.example/today".into(),
        confidence: 0.93,
        band: flag.bar(),
        at_ms,
        judgment: String::new(),
    }
}

/// A core and a session carrying `injection` and `secret`.
fn labelled(dir: &Path) -> (Arc<Core>, String) {
    let core = core_tuned(
        dir,
        0,
        |_| Scripted::text("ok"),
        false,
        (Mode::Notify, Posture::Open),
        |_| {},
    );
    let sid = session(&core);
    let mut rec: SessionRecord = core.store.get_session(&sid).unwrap().unwrap();
    let mut flags = ContentFlags::default();
    assert!(flags.raise(label(Flag::Injection, 11)));
    assert!(flags.raise(label(Flag::Secret, 12)));
    rec.data = Some(Box::new(flags));
    core.store.put_session(&sid, &rec).unwrap();
    (core, sid)
}

fn flags_of(core: &Core, sid: &str) -> ContentFlags {
    let rec: SessionRecord = core.store.get_session(sid).unwrap().unwrap();
    rec.data.map(|b| *b).unwrap_or_default()
}

/// Test 24. The owner's removal clears the flag, keeps the others, writes the
/// `content.removed` row in the one frame, and publishes `session.unlabelled`
/// to the session's watchers.
#[tokio::test]
async fn removal_from_the_owner_clears_the_flag_writes_the_row_and_publishes() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = labelled(dir.path());
    let (tx, mut heard) = tokio::sync::mpsc::unbounded_channel();
    core.bus.watch(&sid, "test", tx);
    let ledger = core.store.ledger_len().unwrap();

    let r = core.unlabel_session(&sid, "injection", "test").unwrap();
    assert_eq!(r.session_id, sid);
    assert_eq!(r.flag, "injection");
    assert_eq!(r.by, "test");
    assert_eq!(r.how, method::CONTENT_UNLABEL);
    assert_eq!(r.label.kind, "fetched_page");
    assert_eq!(r.label.source, "http.fetch notes.example/today");
    assert_eq!(r.label.node.as_deref(), Some("nod_e1"));

    let now = flags_of(&core, &sid);
    assert!(!now.has(Flag::Injection), "the flag is gone");
    assert!(!now.frozen());
    assert!(now.has(Flag::Secret), "the other label stays");
    assert_eq!(now.removed.len(), 1, "and the removal is kept as history");
    assert_eq!(now.removed[0].flag, Flag::Injection);
    assert_eq!(now.removed[0].by, "test");

    let rows = ledgered(&core, "content.removed");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["flag"], "injection");
    assert_eq!(rows[0]["by"], "test");
    assert_eq!(rows[0]["how"], method::CONTENT_UNLABEL);
    assert_eq!(rows[0]["node"], "nod_e1");
    assert_eq!(
        core.store.ledger_len().unwrap(),
        ledger + 1,
        "the one row, and nothing else"
    );
    assert!(ledgered(&core, "approval.refused").is_empty());

    match heard.try_recv().expect("the watcher is told") {
        Message::Notification(n) => {
            assert_eq!(n.method, "session.unlabelled");
            assert_eq!(n.params["flag"], "injection");
            assert_eq!(n.params["session_id"], sid.as_str());
        }
        other => panic!("not a notification: {other:?}"),
    }
    assert!(heard.try_recv().is_err(), "told once");

    // The session's listing shows what is left.
    let i = core
        .session_list()
        .unwrap()
        .into_iter()
        .find(|i| i.session_id == sid)
        .unwrap();
    let words: Vec<&str> = i.labels.iter().map(|l| l.flag.as_str()).collect();
    assert_eq!(words, ["secret"]);
    assert_eq!(i.labels[0].band, "confirm");
}

/// Test 25. From a job's process (the MCP surface), or from a person in a
/// shared place, the removal is refused by `judge_act` exactly as
/// `policy.trust` is: the refusal is the session's row, nothing is removed and
/// no `content.removed` row is written.
#[tokio::test]
async fn removal_from_a_jobs_process_is_refused_as_a_trust_is() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = labelled(dir.path());
    assert!(crate::mcp_server::JUDGED.contains(&method::CONTENT_UNLABEL));
    let before = flags_of(&core, &sid);
    let e = rpc_as(
        &core,
        Client::new("sock#7", Surface::Mcp),
        method::CONTENT_UNLABEL,
        json!({"session_id": sid, "flag": "injection"}),
    )
    .await
    .unwrap_err();
    assert!(e.message.contains("does not count"), "{e:?}");
    assert_eq!(flags_of(&core, &sid), before, "nothing was removed");
    assert!(ledgered(&core, "content.removed").is_empty());
    let refused = crate::test_support::ledger_rows_2k(&core, "approval.refused");
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert_eq!(refused[0].session_id.as_deref(), Some(sid.as_str()));
    assert_eq!(refused[0].data["act"], method::CONTENT_UNLABEL);
    assert_eq!(refused[0].data["session_id"], sid.as_str());

    // A person who is not an owner is refused the same way.
    let ledger = core.store.ledger_len().unwrap();
    let e = core
        .unlabel_session(&sid, "secret", crate::test_support::stranger())
        .unwrap_err();
    let r = e
        .downcast_ref::<crate::approval::Refusal>()
        .unwrap_or_else(|| panic!("not a refusal: {e:#}"));
    assert!(r.why.contains("is not an owner"), "{}", r.why);
    assert_eq!(flags_of(&core, &sid), before);
    assert_eq!(
        core.store.ledger_len().unwrap(),
        ledger + 1,
        "the refusal only"
    );
    assert!(ledgered(&core, "content.removed").is_empty());
}

/// Test 26. Removing a flag that is not on the session is an error and writes
/// nothing, and so is a word that is not a flag.
#[tokio::test]
async fn removing_a_flag_that_is_not_there_is_an_error_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (core, sid) = labelled(dir.path());
    let before = flags_of(&core, &sid);
    let ledger = core.store.ledger_len().unwrap();

    let e = core
        .unlabel_session(&sid, "confidential", "test")
        .unwrap_err();
    assert!(
        e.to_string().contains("carries no confidential label"),
        "{e}"
    );
    let e = core.unlabel_session(&sid, "open", "test").unwrap_err();
    assert!(e.to_string().contains("is not a content flag"), "{e}");
    let e = core
        .unlabel_session("ses_nobody", "injection", "test")
        .unwrap_err();
    assert!(e.to_string().contains("no session is named"), "{e}");

    assert_eq!(flags_of(&core, &sid), before, "the record is as it was");
    assert_eq!(core.store.ledger_len().unwrap(), ledger, "nothing written");

    // A second removal of the same flag finds it gone.
    core.unlabel_session(&sid, "secret", "test").unwrap();
    let ledger = core.store.ledger_len().unwrap();
    let e = core.unlabel_session(&sid, "secret", "test").unwrap_err();
    assert!(e.to_string().contains("carries no secret label"), "{e}");
    assert_eq!(core.store.ledger_len().unwrap(), ledger);
    assert_eq!(ledgered(&core, "content.removed").len(), 1);
}
