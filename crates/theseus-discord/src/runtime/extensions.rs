//! `/extensions` (M7 43b): the loaded extensions, each with its name,
//! digest, tools, and who acked it and when, and a Revoke button each. A
//! child of `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - **The command** answers the presser alone (ephemeral), from
//!   `extend.list`.
//! - **A Revoke press** is `extension.revoke` as the presser, through the
//!   place (`Control::Revoke`), so the core judges it: only the owner, from a
//!   private place. The presser hears how it went, and the list is shown
//!   again without it.

use theseus_protocol::extend::{
    ExtendListResult, ExtendLoadedInfo, ExtensionRevokeParams, ExtensionRevokeResult,
};
use theseus_protocol::{DiscordOrigin, LedgerKind};
use tokio::sync::{mpsc, oneshot};
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::{AllowedMentions, Component, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::CommandBuilder;

use super::{Control, Place, PlaceMsg, Shared};

/// A Revoke button's `custom_id`: this, then the extension's name.
pub(super) const REVOKE_PREFIX: &str = "ext-revoke:";
/// Discord's most buttons in a message: five rows of five.
const MAX_BUTTONS: usize = 25;

/// `/extensions`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "extensions",
        "The loaded extensions: each one's digest, tools, and who acked it, with Revoke",
        CommandType::ChatInput,
    )
    .build()
}

/// One loaded extension, in lines.
fn lines(e: &ExtendLoadedInfo) -> String {
    let short = &e.digest[..e.digest.len().min(6)];
    let network = match e.network.as_slice() {
        [] => "no network".to_string(),
        hosts => format!("network to {}", hosts.join(", ")),
    };
    let tools: Vec<&str> = e
        .tools
        .iter()
        .map(|t| t.rsplit('/').next().unwrap_or(t))
        .collect();
    let mut out = format!(
        "🧩 **{}** `{short}` · {} · {network}\n  tools: {}\n  acked by {} <t:{}:R>",
        e.name,
        e.state,
        tools.join(", "),
        e.acked_by,
        e.acked_at_ms / 1000
    );
    if let Some(old) = &e.replaced {
        out.push_str(&format!(" · replaced `{}`", &old[..old.len().min(6)]));
    }
    if e.calls > 0 {
        out.push_str(&format!(" · {} calls, {} errors", e.calls, e.errors));
    }
    out
}

/// The list's text.
pub(super) fn text(list: &ExtendListResult) -> String {
    if list.loaded.is_empty() {
        return "No extension is loaded. A proposal the operator acks loads (`theseus extend \
                list` shows the proposals)."
            .into();
    }
    let mut out = vec![format!("**Extensions** · {} loaded", list.loaded.len())];
    out.extend(list.loaded.iter().map(lines));
    out.join("\n")
}

/// A Revoke button for each loaded extension, five to a row.
pub(super) fn buttons(list: &ExtendListResult) -> Vec<Component> {
    let all: Vec<Component> = list
        .loaded
        .iter()
        .take(MAX_BUTTONS)
        .map(|e| {
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{REVOKE_PREFIX}{}", e.name)),
                disabled: false,
                emoji: None,
                label: Some(format!("Revoke {}", e.name)),
                style: ButtonStyle::Danger,
                url: None,
                sku_id: None,
            })
        })
        .collect();
    all.chunks(5)
        .map(|row| {
            Component::ActionRow(ActionRow {
                id: None,
                components: row.to_vec(),
            })
        })
        .collect()
}

/// The extension a Revoke button names.
pub(super) fn revoke_of(custom_id: &str) -> Option<&str> {
    custom_id
        .strip_prefix(REVOKE_PREFIX)
        .filter(|n| !n.is_empty())
}

/// What a revoke said.
fn revoked(name: &str, r: Result<ExtensionRevokeResult, crate::rpc_client::CallError>) -> String {
    match r {
        Ok(r) => format!(
            "🧩 Revoked **{}** `{}`: its server stopped, and its tools are gone from the next \
             turn. The frozen copy stays.",
            r.name,
            &r.digest[..r.digest.len().min(6)]
        ),
        Err(e) => format!("⚠️ {name} was not revoked: {}", e.message),
    }
}

