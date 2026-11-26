//! The AWS alerts subscription's confirmation from Discord, with no one at the machine (issue 59). A
//! child of `runtime`, on the shared pattern (`consent.rs`).
//!
//! A turn can only **offer** the control: `alerts.confirm_offer` posts one `aws_alerts` card to the
//! owner's direct messages, with **Confirm alerts**. Every step after that is the owner's own:
//!
//! - **Confirm alerts** (`awsc:in:alerts`) opens a form with one field: the Token=… value from the
//!   link in SNS's email, or the whole address (copied, never opened).
//! - Its submit is `aws.confirm_alerts { token }` as the presser, which the daemon judges
//!   (`rpc/aws.rs`: an owner, in a private place, read off the press as every press is) and
//!   confirms exactly as `theseus aws confirm-alerts` does.
//!
//! The token goes from the form to the daemon and nowhere else; every answer is fixed words.

use serde_json::Value;
use theseus_protocol::{error_code, method, AwsConfirmAlertsParams, AwsConfirmAlertsResult};
use twilight_model::application::interaction::modal::ModalInteractionData;
use twilight_model::application::interaction::Interaction;
use twilight_model::channel::message::component::{ButtonStyle, Component};

use super::consent::{self, button, row};
use super::Shared;
use crate::rpc_client::CallError;

const PREFIX: &str = "awsc";
/// The card's one target: the bound account's alerts subscription.
const TARGET: &str = "alerts";
/// The modal's custom id: `awsc:m:alerts:0`.
pub(crate) const MODAL: &str = "awsc:m:";

pub(crate) const NOT_HERE: &str = "🔐 Only you can confirm the alerts, and only in a private \
     place: your direct messages with Theseus, or a channel bound private. Nothing was confirmed.";
pub(crate) const NOT_THIS: &str = "🔐 That is not a confirmation token. Press **Confirm alerts** \
     again, and paste the long Token=… value from the link in SNS's email, or the whole address.";
pub(crate) const AWS_NO: &str =
    "🔐 AWS did not confirm it, and nothing was kept. Press **Confirm alerts** to try again.";

/// The card's one button.
pub(crate) fn buttons() -> Vec<Component> {
    row(vec![button(
        format!("{PREFIX}:in:{TARGET}"),
        "Confirm alerts",
        ButtonStyle::Primary,
    )])
}

/// Whether `custom_id` is this card's button.
pub(crate) fn parse(custom_id: &str) -> bool {
    custom_id
        .strip_prefix(PREFIX)
        .and_then(|r| r.strip_prefix(":in:"))
        == Some(TARGET)
}

/// The form the button opens: one field, for the token.
pub(crate) fn modal() -> Value {
    consent::form(
        &format!("{PREFIX}:m:{TARGET}"),
        false,
        "Confirm the alerts",
        "The token from SNS's email",
        "The Token=… value, or the whole link (copy it; do not open it). It goes to Theseus and \
         nowhere else.",
    )
}

/// What the presser is told when it is confirmed: the account, and whether only it can unsubscribe.
pub(crate) fn done_words(r: &AwsConfirmAlertsResult) -> String {
    if r.authenticated {
        format!(
            "✅ The alerts subscription is confirmed for `{}`: only the account can unsubscribe it.",
            r.account
        )
    } else {
        format!(
            "⚠️ The alerts subscription for `{}` was confirmed before without authentication, so \
             an alert's unsubscribe link still works: subscribe the address again and confirm the \
             new token.",
            r.account
        )
    }
}

/// A failure, in fixed words: never any part of what the daemon or the presser said.
pub(crate) fn failure(e: &CallError) -> &'static str {
    if e.code == error_code::REFUSED {
        NOT_HERE
    } else if e.message.starts_with("that is not a confirmation token") {
        NOT_THIS
    } else {
        AWS_NO
    }
}

impl Shared {
    /// The form's submit: the daemon's confirm, as the presser. The field goes into the request and
    /// nowhere else.
    pub(super) async fn aws_alerts_modal(
        &self,
        i: &Interaction,
        m: &ModalInteractionData,
        who: &str,
    ) {
        self.ack_ephemeral(i).await;
        let params = AwsConfirmAlertsParams {
            account: None,
            token: consent::pasted(m),
            author: Some(who.to_string()),
            discord: consent::presser(i),
        };
        let r = self
            .rpc
            .call::<_, AwsConfirmAlertsResult>(method::AWS_CONFIRM_ALERTS, params)
            .await;
        let text = match r {
            Ok(done) => done_words(&done),
            Err(e) => failure(&e).to_string(),
        };
        self.consent_finish(i, "aws alerts reply", &text, &[]).await;
    }
}

#[cfg(test)]
mod tests;
