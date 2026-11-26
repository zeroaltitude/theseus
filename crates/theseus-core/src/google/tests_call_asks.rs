//! `google.call` asks for every call that is not a read: its plan carries an
//! authority reason at every posture, and a whole turn that makes one waits
//! with nothing sent to Google. Against the loopback fake only.

use serde_json::{json, Value};
use theseus_google::fake::drive::{Drive, File};
use theseus_google::fake::gmail::MsgSpec;
use theseus_tools::{Tool, ToolCtx};

use super::tests::*;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::test_support::turn;

fn tool(r: &Rig) -> std::sync::Arc<dyn Tool> {
    r.core
        .tools
        .registry
        .get("google.call")
        .expect("registered")
        .clone()
}

fn plan(r: &Rig, input: &Value) -> theseus_tools::Plan {
    tool(r)
        .plan(input, &ToolCtx::for_tests(r.dir.path()))
        .unwrap_or_else(|e| panic!("{input}: {e}"))
}

/// A rig whose enforcement and `google.call` line are both `posture`.
async fn at(posture: Posture, script: Vec<Scripted>) -> Rig {
    rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        script,
        |c| {
            c.policy.enforcement = posture;
            c.policy.tools.insert("google.call".into(), posture);
        },
    )
    .await
}

/// The plan of `input` asks, at the posture of `r`: an authority reason, a
/// decision of Approve, and one no posture alone decides.
fn asks(r: &Rig, input: &Value) -> String {
    let p = plan(r, input);
    let why = p
        .authority
        .clone()
        .unwrap_or_else(|| panic!("{input} asks"));
    let t = tool(r);
    assert!(!r.core.tools.policy.posture_only(t.as_ref(), &p), "{input}");
    assert_eq!(
        r.core.tools.policy.decide(t.as_ref(), &p).posture,
        Posture::Approve,
        "{input}"
    );
    why
}

/// Every request the fake saw that was not a GET.
fn writes(r: &Rig) -> Vec<String> {
    r.fake
        .log()
        .into_iter()
        .filter(|l| l.verb != "GET")
        .map(|l| format!("{} {}", l.verb, l.path))
        .collect()
}

fn message_trash() -> Value {
    json!({"method": "gmail.users.messages.trash", "params": {"id": "m1"}})
}

fn file_trashed() -> Value {
    json!({"method": "drive.files.update", "params": {"fileId": "doc1"}, "body": {"trashed": true}})
}

fn calendar_made() -> Value {
    json!({"method": "calendar.calendars.insert", "body": {"summary": "Trips"}})
}

#[tokio::test]
async fn a_message_trash_a_trashed_patch_and_a_calendar_made_ask_at_every_posture() {
    for posture in Posture::ALL {
        let r = at(posture, vec![]).await;
        for (input, why) in [
            (message_trash(), "moves to or from the trash"),
            (file_trashed(), theseus_google::classify::UNLISTED),
            (calendar_made(), "changes or clears a calendar"),
        ] {
            assert!(asks(&r, &input).starts_with(why), "{posture:?} {input}");
        }
    }
}

#[tokio::test]
async fn a_message_trash_through_a_turn_waits_and_sends_nothing() {
    let r = at(
        Posture::Open,
        vec![call("c1", message_trash()), Scripted::text("Done.")],
    )
    .await;
    r.fake.mail().thread(
        "t1",
        vec![MsgSpec::new("m1", "Sam <sam@example.test>", "A", "a")],
    );
    let res = turn(&r.core, &session(&r.core), "bin it").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(writes(&r), Vec::<String>::new());
    assert!(!r.fake.mail().labels_of("m1").contains(&"TRASH".to_string()));
    assert_eq!(ledgered(&r.core, "tool.confirm_requested").len(), 1);
}

