//! Exit regressions share only their fake transport and known vault value.
use super::*;
pub(super) const VALUE: &str = "vault-token-4c8e1f7a92b3";
pub(super) fn rig() -> (
    tempfile::TempDir,
    Arc<Core>,
    Arc<theseus_sim::fake_discord::FakeDiscord>,
    Arc<Shared>,
) {
    let dir = tempfile::tempdir().unwrap();
    let fake = theseus_sim::fake_discord::FakeDiscord::start();
    fake.set_guild(
        theseus_sim::fake_discord::Guild::new(
            theseus_sim::fake_discord::DEFAULT_GUILD,
            (7, "keeper"),
        )
        .channel(42, "reef"),
    );
    let core = tests::core_with(
        dir.path(),
        theseus_core::secrets::SecretBoard::empty(),
        |c| {
            c.discord.rest_proxy = Some(fake.addr.clone());
            c.places.owner = Some(vec!["discord:7".into()]);
        },
    );
    core.secrets.hold(
        "display_token",
        theseus_core::secrets::Secret::new(VALUE.into()),
    );
    let shared = shared_for_tests(&core);
    shared.place_bits.replace(&crate::bindings::Bindings::parse(&format!("guild_id = \"{}\"\nprivate = true\n[[channel]]\nid = \"223456789012345678\"\nname = \"reef\"\nusers = [\"323456789012345678\"]\n", theseus_sim::fake_discord::DEFAULT_GUILD)).unwrap());
    core.bind_places(vec![theseus_core::places::BoundPlace {
        target: "discord:channel:42".into(),
        name: "reef".into(),
        private: true,
        ..Default::default()
    }]);
    for id in ["ses_exe_reef", "ses_parent"] {
        let rec = theseus_core::session::SessionRecord::with_id(
            id.into(),
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        core.store.put_session(id, &rec).unwrap();
    }
    (dir, core, fake, shared)
}
pub(super) fn interaction() -> twilight_model::application::interaction::Interaction {
    serde_json::from_value(serde_json::json!({"id":"1", "application_id":"2", "token":"fake", "type":2, "data":{"id":"3", "name":"test", "type":1}, "version":1, "entitlements":[], "authorizing_integration_owners":{}, "guild_id":"900000000000000001", "channel":{"id":"42","type":0}, "user":{"id":"7","username":"keeper","discriminator":"0","avatar":null}})).unwrap()
}
pub(super) fn view() -> theseus_protocol::work::WorkView {
    super::task_channels::work_view("exe_reef", "running")
}
pub(super) fn parent() -> super::threads::Parent {
    super::threads::Parent {
        channel: 42,
        threads: true,
        key: "channel:42".into(),
        target: "discord:channel:42".into(),
        label: "reef".into(),
        session: "ses_parent".into(),
        users: vec![7],
        show: Default::default(),
        homed: false,
        tx: tokio::sync::mpsc::unbounded_channel().0,
    }
}

#[tokio::test]
async fn confirm_settle_masks_error_words_r4() {
    let (_dir, _core, fake, shared) = rig();
    let (tx, _rx) = mpsc::unbounded_channel();
    {
        let mut routes = shared.routes.lock().unwrap();
        routes.by_channel.insert(42, tx);
        routes.users.insert(42, vec![7]);
    }
    let m = shared
        .http
        .create_message(Id::new(42))
        .content("Question")
        .await
        .unwrap()
        .model()
        .await
        .unwrap();
    let mut i = interaction();
    i.kind = twilight_model::application::interaction::InteractionType::MessageComponent;
    i.data = Some(
        serde_json::from_value(
            json!({"custom_id":format!("confirm:decline:{VALUE}"), "component_type":2}),
        )
        .map(InteractionData::MessageComponent)
        .unwrap(),
    );
    i.message = Some(m);
    shared.on_interaction(i).await;
    let text = fake.messages(42).last().unwrap().content.clone();
    assert!(text.contains("Could not answer"), "{text}");
    assert!(!text.contains(VALUE), "{text}");
    assert!(text.contains("[redacted:"), "{text}");
}

#[tokio::test]
async fn cancel_command_masks_note_before_cut_r4() {
    let (_dir, core, _fake, _shared) = rig();
    let (mut place, _rx) = tests::place_for_tests(&core, "ses_parent");
    let wake = theseus_protocol::WakeInfo {
        note: format!("{}{}", "x".repeat(195), VALUE),
        ..Default::default()
    };
    let hold = crate::rpc_client::slow_once(
        theseus_protocol::method::WAKE_CANCEL,
        "exit_command",
        json!({"wake":wake}),
    );
    hold.release.notify_one();
    let text = place
        .control(Control::Cancel(Some("exit_command".into())), "keeper")
        .await;
    assert!(!text.contains("vau"), "{text}");
    assert!(text.contains("[red"), "{text}");
}

#[tokio::test]
async fn wakes_mask_note_before_cut_r5() {
    let (_dir, core, _fake, _shared) = rig();
    let (mut place, _rx) = tests::place_for_tests(&core, "ses_parent");
    let wake = theseus_protocol::WakeInfo {
        note: format!("{}{}", "x".repeat(115), VALUE),
        ..Default::default()
    };
    let hold = crate::rpc_client::slow_once(
        theseus_protocol::method::WAKE_LIST,
        "",
        json!({"wakes":[wake]}),
    );
    hold.release.notify_one();
    let text = place.control(Control::Wakes, "keeper").await;
    assert!(text.contains("Wakes here"), "{text}");
    assert!(!text.contains("vau"), "{text}");
    assert!(text.contains("[red"), "{text}");
}
