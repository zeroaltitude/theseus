//! `/orders` (theseus-a7vi): the owner's standing orders, preferences and
//! facts from `standing.list`, ephemeral, each with a Revoke button. A child
//! of `runtime`, apart from it for the shape budget's file ceiling.
//!
//! - **The command** answers the presser alone. The list is read as the
//!   presser, so the core gives the text of a preference or a fact only to
//!   the owner in a private place; anyone else reads `(private)`.
//! - **A Revoke press** is `standing.revoke` as the presser, through the
//!   place (`Control::Unorder`), so the core judges it: only the owner, from
//!   a private place. The presser hears how it went, and the list is shown
//!   again without it.

use theseus_protocol::standing::{
    StandingChanged, StandingListParams, StandingListResult, StandingRevokeParams,
};
use theseus_protocol::{DiscordOrigin, LedgerKind};
use tokio::sync::mpsc;
use twilight_model::application::command::{Command, CommandType};
use twilight_model::application::interaction::{Interaction, InteractionData};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::{AllowedMentions, Component, MessageFlags};
use twilight_model::http::interaction::{
    InteractionResponse, InteractionResponseData, InteractionResponseType,
};
use twilight_util::builder::command::CommandBuilder;

use super::{Control, Place, PlaceMsg, Shared};

/// A Revoke button's `custom_id`: this, then the record's id.
pub(super) const REVOKE_PREFIX: &str = "orders-revoke:";
/// Discord's most buttons in a message: five rows of five.
const MAX_BUTTONS: usize = 25;
/// What a message's text may hold, with room for the lines after it.
const MAX_TEXT: usize = 1800;

/// `/orders`.
pub(super) fn command() -> Command {
    CommandBuilder::new(
        "orders",
        "Your standing orders, preferences and facts, kept in every prompt of their scope, with Revoke",
        CommandType::ChatInput,
    )
    .build()
}

/// The list's text: a line a record, then the manifests' tally.
pub(super) fn text(list: &StandingListResult) -> String {
    if list.records.is_empty() {
        return "No standing records. `theseus standing add \"never push to main\"` makes an order."
            .into();
    }
    let mut out = format!("**Standing** · {} live", list.records.len());
    let mut shown = 0;
    for i in &list.records {
        let r = &i.record;
        let line = format!(
            "\n{} **{}** `{}` · {} · <t:{}:R> · {}",
            match r.kind.name() {
                "order" => "📌",
                "preference" => "🎛️",
                _ => "🗒️",
            },
            r.kind.name(),
            r.id,
            r.scope.words(),
            r.since_ms / 1000,
            r.text
        );
        if out.len() + line.len() > MAX_TEXT {
            break;
        }
        out.push_str(&line);
        shown += 1;
    }
    if shown < list.records.len() {
        out.push_str(&format!(
            "\n…and {} more: `theseus standing` lists them all.",
            list.records.len() - shown
        ));
    }
    out.push_str(&format!(
        "\nin every compile of their scope since they began: {} compiles, {} missing",
        list.compiles, list.missing
    ));
    out
}