#[tokio::test]
async fn a_trashed_patch_through_a_turn_waits_and_sends_nothing() {
    let r = at(
        Posture::Open,
        vec![call("c1", file_trashed()), Scripted::text("Done.")],
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let res = turn(&r.core, &session(&r.core), "bin the plan").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(writes(&r), Vec::<String>::new());
    assert!(!drive.file("doc1").unwrap().trashed);
    assert_eq!(ledgered(&r.core, "tool.confirm_requested").len(), 1);
}

#[tokio::test]
async fn a_calendar_made_through_a_turn_waits_and_sends_nothing() {
    let r = at(
        Posture::Open,
        vec![call("c1", calendar_made()), Scripted::text("Done.")],
    )
    .await;
    let res = turn(&r.core, &session(&r.core), "make me a trips calendar").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(writes(&r), Vec::<String>::new());
    assert_eq!(ledgered(&r.core, "tool.confirm_requested").len(), 1);
}

#[tokio::test]
async fn a_drive_update_that_also_moves_a_parent_waits_and_sends_nothing() {
    let moved = json!({"method": "drive.files.update",
        "params": {"fileId": "doc1", "addParents": "team"}, "body": {"trashed": true}});
    let r = at(
        Posture::Open,
        vec![call("c1", moved), Scripted::text("Done.")],
    )
    .await;
    let drive = Drive::install(&r.fake);
    drive.add(File::doc("doc1", "Plan", "# Plan\n"));
    let res = turn(&r.core, &session(&r.core), "file it in team").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(writes(&r), Vec::<String>::new());
    assert!(!drive.file("doc1").unwrap().trashed);
}

#[tokio::test]
async fn a_thread_trash_waits_and_sends_nothing() {
    let trash = json!({"method": "gmail.users.threads.trash", "params": {"id": "t1"}});
    let r = at(
        Posture::Open,
        vec![call("c1", trash), Scripted::text("Done.")],
    )
    .await;
    r.fake.mail().thread(
        "t1",
        vec![MsgSpec::new("m1", "Sam <sam@example.test>", "A", "a")],
    );
    let res = turn(&r.core, &session(&r.core), "bin the thread").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(writes(&r), Vec::<String>::new());
}

#[tokio::test]
async fn a_message_put_into_the_mailbox_waits_and_nothing_is_put() {
    let insert = json!({"method": "gmail.users.messages.insert", "body": {"raw": "eA"}});
    let import = json!({"method": "gmail.users.messages.import", "body": {"raw": "eA"}});
    let r = at(
        Posture::Open,
        vec![call("c1", insert.clone()), Scripted::text("Done.")],
    )
    .await;
    for input in [&insert, &import] {
        let why = asks(&r, input);
        assert!(
            why.starts_with("puts a message into the mailbox, so the owner decides it"),
            "{why}"
        );
    }
    let res = turn(&r.core, &session(&r.core), "file this note in my inbox").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert_eq!(writes(&r), Vec::<String>::new());
    assert_eq!(ledgered(&r.core, "tool.confirm_requested").len(), 1);
}

/// What sends, deletes, shares, invites, puts mail into the mailbox, clears
/// a calendar, and every other change: each asks at every posture.
#[tokio::test]
async fn the_keep_calls_and_the_unlisted_writes_still_ask_at_every_posture() {
    let attendees = json!({"summary": "x", "attendees": [{"email": "a@example.test"}]});
    let cases = [
        (
            "calendar.calendars.delete",
            json!({"calendarId": "cal9"}),
            None,
        ),
        ("calendar.calendars.clear", json!({}), None),
        ("gmail.users.messages.send", json!({}), None),
        ("gmail.users.messages.insert", json!({}), None),
        ("gmail.users.messages.import", json!({}), None),
        ("gmail.users.messages.delete", json!({"id": "m77"}), None),
        ("gmail.users.messages.batchDelete", json!({}), None),
        ("drive.files.delete", json!({"fileId": "f1"}), None),
        ("drive.permissions.create", json!({"fileId": "f1"}), None),
        ("calendar.acl.insert", json!({}), None),
        (
            "calendar.events.insert",
            json!({"sendUpdates": "all"}),
            None,
        ),
        ("gmail.users.drafts.create", json!({}), Some(json!({}))),
        (
            "POST /gmail/v1/users/me/labels",
            json!({}),
            Some(json!({"name": "x"})),
        ),
        (
            "drive.files.create",
            json!({}),
            Some(json!({"name": "x", "parents": ["team"]})),
        ),
        (
            "drive.files.copy",
            json!({"fileId": "f1"}),
            Some(json!({"parents": ["team"]})),
        ),
        (
            "calendar.events.insert",
            json!({}),
            Some(json!({"summary": "x"})),
        ),
        ("calendar.events.insert", json!({}), Some(attendees.clone())),
        (
            "calendar.events.patch",
            json!({"eventId": "e1"}),
            Some(attendees),
        ),
        (
            "gmail.users.messages.modify",
            json!({"id": "m1"}),
            Some(json!({"removeLabelIds": ["INBOX"]})),
        ),
        (
            "gmail.users.messages.batchModify",
            json!({}),
            Some(json!({"ids": ["m1"], "addLabelIds": ["SPAM"]})),
        ),
        (
            "gmail.users.threads.modify",
            json!({"id": "t1"}),
            Some(json!({"addLabelIds": ["TRASH"]})),
        ),
        (
            "POST /gmail/v1/users/me/settings/filters",
            json!({}),
            Some(json!({"action": {}})),
        ),
        (
            "PUT /gmail/v1/users/me/settings/autoForwarding",
            json!({}),
            Some(json!({"enabled": true})),
        ),
        (
            "PUT /gmail/v1/users/me/settings/vacation",
            json!({}),
            Some(json!({"enableAutoReply": true})),
        ),
        ("POST /drive/v3/files/f1/watch", json!({}), Some(json!({}))),
        (
            "sheets.spreadsheets.batchUpdate",
            json!({"spreadsheetId": "s1"}),
            Some(json!({"requests": []})),
        ),
        (
            "POST /v1/documents/d1:batchUpdate",
            json!({}),
            Some(json!({"requests": []})),
        ),
        (
            "POST /v1/presentations/p1:batchUpdate",
            json!({}),
            Some(json!({"requests": []})),
        ),
    ];
    for posture in Posture::ALL {
        let r = at(posture, vec![]).await;
        for (m, params, body) in &cases {
            let mut input = json!({"method": m, "params": params});
            if let Some(b) = body {
                input["body"] = b.clone();
            }
            asks(&r, &input);
        }
    }
}

#[tokio::test]
async fn a_call_carries_no_header_of_its_callers() {
    let r = at(Posture::Open, vec![]).await;
    let input = json!({"method": "GET /drive/v3/files/f1",
        "headers": {"X-HTTP-Method-Override": "DELETE"}});
    let e = tool(&r)
        .plan(&input, &ToolCtx::for_tests(r.dir.path()))
        .unwrap_err();
    assert!(e.contains("headers"), "{e}");
}
