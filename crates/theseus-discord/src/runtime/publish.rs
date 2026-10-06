//! `/publish` (the place rule, theseus-nbsh): its options, and the place's
//! handler, which goes through `place.publish` as the presser. A child of
//! `runtime`, apart from it for the shape budget's file ceiling.

use theseus_protocol::DiscordOrigin;

use super::Place;

/// `/publish`'s options, and who asked (the place rule).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PublishAsk {
    /// `discord:channel:<id>`.
    pub(super) to: String,
    pub(super) node: Option<String>,
    pub(super) file: Option<String>,
    pub(super) text: Option<String>,
    pub(super) note: Option<String>,
    pub(super) origin: Option<DiscordOrigin>,
}

impl PublishAsk {
    pub(super) fn of(
        options: &[twilight_model::application::interaction::application_command::CommandDataOption],
        origin: Option<DiscordOrigin>,
    ) -> Self {
        use twilight_model::application::interaction::application_command::CommandOptionValue as V;
        let mut ask = PublishAsk {
            to: String::new(),
            node: None,
            file: None,
            text: None,
            note: None,
            origin,
        };
        for o in options {
            match (o.name.as_str(), &o.value) {
                ("to", V::Channel(c)) => ask.to = format!("discord:channel:{c}"),
                ("to", V::String(s)) => ask.to = s.clone(),
                ("node", V::String(s)) => ask.node = Some(s.clone()),
                ("file", V::String(s)) => ask.file = Some(s.clone()),
                ("text", V::String(s)) => ask.text = Some(s.clone()),
                ("note", V::String(s)) => ask.note = Some(s.clone()),
                _ => {}
            }
        }
        ask
    }
}

impl Place {
    /// `/publish` (the place rule): through `place.publish` as the presser,
    /// so the core judges it: only the owner, from a private place.
    pub(super) async fn publish(&self, ask: PublishAsk, by: &str) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, theseus_protocol::PublishResult>(
                theseus_protocol::method::PLACE_PUBLISH,
                theseus_protocol::PlacePublishParams {
                    node_id: ask.node,
                    path: ask.file,
                    text: ask.text,
                    to: ask.to,
                    note: ask.note,
                    author: Some(by.to_string()),
                    discord: ask.origin,
                },
            )
            .await;
        match r {
            Ok(r) => format!(
                "📎 Published {} into {} ({} bytes).",
                r.what, r.name, r.bytes
            ),
            Err(e) => format!("⚠️ Not published: {}", e.message),
        }
    }
}

#[cfg(test)]
mod tests {
    use theseus_protocol::DiscordOrigin;
    use twilight_model::id::Id;

    use super::super::tests::{core_with, place_for_tests, OWNER};
    use super::super::Control;
    use super::PublishAsk;

    /// `/publish` (the place rule): its options become `place.publish`'s, as
    /// the presser; from a guild channel that is shared, the core refuses it
    /// with the reason, and nothing is published.
    #[tokio::test]
    async fn publish_goes_to_the_core_as_the_presser() {
        use twilight_model::application::interaction::application_command::{
            CommandDataOption, CommandOptionValue,
        };
        const LAB: u64 = 314_159_265_358_979_323;
        let origin = Some(DiscordOrigin {
            user_id: OWNER.to_string(),
            channel_id: LAB.to_string(),
            guild_id: Some("900000000000000001".into()),
        });
        let opt = |name: &str, value: CommandOptionValue| CommandDataOption {
            name: name.into(),
            value,
        };
        let ask = PublishAsk::of(
            &[
                opt("to", CommandOptionValue::Channel(Id::new(LAB))),
                opt(
                    "text",
                    CommandOptionValue::String("the tide turns at six".into()),
                ),
                opt("note", CommandOptionValue::String("for everyone".into())),
            ],
            origin.clone(),
        );
        assert_eq!(
            (
                ask.to.as_str(),
                ask.text.as_deref(),
                ask.note.as_deref(),
                ask.node.as_deref()
            ),
            (
                format!("discord:channel:{LAB}").as_str(),
                Some("the tide turns at six"),
                Some("for everyone"),
                None
            )
        );
        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
            c.discord.rest_proxy = Some("127.0.0.1:9".into());
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
        });
        core.bind_places(vec![theseus_core::places::BoundPlace {
            target: format!("discord:channel:{LAB}"),
            name: "#lab".into(),
            private: false,
            ..Default::default()
        }]);
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        core.outbox
            .bind_place(&format!("channel:{LAB}"), &sid)
            .unwrap();
        let (mut place, _rx) = place_for_tests(&core, &sid);
        let answer = place
            .control(Control::Publish(Box::new(ask)), "discord:zeroaltitude")
            .await;
        assert!(answer.starts_with("⚠️ Not published: "), "{answer}");
        let published = core
            .store
            .ledger_tail::<theseus_core::ledger::LedgerRow>(1_000)
            .unwrap()
            .into_iter()
            .filter(|(_, r)| r.kind == "place.published")
            .count();
        assert_eq!(published, 0);
    }
}
