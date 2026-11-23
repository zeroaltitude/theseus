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
