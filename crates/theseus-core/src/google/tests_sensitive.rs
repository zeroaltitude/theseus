//! What a sensitive calendar leaves beside its label (theseus-jid7t.1,
//! theseus-gyjyo.5): the long tail fails closed to `personal`, a node of a
//! session that carries it never goes to a shared place, and the calendar
//! tools are not offered in a shared place at all.

use serde_json::json;
use theseus_google::fake::Fake;

use super::tests_calendar_cache::{ev, install, two, NOON};
use super::tests_label::google_on;
use super::tests_label::{new_session, rig_tweaked, run_on};
use crate::content::Flag;
use crate::provider::Scripted;
use crate::session::SessionRecord;

/// A Calendar read through `google.call` raises `personal` when a
/// configured calendar is sensitive: the long tail cannot say which calendar it read, so
/// it fails closed (the join fix J1 of theseus-jid7t.1).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_calendar_read_through_google_call_is_sensitive() {
    let fake = Fake::start().await;
    let cals = install(&fake);
    cals.work.lock().unwrap().items = vec![ev("w1", "Board prep", NOON.0, NOON.1)];
    let input = json!({
        "method": "calendar.events.list",
        "params": {"calendarId": super::tests_calendar_cache::WORK},
    });
    let (r, _) = rig_tweaked(
        None,
        &fake,
        vec![
            Scripted::tools("", &[("c1", "google_call", input)]),
            Scripted::text("Done."),
        ],
        false,
        two,
    );
    let sid = new_session(&r.core);
    run_on(&r.core, &sid, "what is on my work calendar?", None)
        .await
        .unwrap();
    let rec: SessionRecord = r.core.store.get_session(&sid).unwrap().unwrap();
    let f = rec.data.map(|b| *b).unwrap_or_default();
    assert_eq!(f.labels.len(), 1, "{:?}", f.labels);
    assert_eq!(f.labels[0].flag, Flag::Personal);
    assert_eq!(f.labels[0].source, "google.call");
}

/// Content of a session that carries `personal` never goes to a shared place,
/// whoever asks; a session that carries only `confidential` publishes as
/// before, and so does a personal one into a private place.
#[tokio::test]
async fn a_sensitive_node_is_not_published_to_a_shared_place() {
    use crate::places::BoundPlace;
    use crate::tests_places::{rig_with, session as place_session, LAB};
    let r = rig_with(google_on);
    r.core.bind_places(vec![
        BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        },
        BoundPlace {
            target: "discord:channel:424242".into(),
            name: "#den".into(),
            private: true,
            ..Default::default()
        },
    ]);
    for place in [format!("channel:{LAB}"), "channel:424242".to_string()] {
        let sid = place_session(&r.core, Some(&place));
        crate::tests_places::turn(&r.core, &sid, "hello").await;
    }
    let node_in = |sid: &str, text: &str| {
        let n = crate::node::Node::user(sid, None, "test", text);
        r.core.store.append(&[n.record().unwrap()]).unwrap();
        n.id
    };
    let (sens, privy, plain) = (
        new_session(&r.core),
        new_session(&r.core),
        new_session(&r.core),
    );
    crate::test_support::label_session(&r.core, &sens, Flag::Personal);
    crate::test_support::label_session(&r.core, &privy, Flag::Confidential);
    let publish = |id: &str, to: &str| {
        r.core.publish(
            &theseus_protocol::PlacePublishParams {
                node_id: Some(id.into()),
                to: to.into(),
                ..Default::default()
            },
            "cli",
        )
    };
    let id = node_in(&sens, "the Board prep meeting is moved");
    let e = publish(&id, "#lab").unwrap_err().to_string();
    assert!(
        e.contains("personal content never goes to a shared place"),
        "{e}"
    );
    assert!(!e.contains("Board prep"), "{e}");
    assert!(crate::test_support::rows(&r.core, "place.published").is_empty());
    let den = publish(&id, "#den");
    assert!(den.is_ok(), "a private place may have it: {den:?}");
    assert!(publish(&node_in(&privy, "private only"), "#lab").is_ok());
    assert!(publish(&node_in(&plain, "plain"), "#lab").is_ok());
}

/// The calendar tools are not offered in a shared place at all, so a Discord
/// channel that others can read never reads a calendar.
#[test]
fn a_shared_place_is_offered_no_calendar_tool() {
    use crate::places::{offered, refusal};
    use theseus_protocol::{PlaceClass, Plan};
    for name in ["calendar.events", "calendar.put", "google.call"] {
        assert!(!offered(PlaceClass::Shared, name, &[]), "{name}");
        let why = refusal(PlaceClass::Shared, name, &Plan::default(), &[]).unwrap();
        assert!(why.contains("not offered in a shared place"), "{why}");
        assert!(offered(PlaceClass::Private, name, &[]));
    }
}