/// The list, as the message's text and its buttons.
fn list_data(list: &ExtendListResult) -> InteractionResponseData {
    InteractionResponseData {
        content: Some(text(list)),
        components: Some(buttons(list)),
        flags: Some(MessageFlags::EPHEMERAL),
        allowed_mentions: Some(AllowedMentions::default()),
        ..Default::default()
    }
}

impl Shared {
    async fn extend_list(&self) -> Result<ExtendListResult, String> {
        self.rpc
            .call::<_, ExtendListResult>(
                theseus_protocol::method::EXTEND_LIST,
                serde_json::Value::Null,
            )
            .await
            .map_err(|e| e.message)
    }

    /// An interaction of `/extensions` or its Revoke button: true when it was
    /// one, and answered.
    pub(super) async fn extensions_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        match &i.data {
            Some(InteractionData::ApplicationCommand(c)) if c.name == "extensions" => {
                self.core.binding_ledger(
                    LedgerKind::DiscordCommand,
                    None,
                    serde_json::json!({"command": c.name, "by": who}),
                );
                // The text through the place, as every command's; the
                // buttons from the same list.
                let said = Self::ask_place(tx, Control::Extensions, who).await;
                let mut data = match self.extend_list().await {
                    Ok(list) => list_data(&list),
                    Err(_) => list_data(&ExtendListResult::default()),
                };
                data.content = Some(said);
                self.extensions_answer(i, InteractionResponseType::ChannelMessageWithSource, data)
                    .await;
                true
            }
            Some(InteractionData::MessageComponent(c)) => {
                let Some(name) = revoke_of(&c.custom_id) else {
                    return false;
                };
                // The press goes through the place, as the presser.
                let said =
                    Self::ask_place(tx, Control::Revoke(name.to_string(), discord), who).await;
                // The list again, with what the press did above it.
                let mut data = match self.extend_list().await {
                    Ok(list) => list_data(&list),
                    Err(_) => InteractionResponseData::default(),
                };
                data.content = Some(format!("{said}\n\n{}", data.content.unwrap_or_default()));
                self.extensions_answer(i, InteractionResponseType::UpdateMessage, data)
                    .await;
                true
            }
            _ => false,
        }
    }

    /// A control through the place's own line, and what it said.
    async fn ask_place(tx: &mpsc::UnboundedSender<PlaceMsg>, cmd: Control, who: &str) -> String {
        let (rtx, rrx) = oneshot::channel();
        let _ = tx.send(PlaceMsg::Control {
            cmd,
            by: who.to_string(),
            reply: rtx,
        });
        rrx.await
            .unwrap_or_else(|_| "The place did not answer.".into())
    }

    async fn extensions_answer(
        &self,
        i: &Interaction,
        kind: InteractionResponseType,
        data: InteractionResponseData,
    ) {
        let resp = InteractionResponse {
            kind,
            data: Some(data),
        };
        if let Err(e) = self
            .http
            .interaction(i.application_id)
            .create_response(i.id, &i.token, &resp)
            .await
        {
            self.board.error("extensions response", None, e);
        }
    }
}

impl Place {
    /// `/extensions`'s text, from `extend.list`.
    pub(super) async fn extensions(&self) -> String {
        match self.shared.extend_list().await {
            Ok(list) => text(&list),
            Err(e) => format!("⚠️ {e}"),
        }
    }

    /// A Revoke press: `extension.revoke` as the presser, so the core judges
    /// it (the owner, from a private place).
    pub(super) async fn revoke(
        &self,
        name: String,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, ExtensionRevokeResult>(
                theseus_protocol::method::EXTENSION_REVOKE,
                ExtensionRevokeParams {
                    name: name.clone(),
                    author: Some(by.to_string()),
                    discord: origin,
                },
            )
            .await;
        revoked(&name, r)
    }
}

#[cfg(test)]
mod tests {
    use theseus_protocol::extend::{ExtendListResult, ExtendLoadedInfo};
    use theseus_protocol::DiscordOrigin;

    use super::super::tests::{core_with, place_for_tests, OWNER};
    use super::super::Control;
    use super::{buttons, revoke_of, text};

    fn loaded(name: &str) -> ExtendLoadedInfo {
        ExtendLoadedInfo {
            name: name.into(),
            server: format!("ext-{name}"),
            digest: "3f2a1c9e".into(),
            tools: vec![format!("mcp:ext-{name}/count")],
            acked_by: "cli".into(),
            acked_via: "cli".into(),
            acked_at_ms: 1_700_000_000_000,
            state: "ready".into(),
            ..Default::default()
        }
    }

