//! The service wants an interaction's first response within three seconds (issue 29). A handler that waits for the
//! daemon before it responds can miss that, and the person sees "Something went wrong" whatever it then says: the
//! owner met it on the keep form. These tests hold a daemon call open, with no sleep, and read what the binding has
//! sent to the service by then: the acknowledgement, and nothing else; then they let the call go and read the same
//! response completed.

use serde_json::json;
use theseus_protocol::method;
use twilight_model::application::interaction::Interaction;

use super::shared_for_tests;
use super::tests::{core_with, OWNER};

const DM: u64 = 444_444_444_444_444_444;

fn press(custom_id: &str) -> Interaction {
    let v = json!({
        "id": "1000000000000000021", "application_id": "1000000000000000002", "type": 3,
        "token": "not-a-token", "version": 1, "authorizing_integration_owners": {},
        "channel": {"id": DM.to_string(), "type": 1},
        "user": {"id": OWNER.to_string(), "username": "dockhand", "discriminator": "0", "avatar": null},
        "data": {"custom_id": custom_id, "component_type": 2},
    });
    serde_json::from_value(v).unwrap()
}

/// A press that is answered by the daemon (an `ask` card's option, here) is acknowledged before the daemon is
/// asked, and the same response is completed after. The daemon's call is held open.
#[tokio::test]
async fn a_handler_that_waits_for_the_daemon_acknowledges_first_and_completes_after() {
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    let d = tempfile::tempdir().unwrap();
    let addr = fake.addr.clone();
    let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
        c.discord.rest_proxy = Some(addr);
    });
    let shared = shared_for_tests(&core);
    let (dm_tx, _dm_rx) = tokio::sync::mpsc::unbounded_channel();
    shared
        .routes
        .lock()
        .unwrap()
        .by_dm_user
        .insert(OWNER, dm_tx);
    let held = crate::rpc_client::hold(method::WORK_ANSWER);
    let handler = tokio::spawn(shared.clone().on_interaction(press("ask:1:act_0000feed")));
    held.reached.notified().await;
    // The daemon has not answered, and the press is already acknowledged: a deferred update of the pressed
    // message, carrying nothing.
    let so_far = fake.replies();
    assert_eq!(so_far.len(), 1, "{so_far:?}");
    assert_eq!(
        (
            so_far[0].kind.as_str(),
            so_far[0].response_type,
            so_far[0].content.as_deref()
        ),
        ("callback", Some(6), None),
        "{so_far:?}"
    );
    held.release.notify_one();
    handler.await.unwrap();
    // The daemon refused (no such question): told to the presser alone, after the acknowledgement.
    let all = fake.replies();
    assert_eq!(all.len(), 2, "{all:?}");
    assert_eq!(all[1].kind, "followup", "{all:?}");
    assert!(all[1].ephemeral, "{all:?}");
}
