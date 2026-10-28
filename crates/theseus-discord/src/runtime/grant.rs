//! A plan question's other yes on its card (theseus-lyxy): "Approve the plan
//! and edits mode", between Approve and Decline, when the daemon offers it.
//! A press approves and takes the offer: the answer echoes the pattern the
//! daemon offers now, read again from the question, never from the button
//! (whose id carries only the question), so a press can never ask for more
//! than was offered.

use theseus_core::Core;
use theseus_protocol::grants::{GrantAnswer, GrantOffer};
use twilight_model::channel::message::component::{ActionRow, Button, ButtonStyle};
use twilight_model::channel::message::Component;

/// A grant press's id: `confirm:grant:<corr>`, which `parse_confirm_id`
/// reads as an approval.
pub(crate) const VERB: &str = "grant";

/// Approve, the grant, and Decline.
pub(crate) fn grant_buttons(corr: &str, offer: &GrantOffer) -> Vec<Component> {
    let button = |verb: &str, label: &str, style| {
        Component::Button(Button {
            id: None,
            custom_id: Some(format!("confirm:{verb}:{corr}")),
            disabled: false,
            emoji: None,
            label: Some(label.to_string()),
            style,
            url: None,
            sku_id: None,
        })
    };
    vec![Component::ActionRow(ActionRow {
        id: None,
        components: vec![
            button("approve", "Approve", ButtonStyle::Success),
            button(VERB, offer.button(), ButtonStyle::Primary),
            button("decline", "Decline", ButtonStyle::Danger),
        ],
    })]
}

/// The grant a press takes: for a grant's press, the offer the question
/// carries now. None for any other press, or when it offers none (the
/// approval then goes without one).
pub(crate) fn answer_of(core: &Core, custom_id: &str, corr: &str) -> Option<GrantAnswer> {
    custom_id.strip_prefix(&format!("confirm:{VERB}:"))?;
    let a = core.kernel.action(corr).ok()??;
    let offer = core
        .pending_confirms(&a.session_id)
        .ok()?
        .into_iter()
        .find(|q| q.correlation_id == corr)?
        .grant_offer?;
    Some(GrantAnswer {
        pattern: offer.pattern,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plans_button_sits_between_approve_and_decline() {
        let c = grant_buttons("act_9", &GrantOffer::mode_edits());
        let Component::ActionRow(row) = &c[0] else {
            panic!()
        };
        let ids: Vec<(String, String)> = row
            .components
            .iter()
            .map(|b| match b {
                Component::Button(b) => (b.custom_id.clone().unwrap(), b.label.clone().unwrap()),
                _ => panic!(),
            })
            .collect();
        assert_eq!(
            ids,
            vec![
                ("confirm:approve:act_9".into(), "Approve".into()),
                (
                    "confirm:grant:act_9".into(),
                    "Approve the plan + edits mode".into()
                ),
                ("confirm:decline:act_9".into(), "Decline".into()),
            ]
        );
    }
}
