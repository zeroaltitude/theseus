//! A Google write's own result is not outside text (theseus-e31an): the
//! walkthrough's DM session was held for three days by the echo of a draft
//! Theseus itself created. A created draft's id leaves the session unheld;
//! an update's echo of someone else's object still holds.

use serde_json::json;

use super::tests::*;
use crate::policy::Posture;
use crate::provider::Scripted;
use crate::session::SessionRecord;
use crate::test_support::turn;

fn open(c: &mut crate::Config) {
    c.policy.enforcement = Posture::Open;
    c.policy.tools.insert("google.call".into(), Posture::Open);
}

#[tokio::test]
async fn a_created_drafts_result_leaves_the_session_unheld() {
    let draft = json!({"method": "gmail.users.drafts.create",
        "body": {"message": {"raw": "VG86IG93bmVyQGV4YW1wbGUudGVzdA"}}});
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![call("c1", draft), Scripted::text("Drafted.")],
        open,
    )
    .await;
    r.fake.json(
        "POST",
        "/gmail/v1/users/me/drafts",
        json!({"id": "r-123", "message": {"id": "m1", "threadId": "t1", "labelIds": ["DRAFT"]}}),
    );
    let sid = session(&r.core);
    let res = turn(&r.core, &sid, "draft a note to myself").await;
    assert!(res.awaiting_confirm.is_none(), "{res:?}");
    assert!(
        r.fake.log().iter().any(|l| l.path.ends_with("/drafts")),
        "the draft was created"
    );
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert!(rec.external.is_none(), "{:?}", rec.external);
    assert!(ledgered(&r.core, "session.external_read").is_empty());
}

#[tokio::test]
async fn an_updates_echo_still_holds() {
    let patch = json!({"method": "calendar.events.patch",
        "params": {"calendarId": "primary", "eventId": "ev1"},
        "body": {"colorId": "5"}});
    let r = rig_with(
        Some(theseus_google::fake::REFRESH_TOKEN),
        |_| "file:/tmp/unused-google-refresh-token".into(),
        vec![call("c1", patch), Scripted::text("Done.")],
        open,
    )
    .await;
    r.fake.json(
        "PATCH",
        "/calendar/v3/calendars/primary/events/ev1",
        json!({"id": "ev1", "description": "a stranger's agenda"}),
    );
    let sid = session(&r.core);
    let _ = turn(&r.core, &sid, "color it").await;
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    assert!(
        rec.external.is_some(),
        "an update's echo is someone's object"
    );
}
