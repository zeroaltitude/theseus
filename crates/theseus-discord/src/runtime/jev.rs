//! Jev's live notices (step 24's notices, theseus-0j2.13): the owner's DM
//! gets `security.v3`'s notice after an open call it was sure was risky,
//! with three buttons, `right`, `wrong` and `noise`. A press is
//! `judge.label` on the whole judgment, as the presser, with the place it
//! came from, so the core judges it as it judges the cockpit's label: the
//! owner, from a private place. A counted press shows on the notice (the
//! core's `jev_labeled` post edits it, and its buttons go); a refused one
//! tells only the presser why. A child of `runtime`, apart from it for the
//! shape budget's file ceiling.

use serde_json::{json, Value};
use theseus_protocol::learning::JudgeLabelParams;
use theseus_protocol::{DiscordOrigin, LedgerKind};
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle, Component};
use twilight_model::http::interaction::InteractionResponseType;

use super::Shared;

/// A notice button's custom id: `jev:<label>:<judgment>`.
const PREFIX: &str = "jev";

/// The labels a notice offers, with their buttons' words.
const LABELS: [(&str, &str, ButtonStyle); 3] = [
    ("right", "Right: it was risky", ButtonStyle::Success),
    ("wrong", "Wrong: it was fine", ButtonStyle::Secondary),
    ("noise", "Noise", ButtonStyle::Danger),
];

/// A notice's three buttons, for its judgment.
pub(crate) fn buttons(judgment: &str) -> Vec<Component> {
    let components = LABELS
        .iter()
        .map(|(label, words, style)| {
            Component::Button(Button {
                id: None,
                custom_id: Some(format!("{PREFIX}:{label}:{judgment}")),
                disabled: false,
                emoji: None,
                label: Some((*words).into()),
                style: *style,
                url: None,
                sku_id: None,
            })
        })
        .collect();
    vec![Component::ActionRow(ActionRow {
        id: None,
        components,
    })]
}

/// A notice button's press: its label and judgment.
pub(crate) fn parse(custom_id: &str) -> Option<(String, String)> {
    let rest = custom_id.strip_prefix(PREFIX)?.strip_prefix(':')?;
    let (label, judgment) = rest.split_once(':')?;
    (LABELS.iter().any(|(l, _, _)| *l == label) && judgment.starts_with("jdg_"))
        .then(|| (label.to_string(), judgment.to_string()))
}

/// The notice's words, from the core's `jev_notice` post.
pub(crate) fn notice_text(body: &Value) -> String {
    let s = |k: &str| body[k].as_str().unwrap_or("");
    let task = match body["task"].as_str() {
        Some(t) => format!(" (task {t})"),
        None => String::new(),
    };
    format!(
        "🔔 notified after it ran{task}: `{}` · {}\n> {}\nThe call did not wait. Judgment `{}`.",
        s("tool"),
        s("line"),
        s("summary"),
        s("judgment")
    )
}

/// The notice once labeled: its words, the label and who gave it.
pub(crate) fn labeled_text(text: &str, body: &Value) -> String {
    let s = |k: &str| body[k].as_str().unwrap_or("");
    format!(
        "{text}\n🏷️ labeled **{}** by {} ({})",
        s("label"),
        s("who"),
        s("via")
    )
}

impl Shared {
    /// A notice's button: the label goes to the core as the presser, from
    /// where they pressed. The notice changes only by the core's post; a
    /// refused press tells the presser alone.
    pub(super) async fn jev_press(
        &self,
        i: &Interaction,
        (label, judgment): (String, String),
        who: &str,
        discord: Option<DiscordOrigin>,
    ) {
        self.respond(
            i,
            InteractionResponseType::DeferredUpdateMessage,
            None,
            false,
        )
        .await;
        let r = self
            .rpc
            .call::<_, Value>(
                theseus_protocol::method::JUDGE_LABEL,
                JudgeLabelParams {
                    judgment: judgment.clone(),
                    question: None,
                    label: json!(label),
                    note: None,
                    discord,
                },
            )
            .await;
        self.core.binding_ledger(
            LedgerKind::DiscordLabel,
            None,
            json!({"judgment": judgment, "label": label, "by": who, "ok": r.is_ok(),
                   "error": r.as_ref().err().map(|e| e.message.clone())}),
        );
        let Err(e) = r else { return };
        let why = e
            .data
            .get("why")
            .and_then(Value::as_str)
            .unwrap_or(&e.message);
        let text = match e.code == theseus_protocol::error_code::REFUSED {
            true => format!("🔐 Your label did not count: {why}."),
            false => format!("⚠️ The label was not written: {}", e.message),
        };
        self.followup(i, &text).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notices_buttons_name_their_label_and_judgment() {
        let b = buttons("jdg_0123");
        let Component::ActionRow(row) = &b[0] else {
            panic!("a row")
        };
        let ids: Vec<String> = row
            .components
            .iter()
            .filter_map(|c| match c {
                Component::Button(b) => b.custom_id.clone(),
                _ => None,
            })
            .collect();
        assert_eq!(
            ids,
            [
                "jev:right:jdg_0123",
                "jev:wrong:jdg_0123",
                "jev:noise:jdg_0123"
            ]
        );
        for id in &ids {
            assert!(id.len() <= 100, "Discord's limit");
            let (label, judgment) = parse(id).unwrap();
            assert_eq!(judgment, "jdg_0123");
            assert!(id.contains(&label));
        }
        assert_eq!(parse("jev:maybe:jdg_1"), None);
        assert_eq!(parse("jev:noise:act_1"), None);
        assert_eq!(parse("confirm:approve:act_1"), None);
    }
}
