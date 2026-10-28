//! What asks, and what is counted (the Google design §3.2, §4.3, and the
//! owner's decision that every send asks): a send, a delete, a share and the
//! like are authority, so no posture or grant loosens them; and the
//! daily brake counts a send by `google.call`.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_google::fake::Reply;
use theseus_tools::ToolCtx;

use super::tests::*;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::test_support::turn;

fn plan_of(r: &Rig, input: Value) -> Result<theseus_tools::Plan, String> {
    let tool = r
        .core
        .tools
        .registry
        .get("google.call")
        .expect("google.call is registered")
        .clone();
    tool.plan(&input, &ToolCtx::for_tests(r.dir.path()))
}

#[tokio::test]
async fn a_send_through_google_call_asks_at_every_posture_and_no_grant_loosens_it() {
    let send = json!({"method": "gmail.users.drafts.send", "body": {"id": "d1"}});
    let open = |c: &mut crate::Config| {
        c.policy.enforcement = Posture::Open;
        c.policy.tools.insert("google.call".into(), Posture::Open);
    };
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![
            call("c1", json!({"method": "drive.files.list"})),
            Scripted::text("Listed."),
            call("c2", send.clone()),
        ],
        open,
    )
    .await;
    r.fake.json(
        "POST",
        "/gmail/v1/users/me/drafts/send",
        json!({"id": "m1"}),
    );
    r.fake.json("GET", "/drive/v3/files", json!({"files": []}));
    let sid = session(&r.core);
    // A read runs unasked at `open`, and holds nothing: its result is marked, no more.
    let res = turn(&r.core, &sid, "list my files").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(crate::external::read_any(&r.core.store, &sid));
    assert!(ledgered(&r.core, "session.external_read").is_empty());
    // The plan names the authority, and the gate's decision is Approve with the
    // floor, the lists and the posture all `open`: the one thing every loosening
    // layer (modes, a routine's grant) asks first is
    // whether only the tool's posture decides the call, and here it does not.
    let tool = r.core.tools.registry.get("google.call").unwrap().clone();
    let plan = plan_of(&r, send.clone()).unwrap();
    assert!(
        plan.authority
            .as_deref()
            .is_some_and(|a| a.contains("sends mail")),
        "{plan:?}"
    );
    let d = r.core.tools.policy.decide(tool.as_ref(), &plan);
    assert_eq!(d.posture, Posture::Approve, "{}", d.reason);
    assert!(!r.core.tools.policy.posture_only(tool.as_ref(), &plan));
    // A read at the same posture is decided by the posture alone, which is open.
    let read = plan_of(&r, json!({"method": "drive.files.list"})).unwrap();
    assert!(r.core.tools.policy.posture_only(tool.as_ref(), &read));
    assert_eq!(
        r.core.tools.policy.decide(tool.as_ref(), &read).posture,
        Posture::Open
    );
    // Through a whole turn: the send waits, and nothing was sent.
    let res = turn(&r.core, &sid, "send it").await;
    assert!(res.awaiting_confirm.is_some(), "{res:?}");
    assert!(
        r.fake
            .log()
            .iter()
            .all(|l| !l.path.ends_with("/drafts/send")),
        "nothing was sent"
    );
    let asked = ledgered(&r.core, "tool.confirm_requested");
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert!(
        asked[0]["reason"].as_str().unwrap().contains("approve"),
        "{asked:?}"
    );
    assert!(
        asked[0]["reason"].as_str().unwrap().contains("sends mail"),
        "{asked:?}"
    );
}

#[tokio::test]
async fn google_call_classifies_delete_permissions_and_acl_as_authority_in_its_plan() {
    let r = rig(vec![]).await;
    let authority = |m: &str, params: Value| {
        plan_of(&r, json!({"method": m, "params": params}))
            .unwrap()
            .authority
            .is_some()
    };
    for (m, params) in [
        ("drive.files.delete", json!({"fileId": "f1"})),
        ("drive.permissions.create", json!({"fileId": "f1"})),
        (
            "drive.permissions.delete",
            json!({"fileId": "f1", "permissionId": "p1"}),
        ),
        ("calendar.acl.insert", json!({})),
        ("calendar.calendars.clear", json!({})),
        ("gmail.users.messages.trash", json!({"id": "m1"})),
        ("gmail.users.messages.send", json!({})),
        ("gmail.users.messages.insert", json!({})),
        ("calendar.events.insert", json!({"sendUpdates": "all"})),
        ("DELETE /drive/v3/files/abc", json!({})),
        ("POST /drive/v3/files/abc/permissions", json!({})),
    ] {
        assert!(authority(m, params), "{m} is authority");
    }
    for (m, params) in [
        ("drive.files.list", json!({})),
        ("drive.permissions.list", json!({"fileId": "f1"})),
        ("gmail.users.drafts.create", json!({})),
        ("calendar.events.insert", json!({"sendUpdates": "none"})),
        (
            "sheets.spreadsheets.values.get",
            json!({"spreadsheetId": "s", "range": "A1"}),
        ),
        ("POST /gmail/v1/users/me/labels", json!({})),
    ] {
        assert!(!authority(m, params), "{m} is not authority");
    }
    // A plan names the class: a read is a read, anything else a write.
    let class = |m: &str, p: Value| {
        plan_of(&r, json!({"method": m, "params": p}))
            .unwrap()
            .class
    };
    assert_eq!(
        class("drive.files.list", json!({})),
        Some(theseus_tools::ToolClass::Read)
    );
    assert_eq!(
        class("drive.files.delete", json!({"fileId": "f"})),
        Some(theseus_tools::ToolClass::Write)
    );
    // The model's mistakes are invalid input, not a call.
    assert!(plan_of(&r, json!({"method": "nonsense"})).is_err());
    assert!(plan_of(&r, json!({"method": "drive.files.get"}))
        .unwrap_err()
        .contains("params.fileId"));
    assert!(plan_of(&r, json!({"method": "drive.files.list", "extra": 1})).is_err());
}