    /// The list names each loaded extension with its digest, tools, and who
    /// acked it and when, and gives each a Revoke button, five to a row.
    #[test]
    fn the_list_says_each_loaded_extension_with_a_revoke_button() {
        let list = ExtendListResult {
            loaded: (0..7).map(|n| loaded(&format!("wc{n}"))).collect(),
            ..Default::default()
        };
        let t = text(&list);
        assert!(t.starts_with("**Extensions** · 7 loaded\n"), "{t}");
        assert!(
            t.contains("🧩 **wc0** `3f2a1c` · ready · no network\n  tools: count\n  acked by cli <t:1700000000:R>"),
            "{t}"
        );
        let rows = buttons(&list);
        assert_eq!(rows.len(), 2);
        let ids: Vec<String> = rows
            .iter()
            .flat_map(|r| match r {
                twilight_model::channel::message::Component::ActionRow(a) => a.components.clone(),
                _ => vec![],
            })
            .filter_map(|c| match c {
                twilight_model::channel::message::Component::Button(b) => b.custom_id,
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 7);
        assert_eq!(revoke_of(&ids[0]), Some("wc0"));
        assert_eq!(revoke_of("confirm:approve:act_1"), None);
        assert_eq!(revoke_of("ext-revoke:"), None);
        assert!(text(&ExtendListResult::default()).starts_with("No extension is loaded."));
    }

    /// `/extensions` and a Revoke press, through the place: the list from
    /// the core; a press from a shared guild channel is refused by the core,
    /// with why, and it stays loaded; the owner's press from their DM
    /// revokes it.
    #[tokio::test]
    async fn revoke_goes_to_the_core_as_the_presser() {
        const LAB: u64 = 314_159_265_358_979_323;
        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
            c.discord.rest_proxy = Some("127.0.0.1:9".into());
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
        });
        // A loaded extension, as an ack records it.
        core.store
            .put_meta(
                theseus_core::extend::load::RECORD,
                &serde_json::json!({"loaded": {"wordcount": {
                    "name": "wordcount", "digest": "3f2a1c9e", "description": "Counts words.",
                    "command": ["sh", "server.sh"], "frozen": "/state/extensions/wordcount/3f2a1c9e",
                    "source": "/work/wc", "tools": ["count"],
                    "capabilities": {"network": [], "scratch": true},
                    "acked_by": "cli", "acked_via": "cli", "acked_at_ms": 1_700_000_000_000u64,
                    "question": "act_q1",
                    "proposed_by": {"session_id": "ses_1", "execution_id": "exe_1",
                                    "correlation_id": "act_p1", "principal": "operator"}
                }}}),
            )
            .unwrap();
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let (mut place, _rx) = place_for_tests(&core, &sid);
        let listed = place
            .control(Control::Extensions, "discord:zeroaltitude")
            .await;
        assert!(listed.contains("🧩 **wordcount** `3f2a1c`"), "{listed}");
        let from = |guild: Option<&str>, channel: u64| {
            Some(DiscordOrigin {
                user_id: OWNER.to_string(),
                channel_id: channel.to_string(),
                guild_id: guild.map(str::to_string),
            })
        };
        let shared = place
            .control(
                Control::Revoke("wordcount".into(), from(Some("900000000000000001"), LAB)),
                "discord:zeroaltitude",
            )
            .await;
        assert!(
            shared.starts_with("⚠️ wordcount was not revoked: "),
            "{shared}"
        );
        assert!(shared.contains("shared"), "{shared}");
        let still = place
            .control(Control::Extensions, "discord:zeroaltitude")
            .await;
        assert!(still.contains("**wordcount**"), "{still}");
        let dm = place
            .control(
                Control::Revoke("wordcount".into(), from(None, OWNER + 1)),
                "discord:zeroaltitude",
            )
            .await;
        assert!(dm.starts_with("🧩 Revoked **wordcount** `3f2a1c`"), "{dm}");
        let gone = place
            .control(Control::Extensions, "discord:zeroaltitude")
            .await;
        assert!(gone.starts_with("No extension is loaded."), "{gone}");
    }
}