/// A Revoke button for each record, five to a row.
pub(super) fn buttons(list: &StandingListResult) -> Vec<Component> {
    let all: Vec<Component> = list
        .records
        .iter()
        .take(MAX_BUTTONS)
        .map(|i| {
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{REVOKE_PREFIX}{}", i.record.id)),
                disabled: false,
                emoji: None,
                label: Some(format!("Revoke {}", i.record.id)),
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

/// The record a Revoke button names.
pub(super) fn revoke_of(custom_id: &str) -> Option<&str> {
    custom_id
        .strip_prefix(REVOKE_PREFIX)
        .filter(|n| !n.is_empty())
}

/// What a revoke said.
fn revoked(id: &str, r: Result<StandingChanged, crate::rpc_client::CallError>) -> String {
    match r {
        Ok(r) => format!(
            "📌 Revoked `{}`: it leaves every prompt of its scope at the next turn.",
            r.ended.as_deref().unwrap_or(id)
        ),
        Err(e) => format!("⚠️ {id} was not revoked: {}", e.message),
    }
}

fn list_data(list: &StandingListResult) -> InteractionResponseData {
    InteractionResponseData {
        content: Some(text(list)),
        components: Some(buttons(list)),
        flags: Some(MessageFlags::EPHEMERAL),
        allowed_mentions: Some(AllowedMentions::default()),
        ..Default::default()
    }
}

impl Shared {
    /// The records, as the presser.
    async fn orders_list(
        &self,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> Result<StandingListResult, String> {
        self.rpc
            .call::<_, StandingListResult>(
                theseus_protocol::method::STANDING_LIST,
                StandingListParams {
                    author: Some(who.to_string()),
                    discord,
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| e.message)
    }

    /// An interaction of `/orders`, `/thread` or `/extensions` or a Revoke
    /// button of either: true when it was one, and answered. One call from
    /// `on_interaction`, which clippy holds to its complexity.
    pub(super) async fn revokable_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        self.orders_interaction(i, tx, who, discord.clone()).await
            || self.thread_interaction(i, who, discord.clone()).await
            || self.extensions_interaction(i, tx, who, discord).await
    }

    /// An interaction of `/orders` or its Revoke button: true when it was
    /// one, and answered.
    pub(super) async fn orders_interaction(
        &self,
        i: &Interaction,
        tx: &mpsc::UnboundedSender<PlaceMsg>,
        who: &str,
        discord: Option<DiscordOrigin>,
    ) -> bool {
        match &i.data {
            Some(InteractionData::ApplicationCommand(c)) if c.name == "orders" => {
                self.core.binding_ledger(
                    LedgerKind::DiscordCommand,
                    None,
                    serde_json::json!({"command": c.name, "by": who}),
                );
                let data = match self.orders_list(who, discord).await {
                    Ok(list) => list_data(&list),
                    Err(e) => InteractionResponseData {
                        content: Some(format!("⚠️ {e}")),
                        flags: Some(MessageFlags::EPHEMERAL),
                        ..Default::default()
                    },
                };
                self.orders_answer(i, InteractionResponseType::ChannelMessageWithSource, data)
                    .await;
                true
            }
            Some(InteractionData::MessageComponent(c)) => {
                let Some(id) = revoke_of(&c.custom_id) else {
                    return false;
                };
                // The press goes through the place, as the presser.
                let said =
                    Self::ask_place(tx, Control::Unorder(id.to_string(), discord.clone()), who)
                        .await;
                let mut data = match self.orders_list(who, discord).await {
                    Ok(list) => list_data(&list),
                    Err(_) => InteractionResponseData::default(),
                };
                data.content = Some(format!("{said}\n\n{}", data.content.unwrap_or_default()));
                self.orders_answer(i, InteractionResponseType::UpdateMessage, data)
                    .await;
                true
            }
            _ => false,
        }
    }

    async fn orders_answer(
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
            self.board.error("orders response", None, e);
        }
    }
}

impl Place {
    /// A Revoke press: `standing.revoke` as the presser, so the core judges
    /// it (the owner, from a private place).
    pub(super) async fn unorder(
        &self,
        id: String,
        origin: Option<DiscordOrigin>,
        by: &str,
    ) -> String {
        let r = self
            .shared
            .rpc
            .call::<_, StandingChanged>(
                theseus_protocol::method::STANDING_REVOKE,
                StandingRevokeParams {
                    id: id.clone(),
                    author: Some(by.to_string()),
                    discord: origin,
                },
            )
            .await;
        revoked(&id, r)
    }
}

#[cfg(test)]
mod tests {
    use theseus_protocol::standing::{
        StandingInfo, StandingKind, StandingListResult, StandingRecord, StandingScope,
    };
    use theseus_protocol::DiscordOrigin;
    use twilight_model::channel::message::Component;

    use super::super::tests::{core_with, place_for_tests, OWNER};
    use super::super::Control;
    use super::{buttons, revoke_of, text};

    fn cli() -> theseus_core::approval::Answerer {
        theseus_core::approval::Answerer {
            label: "the CLI".into(),
            surface: theseus_core::approval::Surface::Cli,
            discord: None,
        }
    }

    fn info(id: &str, kind: StandingKind, text: &str) -> StandingInfo {
        StandingInfo {
            record: StandingRecord {
                id: id.into(),
                kind,
                text: text.into(),
                scope: StandingScope::Everywhere,
                since_ms: 1_790_000_000_000,
                until_ms: None,
                source: "the CLI".into(),
                ends: None,
            },
            state: "live".into(),
            compiles: 4,
            missing: 0,
            repeats: 0,
            captured_by: String::new(),
        }
    }

    /// The list names each record with its kind, id, scope and text, ends with
    /// the manifests' tally, and gives each a Revoke button, five to a row.
    #[test]
    fn the_list_says_each_record_with_a_revoke_button() {
        let list = StandingListResult {
            records: (0..7)
                .map(|n| {
                    info(
                        &format!("std_0000000{n}"),
                        StandingKind::Order,
                        "never push to main",
                    )
                })
                .collect(),
            compiles: 4,
            missing: 0,
            asks: vec![],
        };
        let t = text(&list);
        assert!(t.starts_with("**Standing** · 7 live\n"), "{t}");
        assert!(
            t.contains(
                "📌 **order** `std_00000000` · everywhere · <t:1790000000:R> · never push to main"
            ),
            "{t}"
        );
        assert!(
            t.ends_with("in every compile of their scope since they began: 4 compiles, 0 missing"),
            "{t}"
        );
        let rows = buttons(&list);
        assert_eq!(rows.len(), 2);
        let ids: Vec<String> = rows
            .iter()
            .flat_map(|r| match r {
                Component::ActionRow(a) => a.components.clone(),
                _ => vec![],
            })
            .filter_map(|c| match c {
                Component::Button(b) => b.custom_id,
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 7);
        assert_eq!(revoke_of(&ids[0]), Some("std_00000000"));
        assert_eq!(revoke_of("ext-revoke:wc"), None);
        assert_eq!(revoke_of("orders-revoke:"), None);
        assert!(text(&StandingListResult {
            records: vec![],
            compiles: 0,
            missing: 0,
            asks: vec![]
        })
        .starts_with("No standing records."));
    }

    /// A Revoke press through the place, as the presser: from a shared guild
    /// channel the core refuses it, with why, and the record stays; the
    /// owner's press from their DM revokes it.
    #[tokio::test]
    async fn a_press_revokes_as_the_presser_and_the_core_judges_it() {
        const LAB: u64 = 314_159_265_358_979_323;
        let d = tempfile::tempdir().unwrap();
        let core = core_with(d.path(), theseus_core::secrets::SecretBoard::empty(), |c| {
            c.discord.rest_proxy = Some("127.0.0.1:9".into());
            c.places.owner = Some(vec![format!("discord:{OWNER}")]);
        });
        let id = core
            .standing_add(
                &theseus_protocol::StandingAddParams {
                    text: "never push to main".into(),
                    ..Default::default()
                },
                cli(),
            )
            .unwrap()
            .record
            .id;
        let rec = theseus_core::session::SessionRecord::new(
            theseus_protocol::SessionKind::Conversation,
            None,
        );
        let sid = rec.session_id.clone();
        core.store.put_session(&sid, &rec).unwrap();
        let (mut place, _rx) = place_for_tests(&core, &sid);
        let from = |guild: Option<&str>, channel: u64| {
            Some(DiscordOrigin {
                user_id: OWNER.to_string(),
                channel_id: channel.to_string(),
                guild_id: guild.map(str::to_string),
            })
        };
        let shared = place
            .control(
                Control::Unorder(id.clone(), from(Some("900000000000000001"), LAB)),
                "discord:zeroaltitude",
            )
            .await;
        assert!(
            shared.starts_with(&format!("⚠️ {id} was not revoked: ")),
            "{shared}"
        );
        assert!(shared.contains("shared"), "{shared}");
        let live = core
            .standing_list(&theseus_protocol::StandingListParams::default(), cli())
            .unwrap();
        assert_eq!(live.records.len(), 1);
        let dm = place
            .control(
                Control::Unorder(id.clone(), from(None, OWNER + 1)),
                "discord:zeroaltitude",
            )
            .await;
        assert!(dm.starts_with(&format!("📌 Revoked `{id}`")), "{dm}");
        let live = core
            .standing_list(&theseus_protocol::StandingListParams::default(), cli())
            .unwrap();
        assert!(live.records.is_empty());
    }
}