#[tokio::test]
async fn a_service_the_config_leaves_out_is_not_called() {
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![],
        |c| c.google.services = vec!["gmail".into()],
    )
    .await;
    let e = plan_of(&r, json!({"method": "drive.files.list"})).unwrap_err();
    assert!(e.contains("drive is not in `[google] services`"), "{e}");
    assert!(plan_of(&r, json!({"method": "gmail.users.labels.list"})).is_ok());
}

async fn send_with(r: &Rig) -> Result<Value, String> {
    let tool = r.core.tools.registry.get("google.call").unwrap().clone();
    let input = json!({"method": "gmail.users.messages.send", "body": {"raw": "x"}});
    tool.run_async(&input, &ToolCtx::for_tests(r.dir.path()))
        .await
        .map(|(o, _)| o.meta)
        .map_err(|f| f.message)
}

#[tokio::test]
async fn the_daily_send_cap_counts_google_call_sends() {
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![],
        |c| c.google.max_sends_per_day = 2,
    )
    .await;
    r.g.sends.attach(r.core.store.clone());
    r.fake.json(
        "POST",
        "/gmail/v1/users/me/messages/send",
        json!({"id": "m1"}),
    );
    for n in 1..=2 {
        let meta = send_with(&r).await.unwrap();
        assert_eq!(meta["google_called"]["method"], "gmail.users.messages.send");
        assert_eq!(meta["google_called"]["class"], "authority");
        assert_eq!(r.g.sends.used(), n);
    }
    // The third is refused at the plan, so the owner is never asked for it, and
    // at the run, so a plan made before the cap cannot slip past it.
    let e = plan_of(&r, json!({"method": "gmail.users.messages.send"})).unwrap_err();
    assert!(e.contains("today's send cap is reached (2 of 2)"), "{e}");
    assert!(send_with(&r).await.unwrap_err().contains("send cap"));
    let sent = r
        .fake
        .log()
        .iter()
        .filter(|l| l.path.ends_with("/messages/send"))
        .count();
    assert_eq!(sent, 2);
    // A reads and other writes are not counted.
    assert!(plan_of(
        &r,
        json!({"method": "gmail.users.drafts.create", "body": {}})
    )
    .is_ok());
    // A restart does not reset it.
    let again = super::Google::from_config(&r.core.cfg.google, r.core.secrets.clone()).unwrap();
    again.sends.attach(r.core.store.clone());
    assert_eq!(again.sends.used(), 2);
    assert!(again.sends.allows().is_err());
    assert_eq!(r.g.status().sends_today, 2);
}

#[tokio::test]
async fn no_sends_at_all_when_the_cap_is_zero_and_a_refused_send_is_given_back() {
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![],
        |c| c.google.max_sends_per_day = 0,
    )
    .await;
    let e = plan_of(&r, json!({"method": "gmail.users.messages.send"})).unwrap_err();
    assert!(e.contains("sends are off"), "{e}");
    let r = rig(vec![]).await;
    r.g.check().await;
    // Google refuses the send (400): it did not go, so it is not counted. A
    // server error may have gone out: it stays counted, and says so.
    r.fake.fail_next(1, &Reply::json(400, json!({"error": {"code": 400, "message": "Invalid To header", "errors": [{"reason": "invalidArgument"}]}})));
    let e = send_with(&r).await.unwrap_err();
    assert!(e.contains("Invalid To header"), "{e}");
    assert_eq!(r.g.sends.used(), 0);
    r.fake.fail_next(1, &Reply::server_error());
    let e = send_with(&r).await.unwrap_err();
    assert!(e.contains("outcome unknown"), "{e}");
    assert_eq!(r.g.sends.used(), 1);
    assert_eq!(
        r.fake
            .log()
            .iter()
            .filter(|l| l.path.ends_with("/messages/send"))
            .count(),
        2,
        "each was sent once"
    );
}

#[tokio::test]
async fn a_token_shaped_value_in_an_error_body_is_recorded_as_the_tool_said_it_and_no_minted_token_is(
) {
    let planted = "ya29.planted-by-the-fake-0123456789abcdefghij";
    let r = rig(vec![
        call(
            "c1",
            json!({"method": "drive.files.get", "params": {"fileId": "f1"}}),
        ),
        Scripted::text("Done."),
    ])
    .await;
    r.fake.route("GET", "/drive/v3/files/f1", move |_| {
        Reply::json(403, json!({"error": {"code": 403, "message": format!("denied for {planted}"), "errors": [{"reason": "forbidden"}]}}))
    });
    let sid = session(&r.core);
    turn(&r.core, &sid, "get it").await;
    let all = everything(&r.core, &[&sid]);
    // The record is raw: what Google's error body said is what the tool
    // answered, a token-shaped string in it included (the scrubber guesses no
    // shapes; a value the vault holds is substituted on the way out).
    assert!(all.contains(planted), "the record holds the tool's answer");
    // But not the access token the check minted, nor the refresh token, nor the client's secret.
    for secret in [
        "ya29.fake-access",
        theseus_google::fake::REFRESH_TOKEN,
        "s3cret-client-0123",
    ] {
        assert!(!all.contains(secret), "{secret} reached the record");
    }
    let rows = ledgered(&r.core, "google.called");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["error"], "forbidden");
    let _ = Arc::strong_count(&r.core);
}
